use codexmanager_core::storage::{Account, Token};
use super::route_quality::route_health_score;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const ROUTE_STRATEGY_ENV: &str = "CODEXMANAGER_ROUTE_STRATEGY";
const ROUTE_MODE_ORDERED: u8 = 0;
const ROUTE_MODE_BALANCED_ROUND_ROBIN: u8 = 1;
const ROUTE_STRATEGY_ORDERED: &str = "ordered";
const ROUTE_STRATEGY_BALANCED: &str = "balanced";
const ROUTE_HEALTH_P2C_ENABLED_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED";
const ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW";
const ROUTE_HEALTH_P2C_BALANCED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW";
const DEFAULT_ROUTE_HEALTH_P2C_ENABLED: bool = true;
const DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW: usize = 3;
const DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW: usize = 6;

static ROUTE_MODE: AtomicU8 = AtomicU8::new(ROUTE_MODE_ORDERED);
static ROUTE_HEALTH_P2C_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_ROUTE_HEALTH_P2C_ENABLED);
static ROUTE_HEALTH_P2C_ORDERED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW);
static ROUTE_HEALTH_P2C_BALANCED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW);
static ROUTE_STATE: OnceLock<Mutex<RouteRoundRobinState>> = OnceLock::new();
static ROUTE_CONFIG_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Default)]
struct RouteRoundRobinState {
    next_start_by_key_model: HashMap<String, usize>,
    p2c_nonce_by_key_model: HashMap<String, u64>,
    manual_preferred_account_id: Option<String>,
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

    if rotate_to_manual_preferred_account(candidates) {
        return;
    }

    let mode = route_mode();
    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        let start = next_start_index(key_id, model, candidates.len());
        if start > 0 {
            candidates.rotate_left(start);
        }
    }

    apply_health_p2c(candidates, key_id, model, mode);
}

fn rotate_to_manual_preferred_account(candidates: &mut [(Account, Token)]) -> bool {
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let Ok(mut state) = lock.lock() else {
        return false;
    };
    let Some(account_id) = state.manual_preferred_account_id.as_deref() else {
        return false;
    };
    let Some(index) = candidates.iter().position(|(account, _)| account.id.eq(account_id)) else {
        // 中文注释：手动指定账号已不在可用候选池（可能用尽/不可用），自动回退到常规轮转。
        state.manual_preferred_account_id = None;
        return false;
    };
    if index > 0 {
        candidates.rotate_left(index);
    }
    true
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
            state.p2c_nonce_by_key_model.clear();
        }
    }
    Ok(route_mode_label(mode))
}

pub(crate) fn get_manual_preferred_account() -> Option<String> {
    ensure_route_config_loaded();
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    lock.lock()
        .ok()
        .and_then(|state| state.manual_preferred_account_id.clone())
}

pub(crate) fn set_manual_preferred_account(account_id: &str) -> Result<(), String> {
    ensure_route_config_loaded();
    let id = account_id.trim();
    if id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let Ok(mut state) = lock.lock() else {
        return Err("route state unavailable".to_string());
    };
    state.manual_preferred_account_id = Some(id.to_string());
    Ok(())
}

pub(crate) fn clear_manual_preferred_account() {
    ensure_route_config_loaded();
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    if let Ok(mut state) = lock.lock() {
        state.manual_preferred_account_id = None;
    }
}

