use gpttools_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use gpttools_core::storage::{Storage, Token};
use std::thread;
use std::time::Duration;

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

mod usage_refresh_errors;

static USAGE_POLLING_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static GATEWAY_KEEPALIVE_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

use self::usage_refresh_errors::{
    mark_usage_unreachable_if_needed, record_usage_refresh_failure, should_retry_with_refresh,
};

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
#[path = "../../tests/usage/usage_refresh_status_tests.rs"]
mod status_tests;






