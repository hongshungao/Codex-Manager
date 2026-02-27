use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::account_availability::is_available;

static CANDIDATE_SNAPSHOT_CACHE: OnceLock<Mutex<Option<CandidateSnapshotCache>>> = OnceLock::new();
static SELECTION_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static CANDIDATE_CACHE_TTL_MS: AtomicU64 = AtomicU64::new(DEFAULT_CANDIDATE_CACHE_TTL_MS);
static CURRENT_DB_PATH: OnceLock<RwLock<String>> = OnceLock::new();
const DEFAULT_CANDIDATE_CACHE_TTL_MS: u64 = 500;
const CANDIDATE_CACHE_TTL_ENV: &str = "CODEXMANAGER_CANDIDATE_CACHE_TTL_MS";

#[derive(Clone)]
struct CandidateSnapshotCache {
    db_path: String,
    expires_at: Instant,
    candidates: Vec<(Account, Token)>,
}

pub(crate) fn collect_gateway_candidates(storage: &Storage) -> Result<Vec<(Account, Token)>, String> {
    if let Some(cached) = read_candidate_cache() {
        return Ok(cached);
    }

    let candidates = collect_gateway_candidates_uncached(storage)?;
    write_candidate_cache(candidates.clone());
    Ok(candidates)
}

fn collect_gateway_candidates_uncached(storage: &Storage) -> Result<Vec<(Account, Token)>, String> {
    // 选择可用账号作为网关上游候选
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    let snaps = storage
        .latest_usage_snapshots_by_account()
        .map_err(|e| e.to_string())?;
    let mut token_map = HashMap::new();
    for token in tokens {
        token_map.insert(token.account_id.clone(), token);
    }
    let mut snap_map = HashMap::new();
    for snap in snaps {
        snap_map.insert(snap.account_id.clone(), snap);
    }

    let mut out = Vec::new();
    for account in &accounts {
        if account.status != "active" {
            continue;
        }
        let token = match token_map.get(&account.id) {
            Some(token) => token.clone(),
            None => continue,
        };
        let usage = snap_map.get(&account.id);
        if !is_available(usage) {
            continue;
        }
        out.push((account.clone(), token));
    }
    if out.is_empty() {
        let mut fallback = Vec::new();
        for account in &accounts {
            let token = match token_map.get(&account.id) {
                Some(token) => token.clone(),
                None => continue,
            };
            let usage = snap_map.get(&account.id);
            if !fallback_allowed(usage) {
                continue;
            }
            fallback.push((account.clone(), token));
        }
        if !fallback.is_empty() {
            log::warn!("gateway fallback: no active accounts, using {} candidates", fallback.len());
            return Ok(fallback);
        }
    }
    if out.is_empty() {
        log_no_candidates(&accounts, &token_map, &snap_map);
    }
    Ok(out)
}

fn read_candidate_cache() -> Option<Vec<(Account, Token)>> {
    let ttl = candidate_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let db_path = current_db_path();
    let now = Instant::now();
    let mutex = CANDIDATE_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("candidate snapshot cache lock poisoned; dropping cache and continuing");
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        }
    };
    let cached = guard.as_ref()?;
    if cached.db_path != db_path || cached.expires_at <= now {
        *guard = None;
        return None;
    }
    Some(cached.candidates.clone())
}

fn write_candidate_cache(candidates: Vec<(Account, Token)>) {
    let ttl = candidate_cache_ttl();
    if ttl.is_zero() {
        return;
    }
    let db_path = current_db_path();
    let expires_at = Instant::now() + ttl;
    let mutex = CANDIDATE_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("candidate snapshot cache lock poisoned; recovering");
            poisoned.into_inner()
        }
    };
    *guard = Some(CandidateSnapshotCache {
        db_path,
        expires_at,
        candidates,
    });
}

fn candidate_cache_ttl() -> Duration {
    ensure_selection_config_loaded();
    let ttl_ms = CANDIDATE_CACHE_TTL_MS.load(Ordering::Relaxed);
    Duration::from_millis(ttl_ms)
}

fn current_db_path() -> String {
    ensure_selection_config_loaded();
    crate::lock_utils::read_recover(current_db_path_cell(), "current_db_path").clone()
}

fn fallback_allowed(usage: Option<&UsageSnapshotRecord>) -> bool {
    if let Some(record) = usage {
        if let Some(value) = record.used_percent {
            if value >= 100.0 {
                return false;
            }
        }
        if let Some(value) = record.secondary_used_percent {
            if value >= 100.0 {
                return false;
            }
        }
    }
    true
}

