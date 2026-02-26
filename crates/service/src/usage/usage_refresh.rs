use codexmanager_core::auth::{extract_token_exp, DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use codexmanager_core::usage::parse_usage_snapshot;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{
    build_workspace_map_from_accounts, clean_header_value, derive_account_meta, patch_account_meta,
    patch_account_meta_cached, workspace_header_for_account,
};
use crate::usage_http::fetch_usage_snapshot;
use crate::usage_keepalive::{is_keepalive_error_ignorable, run_gateway_keepalive_once};
use crate::usage_scheduler::{
    parse_interval_secs, run_blocking_poll_loop,
    DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS,
    DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS, DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
    DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS, DEFAULT_USAGE_POLL_INTERVAL_SECS,
    DEFAULT_USAGE_POLL_JITTER_SECS, MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    MIN_USAGE_POLL_INTERVAL_SECS,
};
use crate::usage_snapshot_store::store_usage_snapshot;
use crate::usage_token_refresh::refresh_and_persist_access_token;

mod usage_refresh_errors;

static USAGE_POLLING_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static GATEWAY_KEEPALIVE_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static TOKEN_REFRESH_POLLING_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static PENDING_USAGE_REFRESH_TASKS: std::sync::OnceLock<Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();
static USAGE_REFRESH_EXECUTOR: std::sync::OnceLock<UsageRefreshExecutor> = std::sync::OnceLock::new();
const COMMON_POLL_JITTER_ENV: &str = "CODEXMANAGER_POLL_JITTER_SECS";
const COMMON_POLL_FAILURE_BACKOFF_MAX_ENV: &str = "CODEXMANAGER_POLL_FAILURE_BACKOFF_MAX_SECS";
const USAGE_POLL_JITTER_ENV: &str = "CODEXMANAGER_USAGE_POLL_JITTER_SECS";
const USAGE_POLL_FAILURE_BACKOFF_MAX_ENV: &str = "CODEXMANAGER_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS";
const USAGE_REFRESH_WORKERS_ENV: &str = "CODEXMANAGER_USAGE_REFRESH_WORKERS";
const DEFAULT_USAGE_REFRESH_WORKERS: usize = 4;
const GATEWAY_KEEPALIVE_JITTER_ENV: &str = "CODEXMANAGER_GATEWAY_KEEPALIVE_JITTER_SECS";
const GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV: &str =
    "CODEXMANAGER_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS";
const TOKEN_REFRESH_POLL_INTERVAL_SECS: u64 = 60;
const TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS: u64 = 300;
const TOKEN_REFRESH_AHEAD_SECS: i64 = 600;
const TOKEN_REFRESH_FALLBACK_AGE_SECS: i64 = 2700;
const TOKEN_REFRESH_BATCH_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageAvailabilityStatus {
    Available,
    PrimaryWindowAvailableOnly,
    Unavailable,
    Unknown,
}

impl UsageAvailabilityStatus {
    fn as_code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::PrimaryWindowAvailableOnly => "primary_window_available_only",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UsageRefreshResult {
    _status: UsageAvailabilityStatus,
}

use self::usage_refresh_errors::{
    mark_usage_unreachable_if_needed, record_usage_refresh_failure, should_retry_with_refresh,
};

pub(crate) fn ensure_usage_polling() {
    // 启动后台用量刷新线程（只启动一次）
    if std::env::var("CODEXMANAGER_DISABLE_POLLING").is_ok() {
        return;
    }
    USAGE_POLLING_STARTED.get_or_init(|| {
        let _ = thread::spawn(usage_polling_loop);
    });
}

pub(crate) fn ensure_gateway_keepalive() {
    GATEWAY_KEEPALIVE_STARTED.get_or_init(|| {
        let _ = thread::spawn(gateway_keepalive_loop);
    });
}

pub(crate) fn ensure_token_refresh_polling() {
    TOKEN_REFRESH_POLLING_STARTED.get_or_init(|| {
        let _ = thread::spawn(token_refresh_polling_loop);
    });
}

pub(crate) fn enqueue_usage_refresh_for_account(account_id: &str) -> bool {
    enqueue_usage_refresh_with_worker(account_id, |id| {
        if let Err(err) = refresh_usage_for_account(&id) {
            let status = classify_usage_status_from_error(&err);
            log::warn!(
                "async usage refresh failed: account_id={} status={} err={}",
                id,
                status.as_code(),
                err
            );
        }
    })
}

fn usage_polling_loop() {
    // 按间隔循环刷新所有账号用量
    let configured = std::env::var("CODEXMANAGER_USAGE_POLL_INTERVAL_SECS").ok();
    let interval_secs = parse_interval_secs(
        configured.as_deref(),
        DEFAULT_USAGE_POLL_INTERVAL_SECS,
        MIN_USAGE_POLL_INTERVAL_SECS,
    );
    let jitter_secs = parse_interval_with_fallback(
        USAGE_POLL_JITTER_ENV,
        COMMON_POLL_JITTER_ENV,
        DEFAULT_USAGE_POLL_JITTER_SECS,
        0,
    );
    let failure_backoff_cap_secs = parse_interval_with_fallback(
        USAGE_POLL_FAILURE_BACKOFF_MAX_ENV,
        COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
        DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS,
        interval_secs,
    );
    run_blocking_poll_loop(
        "usage polling",
        Duration::from_secs(interval_secs),
        Duration::from_secs(jitter_secs),
        Duration::from_secs(failure_backoff_cap_secs),
        refresh_usage_for_all_accounts,
        |_| true,
    );
}

fn gateway_keepalive_loop() {
    let configured = std::env::var("CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS").ok();
    let interval_secs = parse_interval_secs(
        configured.as_deref(),
        DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS,
        MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    );
    let jitter_secs = parse_interval_with_fallback(
        GATEWAY_KEEPALIVE_JITTER_ENV,
        COMMON_POLL_JITTER_ENV,
        DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
        0,
    );
    let failure_backoff_cap_secs = parse_interval_with_fallback(
        GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV,
        COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
        DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS,
        interval_secs,
    );
    run_blocking_poll_loop(
        "gateway keepalive",
        Duration::from_secs(interval_secs),
        Duration::from_secs(jitter_secs),
        Duration::from_secs(failure_backoff_cap_secs),
        run_gateway_keepalive_once,
        |err| !is_keepalive_error_ignorable(err),
    );
}

fn token_refresh_polling_loop() {
    run_blocking_poll_loop(
        "token refresh polling",
        Duration::from_secs(TOKEN_REFRESH_POLL_INTERVAL_SECS),
        Duration::ZERO,
        Duration::from_secs(TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS),
        refresh_tokens_before_expiry_for_all_accounts,
        |_| true,
    );
}

fn parse_interval_with_fallback(
    primary_env: &str,
    fallback_env: &str,
    default_secs: u64,
    min_secs: u64,
) -> u64 {
    let primary = std::env::var(primary_env).ok();
    let fallback = std::env::var(fallback_env).ok();
    let raw = primary.as_deref().or(fallback.as_deref());
    parse_interval_secs(raw, default_secs, min_secs)
}

fn enqueue_usage_refresh_with_worker<F>(account_id: &str, worker: F) -> bool
where
    F: FnOnce(String) + Send + 'static,
{
    let id = account_id.trim();
    if id.is_empty() {
        return false;
    }
    if !mark_usage_refresh_task_pending(id) {
        return false;
    }
    let task = UsageRefreshTask {
        account_id: id.to_string(),
        worker: Box::new(worker),
    };
    if usage_refresh_executor().sender.send(task).is_err() {
        clear_usage_refresh_task_pending(id);
        return false;
    }
    true
}

struct UsageRefreshTask {
    account_id: String,
    worker: Box<dyn FnOnce(String) + Send + 'static>,
}

struct UsageRefreshExecutor {
    sender: mpsc::Sender<UsageRefreshTask>,
}

impl UsageRefreshExecutor {
    fn new() -> Self {
        let worker_count = usage_refresh_worker_count();
        let (sender, receiver) = mpsc::channel::<UsageRefreshTask>();
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            let _ = thread::Builder::new()
                .name(format!("usage-refresh-worker-{index}"))
                .spawn(move || usage_refresh_worker_loop(receiver));
        }
        Self { sender }
    }
}

fn usage_refresh_executor() -> &'static UsageRefreshExecutor {
    USAGE_REFRESH_EXECUTOR.get_or_init(UsageRefreshExecutor::new)
}

