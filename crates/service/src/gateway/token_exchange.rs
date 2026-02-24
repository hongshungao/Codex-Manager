use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use codexmanager_core::storage::{now_ts, Account, Storage, Token};

use crate::auth_tokens;
use crate::usage_http::refresh_access_token;

const ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS: i64 = 30 * 60;
const ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS: i64 = 60;

struct AccountTokenExchangeLockEntry {
    lock: Arc<Mutex<()>>,
    last_seen_at: i64,
}

#[derive(Default)]
struct AccountTokenExchangeLockTable {
    entries: HashMap<String, AccountTokenExchangeLockEntry>,
    last_cleanup_at: i64,
}

static ACCOUNT_TOKEN_EXCHANGE_LOCKS: OnceLock<Mutex<AccountTokenExchangeLockTable>> =
    OnceLock::new();

pub(super) fn account_token_exchange_lock(account_id: &str) -> Arc<Mutex<()>> {
    let lock =
        ACCOUNT_TOKEN_EXCHANGE_LOCKS.get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    let Ok(mut table) = lock.lock() else {
        return Arc::new(Mutex::new(()));
    };
    let now = now_ts();
    maybe_cleanup_exchange_locks(&mut table, now);
    let entry = table
        .entries
        .entry(account_id.to_string())
        .or_insert_with(|| AccountTokenExchangeLockEntry {
            lock: Arc::new(Mutex::new(())),
            last_seen_at: now,
        });
    entry.last_seen_at = now;
    entry.lock.clone()
}

fn maybe_cleanup_exchange_locks(table: &mut AccountTokenExchangeLockTable, now: i64) {
    if table.last_cleanup_at != 0
        && now.saturating_sub(table.last_cleanup_at) < ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    table.last_cleanup_at = now;
    table.entries.retain(|_, entry| {
        let stale = now.saturating_sub(entry.last_seen_at) > ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS;
        !stale || Arc::strong_count(&entry.lock) > 1
    });
}

fn find_cached_api_key_access_token(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .list_tokens()
        .ok()?
        .into_iter()
        .find(|t| t.account_id == account_id)
        .and_then(|t| t.api_key_access_token)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn exchange_and_persist_api_key_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> Result<String, String> {
    let exchanged = auth_tokens::obtain_api_key(issuer, client_id, &token.id_token)?;
    token.api_key_access_token = Some(exchanged.clone());
    let _ = storage.insert_token(token);
    Ok(exchanged)
}

fn fallback_to_access_token(token: &Token, exchange_error: &str) -> Result<String, String> {
    let fallback = token.access_token.trim();
    if fallback.is_empty() {
        return Err(exchange_error.to_string());
    }
    log::warn!(
        "api_key_access_token exchange unavailable; fallback to access_token: {}",
        exchange_error
    );
    Ok(fallback.to_string())
}

pub(super) fn resolve_openai_bearer_token(
    storage: &Storage,
    account: &Account,
    token: &mut Token,
) -> Result<String, String> {
    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(existing.to_string());
    }

    let exchange_lock = account_token_exchange_lock(&account.id);
    let _guard = exchange_lock
        .lock()
        .map_err(|_| "token exchange lock poisoned".to_string())?;

    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(existing.to_string());
    }

    if let Some(cached) = find_cached_api_key_access_token(storage, &account.id) {
        // 中文注释：并发下后到线程优先复用已落库的新 token，避免重复 token exchange 打上游。
        token.api_key_access_token = Some(cached.clone());
        return Ok(cached);
    }

    let client_id = super::runtime_config::token_exchange_client_id();
    let issuer_env = super::runtime_config::token_exchange_default_issuer();
    let issuer = if account.issuer.trim().is_empty() {
        issuer_env
    } else {
        account.issuer.clone()
    };

    match exchange_and_persist_api_key_access_token(storage, token, &issuer, &client_id) {
        Ok(token) => return Ok(token),
        Err(exchange_err) => {
            if !token.refresh_token.trim().is_empty() {
                match refresh_access_token(&issuer, &client_id, &token.refresh_token) {
                    Ok(refreshed) => {
                        token.access_token = refreshed.access_token;
                        if let Some(refresh_token) = refreshed.refresh_token {
                            token.refresh_token = refresh_token;
                        }
                        if let Some(id_token) = refreshed.id_token {
                            token.id_token = id_token;
                        }
                        let _ = storage.insert_token(token);

                        if !token.id_token.trim().is_empty() {
                            if let Ok(exchanged) = exchange_and_persist_api_key_access_token(
                                storage, token, &issuer, &client_id,
                            ) {
                                return Ok(exchanged);
                            }
                        }
                    }
                    Err(refresh_err) => {
                        log::warn!(
                            "refresh token before api_key_access_token exchange failed: {}",
                            refresh_err
                        );
                    }
                }
            }

            fallback_to_access_token(token, &exchange_err)
        }
    }
}

#[cfg(test)]
fn clear_account_token_exchange_locks_for_tests() {
    let lock =
        ACCOUNT_TOKEN_EXCHANGE_LOCKS.get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    if let Ok(mut table) = lock.lock() {
        table.entries.clear();
        table.last_cleanup_at = 0;
    }
}

#[cfg(test)]
fn token_exchange_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TOKEN_EXCHANGE_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    TOKEN_EXCHANGE_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("token exchange test mutex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_account_reuses_exchange_lock() {
        let _guard = token_exchange_test_guard();
        clear_account_token_exchange_locks_for_tests();
        let first = account_token_exchange_lock("acc-1");
        let second = account_token_exchange_lock("acc-1");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn stale_unshared_exchange_lock_entry_is_reclaimed() {
        let _guard = token_exchange_test_guard();
        clear_account_token_exchange_locks_for_tests();
        let first = account_token_exchange_lock("acc-1");
        let weak = Arc::downgrade(&first);
        drop(first);

        let lock =
            ACCOUNT_TOKEN_EXCHANGE_LOCKS.get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
        let mut table = lock.lock().expect("token exchange table lock");
        let now = now_ts();
        table
            .entries
            .get_mut("acc-1")
            .expect("token exchange entry")
            .last_seen_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS - 1;
        table.last_cleanup_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS - 1;
        drop(table);

        let _second = account_token_exchange_lock("acc-1");
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn stale_shared_exchange_lock_entry_is_not_reclaimed() {
        let _guard = token_exchange_test_guard();
        clear_account_token_exchange_locks_for_tests();
        let first = account_token_exchange_lock("acc-1");

        let lock =
            ACCOUNT_TOKEN_EXCHANGE_LOCKS.get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
        let mut table = lock.lock().expect("token exchange table lock");
        let now = now_ts();
        table
            .entries
            .get_mut("acc-1")
            .expect("token exchange entry")
            .last_seen_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS - 1;
        table.last_cleanup_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS - 1;
        drop(table);

        let second = account_token_exchange_lock("acc-1");
        assert!(Arc::ptr_eq(&first, &second));
    }
}