pub(crate) fn clear_manual_preferred_account_if(account_id: &str) -> bool {
    ensure_route_config_loaded();
    let id = account_id.trim();
    if id.is_empty() {
        return false;
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let Ok(mut state) = lock.lock() else {
        return false;
    };
    if state
        .manual_preferred_account_id
        .as_deref()
        .is_some_and(|current| current == id)
    {
        state.manual_preferred_account_id = None;
        return true;
    }
    false
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

fn apply_health_p2c(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
    mode: u8,
) {
    if !route_health_p2c_enabled() {
        return;
    }
    let window = route_health_window(mode).min(candidates.len());
    if window <= 1 {
        return;
    }
    let Some(challenger_idx) = p2c_challenger_index(key_id, model, window) else {
        return;
    };
    let current_score = route_health_score(candidates[0].0.id.as_str());
    let challenger_score = route_health_score(candidates[challenger_idx].0.id.as_str());
    if challenger_score > current_score {
        // 中文注释：只交换头部候选，避免“整段 rotate”过度扰动既有顺序与轮询语义。
        candidates.swap(0, challenger_idx);
    }
}

fn p2c_challenger_index(
    key_id: &str,
    model: Option<&str>,
    candidate_count: usize,
) -> Option<usize> {
    if candidate_count < 2 {
        return None;
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let Ok(mut state) = lock.lock() else {
        return None;
    };
    let key = key_model_key(key_id, model);
    let nonce = state.p2c_nonce_by_key_model.entry(key.clone()).or_insert(0);
    let seed = stable_hash_u64(format!("{key}|{nonce}").as_bytes());
    *nonce = nonce.wrapping_add(1);
    // 中文注释：当前候选列表已有顺序（ordered / round-robin 后），P2C 只从前 window 内挑一个挑战者
    // 与“当前头部候选”对比，避免完全打乱轮询/排序语义。
    let offset = (seed as usize) % (candidate_count - 1);
    Some(offset + 1)
}

fn stable_hash_u64(input: &[u8]) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211_u64);
    }
    hash
}

fn route_health_p2c_enabled() -> bool {
    ROUTE_HEALTH_P2C_ENABLED.load(Ordering::Relaxed)
}

fn route_health_window(mode: u8) -> usize {
    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        ROUTE_HEALTH_P2C_BALANCED_WINDOW.load(Ordering::Relaxed)
    } else {
        ROUTE_HEALTH_P2C_ORDERED_WINDOW.load(Ordering::Relaxed)
    }
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
    ROUTE_HEALTH_P2C_ENABLED.store(
        env_bool_or(ROUTE_HEALTH_P2C_ENABLED_ENV, DEFAULT_ROUTE_HEALTH_P2C_ENABLED),
        Ordering::Relaxed,
    );
    ROUTE_HEALTH_P2C_ORDERED_WINDOW.store(
        env_usize_or(
            ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW,
        ),
        Ordering::Relaxed,
    );
    ROUTE_HEALTH_P2C_BALANCED_WINDOW.store(
        env_usize_or(
            ROUTE_HEALTH_P2C_BALANCED_WINDOW_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW,
        ),
        Ordering::Relaxed,
    );

    if let Some(lock) = ROUTE_STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.next_start_by_key_model.clear();
            state.p2c_nonce_by_key_model.clear();
            state.manual_preferred_account_id = None;
        }
    }
}

fn ensure_route_config_loaded() {
    let _ = ROUTE_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

fn env_bool_or(name: &str, default: bool) -> bool {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
fn clear_route_state_for_tests() {
    super::route_quality::clear_route_quality_for_tests();
    if let Some(lock) = ROUTE_STATE.get() {
        if let Ok(mut state) = lock.lock() {
            state.next_start_by_key_model.clear();
            state.p2c_nonce_by_key_model.clear();
            state.manual_preferred_account_id = None;
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

    #[test]
    fn health_p2c_promotes_healthier_candidate_in_ordered_mode() {
        let _guard = route_strategy_test_guard();
        super::super::route_quality::clear_route_quality_for_tests();
        std::env::set_var(ROUTE_HEALTH_P2C_ENABLED_ENV, "1");
        // 中文注释：窗口=2 时挑战者固定为 index=1，确保测试稳定可复现。
        std::env::set_var(ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV, "2");
        std::env::set_var(ROUTE_STRATEGY_ENV, "ordered");
        reload_from_env();
        clear_route_state_for_tests();

        for _ in 0..4 {
            super::super::route_quality::record_route_quality("acc-a", 429);
            super::super::route_quality::record_route_quality("acc-b", 200);
        }

        let mut candidates = candidate_list();
        apply_route_strategy(&mut candidates, "gk-health-1", Some("gpt-5.3-codex"));
        assert_eq!(account_ids(&candidates)[0], "acc-b");

        std::env::remove_var(ROUTE_HEALTH_P2C_ENABLED_ENV);
        std::env::remove_var(ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV);
        std::env::remove_var(ROUTE_STRATEGY_ENV);
        reload_from_env();
    }
}