fn usage_refresh_worker_loop(receiver: Arc<Mutex<mpsc::Receiver<UsageRefreshTask>>>) {
    loop {
        let task = {
            let receiver = receiver.lock().expect("usage refresh worker queue poisoned");
            receiver.recv()
        };
        let Ok(task) = task else {
            break;
        };
        let UsageRefreshTask { account_id, worker } = task;
        let account_id_for_clear = account_id.clone();
        worker(account_id);
        clear_usage_refresh_task_pending(&account_id_for_clear);
    }
}

fn usage_refresh_worker_count() -> usize {
    std::env::var(USAGE_REFRESH_WORKERS_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_USAGE_REFRESH_WORKERS)
}

fn mark_usage_refresh_task_pending(account_id: &str) -> bool {
    let mutex = PENDING_USAGE_REFRESH_TASKS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut pending = mutex.lock().expect("usage refresh task set poisoned");
    pending.insert(account_id.to_string())
}

fn clear_usage_refresh_task_pending(account_id: &str) {
    let Some(mutex) = PENDING_USAGE_REFRESH_TASKS.get() else {
        return;
    };
    let mut pending = mutex.lock().expect("usage refresh task set poisoned");
    pending.remove(account_id);
}

#[cfg(test)]
fn clear_pending_usage_refresh_tasks_for_tests() {
    if let Some(mutex) = PENDING_USAGE_REFRESH_TASKS.get() {
        let mut pending = mutex.lock().expect("usage refresh task set poisoned");
        pending.clear();
    }
}

