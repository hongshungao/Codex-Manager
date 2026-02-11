use gpttools_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use gpttools_core::storage::{now_ts, Event, Storage, Token};
use std::thread;
use std::time::Duration;

use crate::account_status::set_account_status;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{
    build_workspace_map, clean_header_value, derive_account_meta, patch_account_meta,
    resolve_workspace_id_for_account,
};
use crate::usage_http::fetch_usage_snapshot;
use crate::usage_keepalive::{is_keepalive_error_ignorable, run_gateway_keepalive_once};
use crate::usage_scheduler::{
    parse_interval_secs, run_blocking_poll_loop, DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    DEFAULT_USAGE_POLL_INTERVAL_SECS, MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    MIN_USAGE_POLL_INTERVAL_SECS,
};
use crate::usage_snapshot_store::store_usage_snapshot;
use crate::usage_token_refresh::refresh_and_persist_access_token;

static USAGE_POLLING_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static GATEWAY_KEEPALIVE_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

pub(crate) fn ensure_usage_polling() {
    // 启动后台用量刷新线程（只启动一次）
    if std::env::var("GPTTOOLS_DISABLE_POLLING").is_ok() {
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

fn usage_polling_loop() {
    // 按间隔循环刷新所有账号用量
    let configured = std::env::var("GPTTOOLS_USAGE_POLL_INTERVAL_SECS").ok();
    let interval_secs = parse_interval_secs(
        configured.as_deref(),
        DEFAULT_USAGE_POLL_INTERVAL_SECS,
        MIN_USAGE_POLL_INTERVAL_SECS,
    );
    run_blocking_poll_loop(
        "usage polling",
        Duration::from_secs(interval_secs),
        refresh_usage_for_all_accounts,
        |_| true,
    );
}

fn gateway_keepalive_loop() {
    let configured = std::env::var("GPTTOOLS_GATEWAY_KEEPALIVE_INTERVAL_SECS").ok();
    let interval_secs = parse_interval_secs(
        configured.as_deref(),
        DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS,
        MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    );
    run_blocking_poll_loop(
        "gateway keepalive",
        Duration::from_secs(interval_secs),
        run_gateway_keepalive_once,
        |err| !is_keepalive_error_ignorable(err),
    );
}

fn record_usage_refresh_failure(storage: &Storage, account_id: &str, message: &str) {
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "usage_refresh_failed".to_string(),
        message: message.to_string(),
        created_at: now_ts(),
    });
}

fn mark_usage_unreachable_if_needed(storage: &Storage, account_id: &str, err: &str) {
    // 中文注释：仅当上游明确返回 usage endpoint 状态错误才降级账号，
    // 否则网络抖动等瞬态错误也会误标 inactive，导致可用账号被过早摘除。
    if err.starts_with("usage endpoint status") {
        set_account_status(storage, account_id, "inactive", "usage_unreachable");
    }
}

fn should_retry_with_refresh(err: &str) -> bool {
    err.contains("401") || err.contains("403")
}

pub(crate) fn refresh_usage_for_all_accounts() -> Result<(), String> {
    // 批量刷新所有账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Ok(());
    }

    let workspace_map = build_workspace_map(&storage);

    for token in tokens {
        let workspace_id = workspace_map
            .get(&token.account_id)
            .and_then(|value| value.as_deref());
        if let Err(err) = refresh_usage_for_token(&storage, &token, workspace_id) {
            record_usage_refresh_failure(&storage, &token.account_id, &err);
        }
    }
    Ok(())
}

pub(crate) fn refresh_usage_for_account(account_id: &str) -> Result<(), String> {
    // 刷新单个账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    let token = match tokens.into_iter().find(|token| token.account_id == account_id) {
        Some(token) => token,
        None => return Ok(()),
    };

    let workspace_id = resolve_workspace_id_for_account(&storage, account_id);

    if let Err(err) = refresh_usage_for_token(&storage, &token, workspace_id.as_deref()) {
        record_usage_refresh_failure(&storage, &token.account_id, &err);
        return Err(err);
    }
    Ok(())
}

