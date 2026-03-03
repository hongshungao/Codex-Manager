use super::route_quality::route_health_score;
use codexmanager_core::storage::{Account, Token};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROUTE_STRATEGY_ENV: &str = "CODEXMANAGER_ROUTE_STRATEGY";
const ROUTE_MODE_ORDERED: u8 = 0;
const ROUTE_MODE_BALANCED_ROUND_ROBIN: u8 = 1;
const ROUTE_STRATEGY_ORDERED: &str = "ordered";
const ROUTE_STRATEGY_BALANCED: &str = "balanced";
const ROUTE_HEALTH_P2C_ENABLED_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED";
const ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW";
const ROUTE_HEALTH_P2C_BALANCED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW";
const ROUTE_STATE_TTL_SECS_ENV: &str = "CODEXMANAGER_ROUTE_STATE_TTL_SECS";
const ROUTE_STATE_CAPACITY_ENV: &str = "CODEXMANAGER_ROUTE_STATE_CAPACITY";
const DEFAULT_ROUTE_HEALTH_P2C_ENABLED: bool = true;
const DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW: usize = 3;
const DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW: usize = 6;
// 中文注释：Route 状态（按 key_id + model 维度）用于 round-robin 起点与 P2C nonce。
// 为避免 key/model 高基数导致 HashMap 无限增长，默认增加 TTL + 容量上限；不会影响“短时间内连续请求”的既有语义。
const DEFAULT_ROUTE_STATE_TTL_SECS: u64 = 6 * 60 * 60;
const DEFAULT_ROUTE_STATE_CAPACITY: usize = 4096;
const ROUTE_STATE_MAINTENANCE_EVERY: u64 = 64;

static ROUTE_MODE: AtomicU8 = AtomicU8::new(ROUTE_MODE_ORDERED);
static ROUTE_HEALTH_P2C_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_ROUTE_HEALTH_P2C_ENABLED);
static ROUTE_HEALTH_P2C_ORDERED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW);
static ROUTE_HEALTH_P2C_BALANCED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW);
static ROUTE_STATE_TTL_SECS: AtomicU64 = AtomicU64::new(DEFAULT_ROUTE_STATE_TTL_SECS);
static ROUTE_STATE_CAPACITY: AtomicUsize = AtomicUsize::new(DEFAULT_ROUTE_STATE_CAPACITY);
static ROUTE_STATE: OnceLock<Mutex<RouteRoundRobinState>> = OnceLock::new();
static ROUTE_CONFIG_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Clone, Copy)]
struct RouteStateEntry<T: Copy> {
    value: T,
    last_seen: Instant,
}

impl<T: Copy> RouteStateEntry<T> {
    fn new(value: T, last_seen: Instant) -> Self {
        Self { value, last_seen }
    }
}

#[derive(Default)]
struct RouteRoundRobinState {
    next_start_by_key_model: HashMap<String, RouteStateEntry<usize>>,
    p2c_nonce_by_key_model: HashMap<String, RouteStateEntry<u64>>,
    manual_preferred_account_id: Option<String>,
    maintenance_tick: u64,
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
    let mut state = crate::lock_utils::lock_recover(lock, "route_state");
    let Some(account_id) = state.manual_preferred_account_id.as_deref() else {
        return false;
    };
    let Some(index) = candidates
        .iter()
        .position(|(account, _)| account.id.eq(account_id))
    else {
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
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.maintenance_tick = 0;
    }
    Ok(route_mode_label(mode))
}

pub(crate) fn get_manual_preferred_account() -> Option<String> {
    ensure_route_config_loaded();
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let state = crate::lock_utils::lock_recover(lock, "route_state");
    state.manual_preferred_account_id.clone()
}

pub(crate) fn set_manual_preferred_account(account_id: &str) -> Result<(), String> {
    ensure_route_config_loaded();
    let id = account_id.trim();
    if id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "route_state");
    state.manual_preferred_account_id = Some(id.to_string());
    Ok(())
}

pub(crate) fn clear_manual_preferred_account() {
    ensure_route_config_loaded();
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "route_state");
    state.manual_preferred_account_id = None;
}