pub(crate) fn refresh_usage_for_all_accounts() -> Result<(), String> {
    // 批量刷新所有账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Ok(());
    }

    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let workspace_map = build_workspace_map_from_accounts(&accounts);
    let mut account_map = account_map_from_list(accounts);

    for token in tokens {
        let workspace_id = workspace_map
            .get(&token.account_id)
            .and_then(|value| value.as_deref());
        let started_at = Instant::now();
        match refresh_usage_for_token(&storage, &token, workspace_id, Some(&mut account_map)) {
            Ok(result) => {
                record_usage_refresh_metrics(true, started_at);
                let _ = result;
            }
            Err(err) => {
                record_usage_refresh_metrics(false, started_at);
                record_usage_refresh_failure(&storage, &token.account_id, &err);
            }
        }
    }
    Ok(())
}

pub(crate) fn refresh_tokens_before_expiry_for_all_accounts() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let now = now_ts();
    let mut tokens = storage
        .list_tokens_due_for_refresh(now, TOKEN_REFRESH_BATCH_LIMIT)
        .map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Ok(());
    }

    let issuer = std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let mut refreshed = 0usize;
    let mut skipped = 0usize;

    for token in tokens.iter_mut() {
        let _ = storage.touch_token_refresh_attempt(&token.account_id, now);
        let (exp_opt, scheduled_at) = token_refresh_schedule(
            token,
            now,
            TOKEN_REFRESH_AHEAD_SECS,
            TOKEN_REFRESH_FALLBACK_AGE_SECS,
        );
        let _ = storage.update_token_refresh_schedule(
            &token.account_id,
            exp_opt,
            Some(scheduled_at),
        );
        if scheduled_at > now {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match refresh_and_persist_access_token(&storage, token, &issuer, &client_id) {
            Ok(_) => {
                refreshed = refreshed.saturating_add(1);
            }
            Err(err) => {
                log::warn!(
                    "token refresh polling failed: account_id={} err={}",
                    token.account_id,
                    err
                );
            }
        }
    }

    let _ = (refreshed, skipped);
    Ok(())
}

pub(crate) fn refresh_usage_for_account(account_id: &str) -> Result<(), String> {
    // 刷新单个账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let token = match storage
        .find_token_by_account_id(account_id)
        .map_err(|e| e.to_string())?
    {
        Some(token) => token,
        None => return Ok(()),
    };

    let account = storage
        .find_account_by_id(account_id)
        .map_err(|e| e.to_string())?;
    let workspace_id = account.as_ref().and_then(workspace_header_for_account);
    let mut account_map = account
        .map(|value| {
            let mut map = HashMap::new();
            map.insert(value.id.clone(), value);
            map
        })
        .unwrap_or_default();

    let started_at = Instant::now();
    let account_cache = if account_map.is_empty() {
        None
    } else {
        Some(&mut account_map)
    };
    match refresh_usage_for_token(&storage, &token, workspace_id.as_deref(), account_cache) {
        Ok(_) => {}
        Err(err) => {
            record_usage_refresh_metrics(false, started_at);
            record_usage_refresh_failure(&storage, &token.account_id, &err);
            return Err(err);
        }
    }
    record_usage_refresh_metrics(true, started_at);
    Ok(())
}

