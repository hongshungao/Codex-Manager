use codexmanager_core::storage::now_ts;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const ROUTE_HINT_TTL_SECS: i64 = 30 * 60;
const ROUTE_HINT_CLEANUP_INTERVAL_SECS: i64 = 30;

#[derive(Debug, Clone)]
struct RouteHintRecord {
    account_id: String,
    expires_at: i64,
}

#[derive(Default)]
struct RouteHintState {
    entries: HashMap<String, RouteHintRecord>,
    last_cleanup_at: i64,
}

static ROUTE_HINTS: OnceLock<Mutex<RouteHintState>> = OnceLock::new();

fn hint_key(key_id: &str, path: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("-")
    )
}

pub(crate) fn preferred_route_account(
    key_id: &str,
    path: &str,
    model: Option<&str>,
) -> Option<String> {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(RouteHintState::default()));
    let Ok(mut state) = lock.lock() else {
        return None;
    };
    let key = hint_key(key_id, path, model);
    let now = now_ts();
    match state.entries.get(&key) {
        Some(value) if value.expires_at > now => Some(value.account_id.clone()),
        Some(_) => {
            state.entries.remove(&key);
            None
        }
        None => None,
    }
}

pub(crate) fn remember_success_route_account(
    key_id: &str,
    path: &str,
    model: Option<&str>,
    account_id: &str,
) {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(RouteHintState::default()));
    let Ok(mut state) = lock.lock() else {
        return;
    };
    let now = now_ts();
    maybe_cleanup_route_hints(&mut state, now);
    let key = hint_key(key_id, path, model);
    state.entries.insert(
        key,
        RouteHintRecord {
            account_id: account_id.to_string(),
            expires_at: now + ROUTE_HINT_TTL_SECS,
        },
    );
}

#[cfg(test)]
pub(crate) fn clear_route_hints_for_tests() {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(RouteHintState::default()));
    if let Ok(mut state) = lock.lock() {
        state.entries.clear();
        state.last_cleanup_at = 0;
    }
}

#[cfg(test)]
fn route_hint_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static ROUTE_HINT_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    ROUTE_HINT_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("route hint test mutex")
}

fn maybe_cleanup_route_hints(state: &mut RouteHintState, now: i64) {
    if state.last_cleanup_at != 0
        && now.saturating_sub(state.last_cleanup_at) < ROUTE_HINT_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    state.last_cleanup_at = now;
    state.entries.retain(|_, value| value.expires_at > now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_route_account_returns_last_successful_account() {
        let _guard = route_hint_test_guard();
        clear_route_hints_for_tests();
        assert_eq!(
            preferred_route_account("gk_1", "/v1/responses", Some("gpt-5.3-codex")),
            None
        );
        remember_success_route_account(
            "gk_1",
            "/v1/responses",
            Some("gpt-5.3-codex"),
            "acc_2",
        );
        assert_eq!(
            preferred_route_account("gk_1", "/v1/responses", Some("gpt-5.3-codex"))
                .as_deref(),
            Some("acc_2")
        );
    }

    #[test]
    fn lookup_evicts_expired_target_hint() {
        let _guard = route_hint_test_guard();
        clear_route_hints_for_tests();
        let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(RouteHintState::default()));
        let mut state = lock.lock().expect("route hint state lock");
        let key = hint_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        state.entries.insert(
            key.clone(),
            RouteHintRecord {
                account_id: "acc_2".to_string(),
                expires_at: now_ts() - 1,
            },
        );
        drop(state);

        assert_eq!(
            preferred_route_account("gk_1", "/v1/responses", Some("gpt-5.3-codex")),
            None
        );
        let state = lock.lock().expect("route hint state lock");
        assert!(!state.entries.contains_key(&key));
    }

    #[test]
    fn remember_path_cleanup_prunes_expired_hints() {
        let _guard = route_hint_test_guard();
        clear_route_hints_for_tests();
        let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(RouteHintState::default()));
        let mut state = lock.lock().expect("route hint state lock");
        let now = now_ts();
        state.entries.insert(
            hint_key("gk_stale", "/v1/responses", Some("gpt-5.3-codex")),
            RouteHintRecord {
                account_id: "acc_stale".to_string(),
                expires_at: now - 1,
            },
        );
        state.last_cleanup_at = now - ROUTE_HINT_CLEANUP_INTERVAL_SECS - 1;
        drop(state);

        remember_success_route_account(
            "gk_1",
            "/v1/responses",
            Some("gpt-5.3-codex"),
            "acc_2",
        );
        let state = lock.lock().expect("route hint state lock");
        assert!(!state.entries.contains_key(&hint_key(
            "gk_stale",
            "/v1/responses",
            Some("gpt-5.3-codex"),
        )));
    }
}
