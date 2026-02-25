use codexmanager_core::storage::{Account, Token};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

const ROUTE_STRATEGY_ENV: &str = "CODEXMANAGER_ROUTE_STRATEGY";
const ROUTE_MODE_ORDERED: u8 = 0;
const ROUTE_MODE_BALANCED_ROUND_ROBIN: u8 = 1;
const ROUTE_STRATEGY_ORDERED: &str = "ordered";
const ROUTE_STRATEGY_BALANCED: &str = "balanced";

static ROUTE_MODE: AtomicU8 = AtomicU8::new(ROUTE_MODE_ORDERED);
static ROUTE_STATE: OnceLock<Mutex<RouteRoundRobinState>> = OnceLock::new();
static ROUTE_CONFIG_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Default)]
struct RouteRoundRobinState {
    next_start_by_key_model: HashMap<String, usize>,
}

pub(crate) fn apply_route_strategy(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
) {
    ensure_route_config_loaded();
    if candidates.len() <= 1 {
        return;
    }
    if route_mode() != ROUTE_MODE_BALANCED_ROUND_ROBIN {
        return;
    }

    let start = next_start_index(key_id, model, candidates.len());
    if start > 0 {
        candidates.rotate_left(start);
    }
}

fn route_mode() -> u8 {
    ROUTE_MODE.load(Ordering::Relaxed)
}

fn route_mode_label(mode: u8) -> &'static str {
    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        ROUTE_STRATEGY_BALANCED
    } else {
        ROUTE_STRATEGY_ORDERED
    }
}

fn parse_route_mode(raw: &str) -> Option<u8> {
    match raw.trim().to_ascii_lowercase().as_str() {
        ROUTE_STRATEGY_ORDERED | "order" | "priority" | "sequential" => Some(ROUTE_MODE_ORDERED),
        ROUTE_STRATEGY_BALANCED | "round_robin" | "round-robin" | "rr" => {
            Some(ROUTE_MODE_BALANCED_ROUND_ROBIN)
        }
        _ => None,
    }
}

pub(crate) fn current_route_strategy() -> &'static str {
    ensure_route_config_loaded();
    route_mode_label(route_mode())
}

pub(crate) fn set_route_strategy(strategy: &str) -> Result<&'static str, String> {
    let Some(mode) = parse_route_mode(strategy) else {
        return Err(
            "invalid strategy; use ordered or balanced (aliases: round_robin/round-robin/rr)"
                .to_string(),
        );
    };
    ROUTE_MODE.store(mode, Ordering::Relaxed);
    if let Some(lock) = ROUTE_STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.next_start_by_key_model.clear();
        }
    }
    Ok(route_mode_label(mode))
}

fn next_start_index(key_id: &str, model: Option<&str>, candidate_count: usize) -> usize {
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let Ok(mut state) = lock.lock() else {
        return 0;
    };
    let key = key_model_key(key_id, model);
    let next = state.next_start_by_key_model.entry(key).or_insert(0);
    let start = *next % candidate_count;
    *next = (start + 1) % candidate_count;
    start
}

fn key_model_key(key_id: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}",
        key_id.trim(),
        model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("-")
    )
}

pub(super) fn reload_from_env() {
    let raw = std::env::var(ROUTE_STRATEGY_ENV).unwrap_or_default();
    let mode = parse_route_mode(raw.as_str()).unwrap_or(ROUTE_MODE_ORDERED);
    ROUTE_MODE.store(mode, Ordering::Relaxed);

    if let Some(lock) = ROUTE_STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.next_start_by_key_model.clear();
        }
    }
}

fn ensure_route_config_loaded() {
    let _ = ROUTE_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

#[cfg(test)]
fn clear_route_state_for_tests() {
    if let Some(lock) = ROUTE_STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.next_start_by_key_model.clear();
        }
    }
}