pub(crate) fn clear_manual_preferred_account_if(account_id: &str) -> bool {
    ensure_route_config_loaded();
    let id = account_id.trim();
    if id.is_empty() {
        return false;
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "route_state");
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
    let mut state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &mut *state_guard;
    let now = Instant::now();
    state.maybe_maintain(now);

    let ttl = route_state_ttl();
    let capacity = route_state_capacity();
    let key = key_model_key(key_id, model);
    remove_entry_if_expired(&mut state.next_start_by_key_model, key.as_str(), now, ttl);
    let start = {
        let entry = state
            .next_start_by_key_model
            .entry(key.clone())
            .or_insert(RouteStateEntry::new(0, now));
        entry.last_seen = now;
        let start = entry.value % candidate_count;
        entry.value = (start + 1) % candidate_count;
        start
    };
    enforce_capacity_pair(
        &mut state.next_start_by_key_model,
        &mut state.p2c_nonce_by_key_model,
        capacity,
    );
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
    let mut state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &mut *state_guard;
    let now = Instant::now();
    state.maybe_maintain(now);

    let ttl = route_state_ttl();
    let capacity = route_state_capacity();
    let key = key_model_key(key_id, model);
    remove_entry_if_expired(&mut state.p2c_nonce_by_key_model, key.as_str(), now, ttl);
    let nonce = {
        let entry = state
            .p2c_nonce_by_key_model
            .entry(key.clone())
            .or_insert(RouteStateEntry::new(0, now));
        entry.last_seen = now;
        let nonce = entry.value;
        entry.value = nonce.wrapping_add(1);
        nonce
    };
    enforce_capacity_pair(
        &mut state.p2c_nonce_by_key_model,
        &mut state.next_start_by_key_model,
        capacity,
    );
    let seed = stable_hash_u64(format!("{key}|{nonce}").as_bytes());
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

fn route_state_ttl() -> Duration {
    Duration::from_secs(ROUTE_STATE_TTL_SECS.load(Ordering::Relaxed))
}

fn route_state_capacity() -> usize {
    ROUTE_STATE_CAPACITY.load(Ordering::Relaxed)
}

fn is_entry_expired(last_seen: Instant, now: Instant, ttl: Duration) -> bool {
    if ttl.is_zero() {
        return false;
    }
    now.checked_duration_since(last_seen)
        .is_some_and(|age| age > ttl)
}

fn remove_entry_if_expired<T: Copy>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    key: &str,
    now: Instant,
    ttl: Duration,
) {
    if ttl.is_zero() {
        return;
    }
    let expired = map
        .get(key)
        .is_some_and(|entry| is_entry_expired(entry.last_seen, now, ttl));
    if expired {
        map.remove(key);
    }
}

fn prune_expired_entries<T: Copy>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    now: Instant,
    ttl: Duration,
) {
    if ttl.is_zero() {
        return;
    }
    map.retain(|_, entry| !is_entry_expired(entry.last_seen, now, ttl));
}

fn enforce_capacity_pair<T: Copy, U: Copy>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    other: &mut HashMap<String, RouteStateEntry<U>>,
    capacity: usize,
) {
    if capacity == 0 {
        return;
    }
    while map.len() > capacity {
        let Some(oldest_key) = find_oldest_key(map) else {
            break;
        };
        map.remove(oldest_key.as_str());
        other.remove(oldest_key.as_str());
    }
}

fn find_oldest_key<T: Copy>(map: &HashMap<String, RouteStateEntry<T>>) -> Option<String> {
    map.iter()
        .min_by(|(ka, ea), (kb, eb)| ea.last_seen.cmp(&eb.last_seen).then_with(|| ka.cmp(kb)))
        .map(|(key, _)| key.clone())
}

fn key_model_key(key_id: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}",
        key_id.trim(),
        model
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("-")
    )
}

pub(super) fn reload_from_env() {
    let raw = std::env::var(ROUTE_STRATEGY_ENV).unwrap_or_default();
    let mode = parse_route_mode(raw.as_str()).unwrap_or(ROUTE_MODE_ORDERED);
    ROUTE_MODE.store(mode, Ordering::Relaxed);
    ROUTE_HEALTH_P2C_ENABLED.store(
        env_bool_or(
            ROUTE_HEALTH_P2C_ENABLED_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_ENABLED,
        ),
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
    ROUTE_STATE_TTL_SECS.store(
        env_u64_or(ROUTE_STATE_TTL_SECS_ENV, DEFAULT_ROUTE_STATE_TTL_SECS),
        Ordering::Relaxed,
    );
    ROUTE_STATE_CAPACITY.store(
        env_usize_or(ROUTE_STATE_CAPACITY_ENV, DEFAULT_ROUTE_STATE_CAPACITY),
        Ordering::Relaxed,
    );

    if let Some(lock) = ROUTE_STATE.get() {
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.manual_preferred_account_id = None;
        state.maintenance_tick = 0;
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

fn env_u64_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

impl RouteRoundRobinState {
    fn maybe_maintain(&mut self, now: Instant) {
        self.maintenance_tick = self.maintenance_tick.wrapping_add(1);
        if self.maintenance_tick % ROUTE_STATE_MAINTENANCE_EVERY != 0 {
            return;
        }
        let ttl = route_state_ttl();
        let capacity = route_state_capacity();
        prune_expired_entries(&mut self.next_start_by_key_model, now, ttl);
        prune_expired_entries(&mut self.p2c_nonce_by_key_model, now, ttl);
        enforce_capacity_pair(
            &mut self.next_start_by_key_model,
            &mut self.p2c_nonce_by_key_model,
            capacity,
        );
        enforce_capacity_pair(
            &mut self.p2c_nonce_by_key_model,
            &mut self.next_start_by_key_model,
            capacity,
        );
    }
}

#[cfg(test)]
fn clear_route_state_for_tests() {
    super::route_quality::clear_route_quality_for_tests();
    if let Some(lock) = ROUTE_STATE.get() {
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.manual_preferred_account_id = None;
        state.maintenance_tick = 0;
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
#[path = "tests/route_hint_tests.rs"]
mod tests;