fn log_no_candidates(
    accounts: &[Account],
    token_map: &HashMap<String, Token>,
    snap_map: &HashMap<String, UsageSnapshotRecord>,
) {
    let db_path = current_db_path();
    log::warn!(
        "gateway no candidates: db_path={}, accounts={}, tokens={}, snapshots={}",
        db_path,
        accounts.len(),
        token_map.len(),
        snap_map.len()
    );
    for account in accounts {
        let usage = snap_map.get(&account.id);
        log::warn!(
            "gateway account: id={}, status={}, has_token={}, primary=({:?}/{:?}) secondary=({:?}/{:?})",
            account.id,
            account.status,
            token_map.contains_key(&account.id),
            usage.and_then(|u| u.used_percent),
            usage.and_then(|u| u.window_minutes),
            usage.and_then(|u| u.secondary_used_percent),
            usage.and_then(|u| u.secondary_window_minutes),
        );
    }
}

pub(super) fn reload_from_env() {
    let ttl_ms = std::env::var(CANDIDATE_CACHE_TTL_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CANDIDATE_CACHE_TTL_MS);
    CANDIDATE_CACHE_TTL_MS.store(ttl_ms, Ordering::Relaxed);

    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    let mut cached = crate::lock_utils::write_recover(current_db_path_cell(), "current_db_path");
    *cached = db_path;
}

fn ensure_selection_config_loaded() {
    let _ = SELECTION_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

fn current_db_path_cell() -> &'static RwLock<String> {
    CURRENT_DB_PATH.get_or_init(|| RwLock::new("<unset>".to_string()))
}

#[cfg(test)]
fn clear_candidate_cache_for_tests() {
    if let Some(mutex) = CANDIDATE_SNAPSHOT_CACHE.get() {
        let mut guard = match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("candidate snapshot cache lock poisoned; recovering for tests");
                poisoned.into_inner()
            }
        };
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{clear_candidate_cache_for_tests, collect_gateway_candidates, CANDIDATE_CACHE_TTL_ENV};
    use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};
    use std::sync::Mutex;

    static CANDIDATE_CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn candidate_snapshot_cache_reuses_recent_snapshot() {
        let _guard = CANDIDATE_CACHE_TEST_LOCK.lock().expect("lock");
        let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "2000");
        super::reload_from_env();
        clear_candidate_cache_for_tests();

        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        storage
            .insert_account(&Account {
                id: "acc-cache-1".to_string(),
                label: "cached".to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: "acc-cache-1".to_string(),
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: now_ts(),
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-cache-1".to_string(),
                used_percent: Some(10.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now_ts(),
            })
            .expect("insert snapshot");

        let first = collect_gateway_candidates(&storage).expect("first candidates");
        assert_eq!(first.len(), 1);

        storage
            .update_account_status("acc-cache-1", "inactive")
            .expect("mark inactive");
        let second = collect_gateway_candidates(&storage).expect("second candidates");
        assert_eq!(second.len(), 1);

        clear_candidate_cache_for_tests();
        if let Some(value) = previous_ttl {
            std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
        } else {
            std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
        }
        super::reload_from_env();
    }

    #[test]
    fn candidates_follow_account_sort_order() {
        let _guard = CANDIDATE_CACHE_TEST_LOCK.lock().expect("lock");
        let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "0");
        super::reload_from_env();
        clear_candidate_cache_for_tests();

        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let now = now_ts();
        let accounts = vec![
            ("acc-sort-10", 10_i64),
            ("acc-sort-0", 0_i64),
            ("acc-sort-1", 1_i64),
        ];
        for (id, sort) in &accounts {
            storage
                .insert_account(&Account {
                    id: (*id).to_string(),
                    label: (*id).to_string(),
                    issuer: "issuer".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: *sort,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                })
                .expect("insert account");
            storage
                .insert_token(&Token {
                    account_id: (*id).to_string(),
                    id_token: "id".to_string(),
                    access_token: "access".to_string(),
                    refresh_token: "refresh".to_string(),
                    api_key_access_token: None,
                    last_refresh: now,
                })
                .expect("insert token");
            storage
                .insert_usage_snapshot(&UsageSnapshotRecord {
                    account_id: (*id).to_string(),
                    used_percent: Some(10.0),
                    window_minutes: Some(300),
                    resets_at: None,
                    secondary_used_percent: None,
                    secondary_window_minutes: None,
                    secondary_resets_at: None,
                    credits_json: None,
                    captured_at: now,
                })
                .expect("insert usage");
        }

        let candidates = collect_gateway_candidates(&storage).expect("collect candidates");
        let ordered_ids = candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ordered_ids, vec!["acc-sort-0", "acc-sort-1", "acc-sort-10"]);

        clear_candidate_cache_for_tests();
        if let Some(value) = previous_ttl {
            std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
        } else {
            std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
        }
        super::reload_from_env();
    }
}