#[cfg(test)]
fn route_strategy_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static ROUTE_STRATEGY_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    ROUTE_STRATEGY_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("route strategy test mutex")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_list() -> Vec<(Account, Token)> {
        vec![
            (
                Account {
                    id: "acc-a".to_string(),
                    label: "".to_string(),
                    issuer: "".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: 0,
                    status: "active".to_string(),
                    created_at: 0,
                    updated_at: 0,
                },
                Token {
                    account_id: "acc-a".to_string(),
                    id_token: "".to_string(),
                    access_token: "".to_string(),
                    refresh_token: "".to_string(),
                    api_key_access_token: None,
                    last_refresh: 0,
                },
            ),
            (
                Account {
                    id: "acc-b".to_string(),
                    label: "".to_string(),
                    issuer: "".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: 1,
                    status: "active".to_string(),
                    created_at: 0,
                    updated_at: 0,
                },
                Token {
                    account_id: "acc-b".to_string(),
                    id_token: "".to_string(),
                    access_token: "".to_string(),
                    refresh_token: "".to_string(),
                    api_key_access_token: None,
                    last_refresh: 0,
                },
            ),
            (
                Account {
                    id: "acc-c".to_string(),
                    label: "".to_string(),
                    issuer: "".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: 2,
                    status: "active".to_string(),
                    created_at: 0,
                    updated_at: 0,
                },
                Token {
                    account_id: "acc-c".to_string(),
                    id_token: "".to_string(),
                    access_token: "".to_string(),
                    refresh_token: "".to_string(),
                    api_key_access_token: None,
                    last_refresh: 0,
                },
            ),
        ]
    }

    fn account_ids(candidates: &[(Account, Token)]) -> Vec<String> {
        candidates.iter().map(|(account, _)| account.id.clone()).collect()
    }

    #[test]
    fn defaults_to_ordered_strategy() {
        let _guard = route_strategy_test_guard();
        let previous = std::env::var(ROUTE_STRATEGY_ENV).ok();
        std::env::remove_var(ROUTE_STRATEGY_ENV);
        reload_from_env();
        clear_route_state_for_tests();

        let mut candidates = candidate_list();
        apply_route_strategy(&mut candidates, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(
            account_ids(&candidates),
            vec!["acc-a".to_string(), "acc-b".to_string(), "acc-c".to_string()]
        );

        if let Some(value) = previous {
            std::env::set_var(ROUTE_STRATEGY_ENV, value);
        } else {
            std::env::remove_var(ROUTE_STRATEGY_ENV);
        }
        reload_from_env();
    }

    #[test]
    fn balanced_round_robin_rotates_start_by_key_and_model() {
        let _guard = route_strategy_test_guard();
        let previous = std::env::var(ROUTE_STRATEGY_ENV).ok();
        std::env::set_var(ROUTE_STRATEGY_ENV, "balanced");
        reload_from_env();
        clear_route_state_for_tests();

        let mut first = candidate_list();
        apply_route_strategy(&mut first, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(
            account_ids(&first),
            vec!["acc-a".to_string(), "acc-b".to_string(), "acc-c".to_string()]
        );

        let mut second = candidate_list();
        apply_route_strategy(&mut second, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(
            account_ids(&second),
            vec!["acc-b".to_string(), "acc-c".to_string(), "acc-a".to_string()]
        );

        let mut third = candidate_list();
        apply_route_strategy(&mut third, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(
            account_ids(&third),
            vec!["acc-c".to_string(), "acc-a".to_string(), "acc-b".to_string()]
        );

        if let Some(value) = previous {
            std::env::set_var(ROUTE_STRATEGY_ENV, value);
        } else {
            std::env::remove_var(ROUTE_STRATEGY_ENV);
        }
        reload_from_env();
    }

    #[test]
    fn balanced_round_robin_isolated_by_key_and_model() {
        let _guard = route_strategy_test_guard();
        let previous = std::env::var(ROUTE_STRATEGY_ENV).ok();
        std::env::set_var(ROUTE_STRATEGY_ENV, "balanced");
        reload_from_env();
        clear_route_state_for_tests();

        let mut gpt_first = candidate_list();
        apply_route_strategy(&mut gpt_first, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(account_ids(&gpt_first)[0], "acc-a");

        let mut gpt_second = candidate_list();
        apply_route_strategy(&mut gpt_second, "gk_1", Some("gpt-5.3-codex"));
        assert_eq!(account_ids(&gpt_second)[0], "acc-b");

        let mut o3_first = candidate_list();
        apply_route_strategy(&mut o3_first, "gk_1", Some("o3"));
        assert_eq!(account_ids(&o3_first)[0], "acc-a");

        let mut other_key_first = candidate_list();
        apply_route_strategy(&mut other_key_first, "gk_2", Some("gpt-5.3-codex"));
        assert_eq!(account_ids(&other_key_first)[0], "acc-a");

        if let Some(value) = previous {
            std::env::set_var(ROUTE_STRATEGY_ENV, value);
        } else {
            std::env::remove_var(ROUTE_STRATEGY_ENV);
        }
        reload_from_env();
    }

    #[test]
    fn set_route_strategy_accepts_aliases_and_reports_canonical_name() {
        let _guard = route_strategy_test_guard();
        clear_route_state_for_tests();
        assert_eq!(set_route_strategy("ordered").expect("set ordered"), "ordered");
        assert_eq!(
            set_route_strategy("round_robin").expect("set rr alias"),
            "balanced"
        );
        assert_eq!(current_route_strategy(), "balanced");
        assert!(set_route_strategy("unsupported").is_err());
    }
}