fn record_usage_refresh_metrics(success: bool, started_at: Instant) {
    crate::gateway::record_usage_refresh_outcome(
        success,
        crate::gateway::duration_to_millis(started_at.elapsed()),
    );
}

fn refresh_usage_for_token(
    storage: &Storage,
    token: &Token,
    workspace_id: Option<&str>,
    account_cache: Option<&mut HashMap<String, Account>>,
) -> Result<UsageRefreshResult, String> {
    // 读取用量接口所需的基础配置
    let issuer = std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let base_url = std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());

    let mut current = token.clone();
    let mut resolved_workspace_id = workspace_id.map(|v| v.to_string());
    let (derived_chatgpt_id, derived_workspace_id) =
        derive_account_meta(&current);

    if resolved_workspace_id.is_none() {
        resolved_workspace_id = derived_workspace_id
            .clone()
            .or_else(|| derived_chatgpt_id.clone());
    }

    if let Some(accounts) = account_cache {
        patch_account_meta_cached(
            storage,
            accounts,
            &current.account_id,
            derived_chatgpt_id,
            derived_workspace_id,
        );
    } else {
        patch_account_meta(
            storage,
            &current.account_id,
            derived_chatgpt_id,
            derived_workspace_id,
        );
    }

    let resolved_workspace_id = clean_header_value(resolved_workspace_id);
    let bearer = current.access_token.clone();

    match fetch_usage_snapshot(&base_url, &bearer, resolved_workspace_id.as_deref()) {
        Ok(value) => {
            let status = classify_usage_status_from_snapshot_value(&value);
            store_usage_snapshot(storage, &current.account_id, value)?;
            Ok(UsageRefreshResult { _status: status })
        }
        Err(err) if should_retry_with_refresh(&err) => {
            // 中文注释：token 刷新与持久化独立封装，避免轮询流程继续膨胀；
            // 不下沉会让后续 async 迁移时刷新链路与业务编排强耦合，回归范围扩大。
            let _ = refresh_and_persist_access_token(storage, &mut current, &issuer, &client_id)?;
            let bearer = current.access_token.clone();
            match fetch_usage_snapshot(&base_url, &bearer, resolved_workspace_id.as_deref()) {
                Ok(value) => {
                    let status = classify_usage_status_from_snapshot_value(&value);
                    store_usage_snapshot(storage, &current.account_id, value)?;
                    Ok(UsageRefreshResult { _status: status })
                }
                Err(err) => {
                    mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
                    Err(err)
                }
            }
        }
        Err(err) => {
            mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
            Err(err)
        }
    }
}

fn account_map_from_list(accounts: Vec<Account>) -> HashMap<String, Account> {
    let mut out = HashMap::with_capacity(accounts.len());
    for account in accounts {
        out.insert(account.id.clone(), account);
    }
    out
}

#[cfg(test)]
#[path = "../../tests/usage/usage_refresh_status_tests.rs"]
mod status_tests;

#[cfg(test)]
mod async_tests {
    use super::{
        clear_pending_usage_refresh_tasks_for_tests, enqueue_usage_refresh_with_worker,
    };
    use std::collections::HashSet;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;

