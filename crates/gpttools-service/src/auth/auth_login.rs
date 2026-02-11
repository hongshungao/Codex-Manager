use gpttools_core::auth::{
    build_authorize_url, device_redirect_uri, device_token_url, device_usercode_url,
    device_verification_url, generate_pkce, generate_state, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
    DEFAULT_ORIGINATOR,
};
use gpttools_core::rpc::types::{DeviceAuthInfo, LoginStartResult};
use gpttools_core::storage::{now_ts, Event, LoginSession};

use crate::auth_callback::{ensure_login_server, resolve_redirect_uri};
use crate::storage_helpers::open_storage;

pub(crate) fn login_start(
    login_type: &str,
    open_browser: bool,
    note: Option<String>,
    tags: Option<String>,
    group_name: Option<String>,
    workspace_id: Option<String>,
) -> Result<LoginStartResult, String> {
    // 读取登录相关配置
    let issuer = std::env::var("GPTTOOLS_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("GPTTOOLS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let originator =
        std::env::var("GPTTOOLS_ORIGINATOR").unwrap_or_else(|_| DEFAULT_ORIGINATOR.to_string());
    let mut warning = None;
    if login_type != "device" {
        if let Err(err) = ensure_login_server() {
            warning = Some(err);
        }
    }
    let redirect_uri = if login_type == "device" {
        std::env::var("GPTTOOLS_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:1455/auth/callback".to_string())
    } else {
        resolve_redirect_uri().unwrap_or_else(|| "http://localhost:1455/auth/callback".to_string())
    };

    // 生成 PKCE 与状态
    let pkce = generate_pkce();
    let state = generate_state();

    // 写入登录会话
    if let Some(storage) = open_storage() {
        let _ = storage.insert_login_session(&LoginSession {
            login_id: state.clone(),
            code_verifier: pkce.code_verifier.clone(),
            state: state.clone(),
            status: "pending".to_string(),
            error: None,
            note,
            tags,
            group_name,
            created_at: now_ts(),
            updated_at: now_ts(),
        });
    }

    // 构造登录地址
    let auth_url = if login_type == "device" {
        device_verification_url(&issuer)
    } else {
        build_authorize_url(
            &issuer,
            &client_id,
            &redirect_uri,
            &pkce.code_challenge,
            &state,
            &originator,
            workspace_id.as_deref(),
        )
    };

    // 设备登录信息
    let device = if login_type == "device" {
        Some(DeviceAuthInfo {
            user_code_url: device_usercode_url(&issuer),
            token_url: device_token_url(&issuer),
            verification_url: device_verification_url(&issuer),
            redirect_uri: device_redirect_uri(&issuer),
        })
    } else {
        None
    };

    // 写入事件日志
    if let Some(storage) = open_storage() {
        let _ = storage.insert_event(&Event {
            account_id: None,
            event_type: "login_start".to_string(),
            message: format!(
                "{{\"login_id\":\"{}\",\"code_verifier\":\"{}\"}}",
                state, pkce.code_verifier
            ),
            created_at: now_ts(),
        });
    }

    // 可选自动打开浏览器
    if login_type != "device" && open_browser {
        let _ = webbrowser::open(&auth_url);
    }

    Ok(LoginStartResult {
        auth_url,
        login_id: state,
        login_type: login_type.to_string(),
        issuer,
        client_id,
        redirect_uri,
        warning,
        device,
    })
}

pub(crate) fn login_status(login_id: &str) -> serde_json::Value {
    // 查询登录会话状态
    if login_id.is_empty() {
        return serde_json::json!({ "status": "unknown" });
    }
    let storage = match open_storage() {
        Some(storage) => storage,
        None => return serde_json::json!({ "status": "unknown" }),
    };
    let session = match storage.get_login_session(login_id) {
        Ok(Some(session)) => session,
        _ => return serde_json::json!({ "status": "unknown" }),
    };
    serde_json::json!({
        "status": session.status,
        "error": session.error,
        "updatedAt": session.updated_at
    })
}
