use codexmanager_core::storage::now_ts;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default)]
struct RouteQualityRecord {
    success_2xx: u32,
    challenge_403: u32,
    throttle_429: u32,
    updated_at: i64,
}

static ROUTE_QUALITY: OnceLock<Mutex<RouteQualityState>> = OnceLock::new();
const ROUTE_QUALITY_TTL_SECS: i64 = 24 * 60 * 60;
const ROUTE_QUALITY_CLEANUP_INTERVAL_SECS: i64 = 60;

#[derive(Default)]
struct RouteQualityState {
    entries: HashMap<String, RouteQualityRecord>,
    last_cleanup_at: i64,
}

fn with_map_mut<F>(mutator: F)
where
    F: FnOnce(&mut HashMap<String, RouteQualityRecord>, i64),
{
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(RouteQualityState::default()));
    let Ok(mut state) = lock.lock() else {
        return;
    };
    let now = now_ts();
    maybe_cleanup_route_quality(&mut state, now);
    mutator(&mut state.entries, now);
}

pub(crate) fn record_route_quality(account_id: &str, status_code: u16) {
    with_map_mut(|map, now| {
        let record = map.entry(account_id.to_string()).or_default();
        record.updated_at = now;
        if (200..300).contains(&status_code) {
            record.success_2xx = record.success_2xx.saturating_add(1);
            return;
        }
        if status_code == 403 {
            record.challenge_403 = record.challenge_403.saturating_add(1);
            return;
        }
        if status_code == 429 {
            record.throttle_429 = record.throttle_429.saturating_add(1);
        }
    });
}

#[allow(dead_code)]
pub(crate) fn route_quality_penalty(account_id: &str) -> i64 {
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(RouteQualityState::default()));
    let Ok(mut state) = lock.lock() else {
        return 0;
    };
    let now = now_ts();
    let Some(record) = state.entries.get(account_id).cloned() else {
        return 0;
    };
    if route_quality_record_expired(&record, now) {
        state.entries.remove(account_id);
        return 0;
    }
    i64::from(record.challenge_403) * 6 + i64::from(record.throttle_429) * 3
        - i64::from(record.success_2xx) * 2
}

#[cfg(test)]
pub(crate) fn clear_route_quality_for_tests() {
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(RouteQualityState::default()));
    if let Ok(mut state) = lock.lock() {
        state.entries.clear();
        state.last_cleanup_at = 0;
    }
}

#[cfg(test)]
fn route_quality_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static ROUTE_QUALITY_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    ROUTE_QUALITY_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("route quality test mutex")
}

fn maybe_cleanup_route_quality(state: &mut RouteQualityState, now: i64) {
    if state.last_cleanup_at != 0
        && now.saturating_sub(state.last_cleanup_at) < ROUTE_QUALITY_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    state.last_cleanup_at = now;
    state
        .entries
        .retain(|_, value| !route_quality_record_expired(value, now));
}

fn route_quality_record_expired(record: &RouteQualityRecord, now: i64) -> bool {
    record.updated_at + ROUTE_QUALITY_TTL_SECS <= now
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_quality_penalty_prefers_successful_accounts() {
        let _guard = route_quality_test_guard();
        clear_route_quality_for_tests();
        record_route_quality("acc_a", 403);
        record_route_quality("acc_a", 403);
        record_route_quality("acc_b", 200);
        assert!(route_quality_penalty("acc_a") > route_quality_penalty("acc_b"));
    }

    #[test]
    fn route_quality_penalty_evicts_expired_record() {
        let _guard = route_quality_test_guard();
        clear_route_quality_for_tests();
        let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(RouteQualityState::default()));
        let mut state = lock.lock().expect("route quality state lock");
        let now = now_ts();
        state.entries.insert(
            "acc_old".to_string(),
            RouteQualityRecord {
                success_2xx: 0,
                challenge_403: 1,
                throttle_429: 0,
                updated_at: now - ROUTE_QUALITY_TTL_SECS - 1,
            },
        );
        drop(state);

        assert_eq!(route_quality_penalty("acc_old"), 0);
        let state = lock.lock().expect("route quality state lock");
        assert!(!state.entries.contains_key("acc_old"));
    }

    #[test]
    fn record_path_cleanup_prunes_expired_records() {
        let _guard = route_quality_test_guard();
        clear_route_quality_for_tests();
        let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(RouteQualityState::default()));
        let mut state = lock.lock().expect("route quality state lock");
        let now = now_ts();
        state.entries.insert(
            "acc_stale".to_string(),
            RouteQualityRecord {
                success_2xx: 0,
                challenge_403: 1,
                throttle_429: 0,
                updated_at: now - ROUTE_QUALITY_TTL_SECS - 1,
            },
        );
        state.last_cleanup_at = now - ROUTE_QUALITY_CLEANUP_INTERVAL_SECS - 1;
        drop(state);

        record_route_quality("acc_fresh", 200);
        let state = lock.lock().expect("route quality state lock");
        assert!(!state.entries.contains_key("acc_stale"));
        assert!(state.entries.contains_key("acc_fresh"));
    }
}