    static USAGE_ASYNC_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn enqueue_usage_refresh_for_same_account_is_deduplicated_until_finish() {
        let _guard = USAGE_ASYNC_TEST_LOCK.lock().expect("lock");
        clear_pending_usage_refresh_tasks_for_tests();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first = enqueue_usage_refresh_with_worker("acc-dedup", move |_| {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        assert!(first);
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker started");

        let second = enqueue_usage_refresh_with_worker("acc-dedup", |_| {});
        assert!(!second);

        let _ = release_tx.send(());
        std::thread::sleep(Duration::from_millis(20));

        let third = enqueue_usage_refresh_with_worker("acc-dedup", |_| {});
        assert!(third);
        std::thread::sleep(Duration::from_millis(20));
        clear_pending_usage_refresh_tasks_for_tests();
    }

    #[test]
    fn enqueue_usage_refresh_for_different_accounts_keeps_queue_progress() {
        let _guard = USAGE_ASYNC_TEST_LOCK.lock().expect("lock");
        clear_pending_usage_refresh_tasks_for_tests();
        let (started_tx, started_rx) = mpsc::channel::<String>();
        let (release_tx, release_rx) = mpsc::channel();
        let started_tx_first = started_tx.clone();

        let first = enqueue_usage_refresh_with_worker("acc-a", move |_| {
            let _ = started_tx_first.send("acc-a".to_string());
            let _ = release_rx.recv_timeout(Duration::from_secs(1));
        });
        assert!(first);

        let started_tx = started_tx.clone();
        let second = enqueue_usage_refresh_with_worker("acc-b", move |_| {
            let _ = started_tx.send("acc-b".to_string());
        });
        assert!(second);

        let first_started = started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first task should start");
        let _ = release_tx.send(());
        let second_started = started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second task should start");

        let seen: HashSet<String> = [first_started, second_started].into_iter().collect();
        assert_eq!(seen.len(), 2);
        assert!(seen.contains("acc-a"));
        assert!(seen.contains("acc-b"));

        std::thread::sleep(Duration::from_millis(20));
        clear_pending_usage_refresh_tasks_for_tests();
    }
}

fn classify_usage_status_from_snapshot_value(value: &serde_json::Value) -> UsageAvailabilityStatus {
    let parsed = parse_usage_snapshot(value);

    let primary_present = parsed.used_percent.is_some() && parsed.window_minutes.is_some();
    if !primary_present {
        return UsageAvailabilityStatus::Unknown;
    }

    if parsed.used_percent.map(|v| v >= 100.0).unwrap_or(false) {
        return UsageAvailabilityStatus::Unavailable;
    }

    let secondary_used = parsed.secondary_used_percent;
    let secondary_window = parsed.secondary_window_minutes;
    let secondary_present = secondary_used.is_some() || secondary_window.is_some();
    let secondary_complete = secondary_used.is_some() && secondary_window.is_some();

    if !secondary_present {
        return UsageAvailabilityStatus::PrimaryWindowAvailableOnly;
    }
    if !secondary_complete {
        return UsageAvailabilityStatus::Unknown;
    }
    if secondary_used.map(|v| v >= 100.0).unwrap_or(false) {
        return UsageAvailabilityStatus::Unavailable;
    }
    UsageAvailabilityStatus::Available
}

fn classify_usage_status_from_error(err: &str) -> UsageAvailabilityStatus {
    if err.starts_with("usage endpoint status ") {
        return UsageAvailabilityStatus::Unavailable;
    }
    UsageAvailabilityStatus::Unknown
}

fn token_refresh_schedule(
    token: &Token,
    now_ts_secs: i64,
    ahead_secs: i64,
    fallback_age_secs: i64,
) -> (Option<i64>, i64) {
    if token.refresh_token.trim().is_empty() {
        return (None, i64::MAX);
    }
    if let Some(exp) = extract_token_exp(&token.access_token) {
        return (Some(exp), exp.saturating_sub(ahead_secs));
    }
    (
        None,
        token
            .last_refresh
            .saturating_add(fallback_age_secs)
            .max(now_ts_secs),
    )
}

#[cfg(test)]
mod proactive_token_tests {
    use super::token_refresh_schedule;
    use codexmanager_core::storage::{now_ts, Token};

    #[test]
    fn schedule_prefers_exp_minus_ahead() {
        let now = now_ts();
        let token = Token {
            account_id: "acc-1".to_string(),
            id_token: "id".to_string(),
            access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now - 10,
        };
        let (exp, scheduled_at) = token_refresh_schedule(&token, now, 600, 2700);
        assert_eq!(exp, Some(4_102_444_800));
        assert_eq!(scheduled_at, 4_102_444_200);
    }

    #[test]
    fn schedule_falls_back_to_last_refresh_when_exp_missing() {
        let now = now_ts();
        let token = Token {
            account_id: "acc-2".to_string(),
            id_token: "id".to_string(),
            access_token: "no-jwt".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now - 5000,
        };
        let (exp, scheduled_at) = token_refresh_schedule(&token, now, 300, 2700);
        assert_eq!(exp, None);
        assert_eq!(scheduled_at, now);
    }
}

