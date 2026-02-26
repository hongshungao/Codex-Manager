use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use codexmanager_core::storage::now_ts;

const DEFAULT_ACCOUNT_COOLDOWN_SECS: i64 = 20;
const DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_429_SECS: i64 = 45;
const DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS: i64 = 30;
const DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS: i64 = 6;

const ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS: i64 = 30;

#[derive(Default)]
struct AccountCooldownState {
    entries: HashMap<String, i64>,
    last_cleanup_at: i64,
}

static ACCOUNT_COOLDOWN_UNTIL: OnceLock<Mutex<AccountCooldownState>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CooldownReason {
    Default,
    Network,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    Challenge,
}

fn cooldown_secs_for_reason(reason: CooldownReason) -> i64 {
    match reason {
        CooldownReason::Default => DEFAULT_ACCOUNT_COOLDOWN_SECS,
        CooldownReason::Network => DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS,
        CooldownReason::RateLimited => DEFAULT_ACCOUNT_COOLDOWN_429_SECS,
        CooldownReason::Upstream5xx => DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS,
        CooldownReason::Upstream4xx => DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS,
        CooldownReason::Challenge => DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS,
    }
}

pub(super) fn cooldown_reason_for_status(status: u16) -> CooldownReason {
    match status {
        429 => CooldownReason::RateLimited,
        500..=599 => CooldownReason::Upstream5xx,
        401 | 403 => CooldownReason::Challenge,
        400..=499 => CooldownReason::Upstream4xx,
        _ => CooldownReason::Default,
    }
}

pub(super) fn is_account_in_cooldown(account_id: &str) -> bool {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let Ok(mut state) = lock.lock() else {
        return false;
    };
    let now = now_ts();
    match state.entries.get(account_id).copied() {
        Some(until) if until > now => true,
        Some(_) => {
            state.entries.remove(account_id);
            false
        }
        None => false,
    }
}

pub(super) fn mark_account_cooldown(account_id: &str, reason: CooldownReason) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    if let Ok(mut state) = lock.lock() {
        super::record_gateway_cooldown_mark();
        let now = now_ts();
        maybe_cleanup_expired_cooldowns(&mut state, now);
        let cooldown_until = now + cooldown_secs_for_reason(reason);
        // 中文注释：同账号短时间内可能触发不同失败类型；保留更晚的 until 可避免被较短冷却覆盖。
        match state.entries.get_mut(account_id) {
            Some(until) => {
                if cooldown_until > *until {
                    *until = cooldown_until;
                }
            }
            None => {
                state.entries.insert(account_id.to_string(), cooldown_until);
            }
        }
    }
}

pub(super) fn mark_account_cooldown_for_status(account_id: &str, status: u16) {
    mark_account_cooldown(account_id, cooldown_reason_for_status(status));
}

pub(super) fn clear_account_cooldown(account_id: &str) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    if let Ok(mut state) = lock.lock() {
        state.entries.remove(account_id);
    }
}

fn maybe_cleanup_expired_cooldowns(state: &mut AccountCooldownState, now: i64) {
    if state.last_cleanup_at != 0
        && now.saturating_sub(state.last_cleanup_at) < ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    state.last_cleanup_at = now;
    state.entries.retain(|_, until| *until > now);
}

#[cfg(test)]
fn clear_account_cooldown_for_tests() {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    if let Ok(mut state) = lock.lock() {
        state.entries.clear();
        state.last_cleanup_at = 0;
    }
}

#[cfg(test)]
fn cooldown_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static COOLDOWN_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    COOLDOWN_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("cooldown test mutex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_evicts_expired_target_entry_without_full_scan() {
        let _guard = cooldown_test_guard();
        clear_account_cooldown_for_tests();
        let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
        let mut state = lock.lock().expect("cooldown state lock");
        let now = now_ts();
        state.entries.insert("acc-a".to_string(), now - 1);
        state.entries.insert("acc-b".to_string(), now - 1);
        drop(state);

        assert!(!is_account_in_cooldown("acc-a"));

        let state = lock.lock().expect("cooldown state lock");
        assert!(!state.entries.contains_key("acc-a"));
        assert!(state.entries.contains_key("acc-b"));
    }

    #[test]
    fn mark_path_cleanup_prunes_expired_entries() {
        let _guard = cooldown_test_guard();
        clear_account_cooldown_for_tests();
        let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
        let mut state = lock.lock().expect("cooldown state lock");
        let now = now_ts();
        state.entries.insert("stale".to_string(), now - 1);
        state.last_cleanup_at = now - ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS - 1;
        drop(state);

        mark_account_cooldown("fresh", CooldownReason::Default);

        let state = lock.lock().expect("cooldown state lock");
        assert!(!state.entries.contains_key("stale"));
        assert!(state.entries.contains_key("fresh"));
    }
}
