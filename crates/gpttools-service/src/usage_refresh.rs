use gpttools_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, extract_workspace_name,
    parse_id_token_claims, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use gpttools_core::storage::{now_ts, Event, Storage, Token, UsageSnapshotRecord};
use gpttools_core::usage::{parse_usage_snapshot, usage_endpoint};
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_status::set_account_status;
use crate::auth_tokens::obtain_api_key;
use crate::storage_helpers::open_storage;

static USAGE_POLLING_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static GATEWAY_KEEPALIVE_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
const DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS: u64 = 180;

pub(crate) fn ensure_usage_polling() {
    // 启动后台用量刷新线程（只启动一次）
    if std::env::var("GPTTOOLS_DISABLE_POLLING").is_ok() {
        return;
    }
    USAGE_POLLING_STARTED.get_or_init(|| {
        let _ = thread::spawn(|| usage_polling_loop());
    });
}

pub(crate) fn ensure_gateway_keepalive() {
    GATEWAY_KEEPALIVE_STARTED.get_or_init(|| {
        let _ = thread::spawn(|| gateway_keepalive_loop());
    });
}

fn usage_polling_loop() {
    // 按间隔循环刷新所有账号用量
    let interval_secs = std::env::var("GPTTOOLS_USAGE_POLL_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    loop {
        if let Err(err) = refresh_usage_for_all_accounts() {
            log::warn!("usage polling error: {err}");
        }
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

fn gateway_keepalive_loop() {
    loop {
        if let Err(err) = run_gateway_keepalive_once() {
            if !is_keepalive_error_ignorable(err.as_str()) {
                log::warn!("gateway keepalive error: {err}");
            }
        }
        thread::sleep(Duration::from_secs(DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS));
    }
}

fn run_gateway_keepalive_once() -> Result<(), String> {
    // 中文注释：定期探活 models 路径可预热上游连接与 token exchange，减少服务空闲后首个请求的冷启动失败概率。
    let _ = crate::gateway::fetch_models_for_picker()?;
    Ok(())
}

fn is_keepalive_error_ignorable(err: &str) -> bool {
    let normalized = err.trim().to_ascii_lowercase();
    normalized.contains("no available account") || normalized.contains("storage unavailable")
}

pub(crate) fn refresh_usage_for_all_accounts() -> Result<(), String> {
    // 批量刷新所有账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Ok(());
    }
    let mut workspace_map = HashMap::new();
    if let Ok(accounts) = storage.list_accounts() {
        for account in accounts {
            let header_id = clean_header_value(account.workspace_id)
                .or_else(|| clean_header_value(account.chatgpt_account_id));
            workspace_map.insert(account.id, header_id);
        }
    }
    for token in tokens {
        let workspace_id = workspace_map
            .get(&token.account_id)
            .and_then(|v| v.as_deref());
        if let Err(err) = refresh_usage_for_token(&storage, &token, workspace_id) {
            let _ = storage.insert_event(&Event {
                account_id: Some(token.account_id.clone()),
                event_type: "usage_refresh_failed".to_string(),
                message: err.clone(),
                created_at: now_ts(),
            });
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
    let workspace_id = storage
        .list_accounts()
        .ok()
        .and_then(|accounts| {
            accounts
                .into_iter()
                .find(|account| account.id == account_id)
                .and_then(|account| {
                    clean_header_value(account.workspace_id)
                        .or_else(|| clean_header_value(account.chatgpt_account_id))
                })
        });
    let workspace_id = workspace_id.as_deref();
    if let Err(err) = refresh_usage_for_token(&storage, &token, workspace_id) {
        let _ = storage.insert_event(&Event {
            account_id: Some(token.account_id.clone()),
            event_type: "usage_refresh_failed".to_string(),
            message: err.clone(),
            created_at: now_ts(),
        });
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
    let mut bearer = current.access_token.clone();

    match fetch_usage_snapshot(&base_url, &bearer, resolved_workspace_id.as_deref()) {
        Ok(value) => {
            return store_usage_snapshot(storage, &current.account_id, value);
        }
        Err(err) if err.contains("401") || err.contains("403") => {
            let refreshed = refresh_access_token(&issuer, &client_id, &current.refresh_token)?;
            current.access_token = refreshed.access_token;
            if let Some(refresh_token) = refreshed.refresh_token {
                current.refresh_token = refresh_token;
            }
            if let Some(id_token) = refreshed.id_token {
                current.id_token = id_token.clone();
                if let Ok(api_key) = obtain_api_key(&issuer, &client_id, &id_token) {
                    current.api_key_access_token = Some(api_key);
                }
            }
            current.last_refresh = now_ts();
            storage.insert_token(&current).map_err(|e| e.to_string())?;
            bearer = current.access_token.clone();
            let value = match fetch_usage_snapshot(
                &base_url,
                &bearer,
                resolved_workspace_id.as_deref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    if err.starts_with("usage endpoint status") {
                        set_account_status(storage, &current.account_id, "inactive", "usage_unreachable");
                    }
                    return Err(err);
                }
            };
            return store_usage_snapshot(storage, &current.account_id, value);
        }
        Err(err) => {
            if err.starts_with("usage endpoint status") {
                set_account_status(storage, &current.account_id, "inactive", "usage_unreachable");
            }
            return Err(err);
        }
    }
}

fn clean_header_value(value: Option<String>) -> Option<String> {
    match value {
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => None,
    }
}

fn derive_account_meta(token: &Token) -> (Option<String>, Option<String>, Option<String>) {
    let mut chatgpt_account_id = None;
    let mut workspace_id = None;
    let mut workspace_name = None;

    if let Ok(claims) = parse_id_token_claims(&token.id_token) {
        if let Some(auth) = claims.auth {
            if chatgpt_account_id.is_none() {
                chatgpt_account_id = clean_header_value(auth.chatgpt_account_id);
            }
        }
        if workspace_id.is_none() {
            workspace_id = clean_header_value(claims.workspace_id);
        }
    }

    if workspace_id.is_none() {
        workspace_id = clean_header_value(
            extract_workspace_id(&token.id_token)
                .or_else(|| extract_workspace_id(&token.access_token)),
        );
    }
    if chatgpt_account_id.is_none() {
        chatgpt_account_id = clean_header_value(
            extract_chatgpt_account_id(&token.id_token)
                .or_else(|| extract_chatgpt_account_id(&token.access_token)),
        );
    }
    if workspace_id.is_none() {
        workspace_id = chatgpt_account_id.clone();
    }
    if workspace_name.is_none() {
        workspace_name = clean_header_value(
            extract_workspace_name(&token.id_token)
                .or_else(|| extract_workspace_name(&token.access_token)),
        );
    }

    (chatgpt_account_id, workspace_id, workspace_name)
}

fn patch_account_meta(
    storage: &Storage,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
) {
    let Ok(accounts) = storage.list_accounts() else { return };
    let Some(mut account) = accounts.into_iter().find(|acc| acc.id == account_id) else { return };

    let mut changed = false;
    if account.chatgpt_account_id.as_deref().unwrap_or("").trim().is_empty()
        && chatgpt_account_id.is_some()
    {
        account.chatgpt_account_id = chatgpt_account_id;
        changed = true;
    }
    if account.workspace_id.as_deref().unwrap_or("").trim().is_empty() && workspace_id.is_some() {
        account.workspace_id = workspace_id;
        changed = true;
    }
    if account.workspace_name.as_deref().unwrap_or("").trim().is_empty()
        && workspace_name.is_some()
    {
        account.workspace_name = workspace_name;
        changed = true;
    }

    if changed {
        account.updated_at = now_ts();
        let _ = storage.insert_account(&account);
    }
}

fn apply_status_from_snapshot(storage: &Storage, record: &UsageSnapshotRecord) -> Availability {
    let availability = evaluate_snapshot(record);
    match availability {
        Availability::Available => {
            set_account_status(storage, &record.account_id, "active", "usage_ok");
        }
        Availability::Unavailable(reason) => {
            set_account_status(storage, &record.account_id, "inactive", reason);
        }
    }
    availability
}

fn store_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    // 解析并写入用量快照
    let parsed = parse_usage_snapshot(&value);
    let record = UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: parsed.used_percent,
        window_minutes: parsed.window_minutes,
        resets_at: parsed.resets_at,
        secondary_used_percent: parsed.secondary_used_percent,
        secondary_window_minutes: parsed.secondary_window_minutes,
        secondary_resets_at: parsed.secondary_resets_at,
        credits_json: parsed.credits_json,
        captured_at: now_ts(),
    };
    storage
        .insert_usage_snapshot(&record)
        .map_err(|e| e.to_string())?;
    let _ = apply_status_from_snapshot(storage, &record);
    Ok(())
}

fn fetch_usage_snapshot(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    // 调用上游用量接口
    let url = usage_endpoint(base_url);
    let client = Client::new();
    let mut req = client
        .get(&url)
        .header("Authorization", format!("Bearer {bearer}"));
    if let Some(workspace_id) = workspace_id {
        req = req.header("ChatGPT-Account-Id", workspace_id);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("usage endpoint status {}", resp.status()));
    }
    resp.json().map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

fn refresh_access_token(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    // 使用 refresh_token 获取新的 access_token
    let client = Client::new();
    let resp = client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&scope=openid%20profile%20email",
            urlencoding::encode(refresh_token),
            urlencoding::encode(client_id)
        ))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "refresh token failed with status {}",
            resp.status()
        ));
    }
    resp.json().map_err(|e| e.to_string())
}

#[cfg(test)]
mod status_tests {
    use super::{apply_status_from_snapshot, is_keepalive_error_ignorable};
    use crate::account_availability::Availability;
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
    fn keepalive_ignores_expected_idle_errors() {
        assert!(is_keepalive_error_ignorable("no available account"));
        assert!(is_keepalive_error_ignorable("storage unavailable"));
        assert!(!is_keepalive_error_ignorable("upstream timeout"));
    }
}