fn refresh_usage_for_token(
    storage: &Storage,
    token: &Token,
    workspace_id: Option<&str>,
) -> Result<(), String> {
    // 读取用量接口所需的基础配置
    let issuer = std::env::var("GPTTOOLS_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("GPTTOOLS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let base_url = std::env::var("GPTTOOLS_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());

    let mut current = token.clone();
    let mut resolved_workspace_id = workspace_id.map(|v| v.to_string());
    let (derived_chatgpt_id, derived_workspace_id, derived_workspace_name) =
        derive_account_meta(&current);

    if resolved_workspace_id.is_none() {
        resolved_workspace_id = derived_workspace_id
            .clone()
            .or_else(|| derived_chatgpt_id.clone());
    }

    patch_account_meta(
        storage,
        &current.account_id,
        derived_chatgpt_id,
        derived_workspace_id,
        derived_workspace_name,
    );

    let resolved_workspace_id = clean_header_value(resolved_workspace_id);
    let bearer = current.access_token.clone();

    match fetch_usage_snapshot(&base_url, &bearer, resolved_workspace_id.as_deref()) {
        Ok(value) => store_usage_snapshot(storage, &current.account_id, value),
        Err(err) if should_retry_with_refresh(&err) => {
            // 中文注释：token 刷新与持久化独立封装，避免轮询流程继续膨胀；
            // 不下沉会让后续 async 迁移时刷新链路与业务编排强耦合，回归范围扩大。
            refresh_and_persist_access_token(storage, &mut current, &issuer, &client_id)?;
            let bearer = current.access_token.clone();
            match fetch_usage_snapshot(&base_url, &bearer, resolved_workspace_id.as_deref()) {
                Ok(value) => store_usage_snapshot(storage, &current.account_id, value),
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

#[cfg(test)]
mod status_tests {
    use super::{mark_usage_unreachable_if_needed, should_retry_with_refresh};
    use crate::account_availability::Availability;
    use crate::usage_snapshot_store::apply_status_from_snapshot;
    use gpttools_core::storage::{now_ts, Account, Storage, UsageSnapshotRecord};

    #[test]
    fn apply_status_marks_inactive_on_missing() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let account = Account {
            id: "acc-1".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            workspace_name: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        storage.insert_account(&account).expect("insert");

        let record = UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: None,
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now_ts(),
        };

        let availability = apply_status_from_snapshot(&storage, &record);
        assert!(matches!(availability, Availability::Unavailable(_)));
        let loaded = storage
            .list_accounts()
            .expect("list")
            .into_iter()
            .find(|acc| acc.id == "acc-1")
            .expect("exists");
        assert_eq!(loaded.status, "inactive");
    }

    #[test]
    fn mark_usage_unreachable_only_for_usage_status_error() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let account = Account {
            id: "acc-2".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            workspace_name: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        storage.insert_account(&account).expect("insert");

        mark_usage_unreachable_if_needed(&storage, "acc-2", "network timeout");
        let still_active = storage
            .list_accounts()
            .expect("list")
            .into_iter()
            .find(|acc| acc.id == "acc-2")
            .expect("exists");
        assert_eq!(still_active.status, "active");

        mark_usage_unreachable_if_needed(
            &storage,
            "acc-2",
            "usage endpoint status 500 Internal Server Error",
        );
        let inactive = storage
            .list_accounts()
            .expect("list")
            .into_iter()
            .find(|acc| acc.id == "acc-2")
            .expect("exists");
        assert_eq!(inactive.status, "inactive");
    }

    #[test]
    fn refresh_retry_filter_matches_auth_failures() {
        assert!(should_retry_with_refresh("usage endpoint status 401"));
        assert!(should_retry_with_refresh("usage endpoint status 403"));
        assert!(!should_retry_with_refresh("usage endpoint status 429"));
    }
}
