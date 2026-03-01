use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, parse_id_token_claims,
    DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use codexmanager_core::storage::{now_ts, Account, Token};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::sync::mpsc;
use std::time::Duration;

use crate::auth_callback::resolve_redirect_uri;
use crate::storage_helpers::{account_key, open_storage};

static OPENAI_AUTH_HTTP_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
const OPENAI_AUTH_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const OPENAI_AUTH_READ_TIMEOUT: Duration = Duration::from_secs(30);
const OPENAI_AUTH_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

fn read_json_with_timeout<T>(
    resp: reqwest::blocking::Response,
    read_timeout: Duration,
) -> Result<T, String>
where
    T: DeserializeOwned + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(resp.json::<T>().map_err(|e| e.to_string()));
    });
    match rx.recv_timeout(read_timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "response read timed out after {}ms",
            read_timeout.as_millis()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("response read failed: worker disconnected".to_string())
        }
    }
}

fn read_text_with_timeout(
    resp: reqwest::blocking::Response,
    read_timeout: Duration,
) -> Result<String, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(resp.text().map_err(|e| e.to_string()));
    });
    match rx.recv_timeout(read_timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "response read timed out after {}ms",
            read_timeout.as_millis()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("response read failed: worker disconnected".to_string())
        }
    }
}

fn clean_value(value: Option<String>) -> Option<String> {
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

fn normalize_id_part(value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.replace("::", "_"))
}

fn build_account_storage_id(
    subject_account_id: &str,
    identity_hint: Option<&str>,
    tags: Option<&str>,
) -> String {
    let base = subject_account_id.trim();
    let mut suffix_parts: Vec<String> = Vec::new();
    let normalized_hint = normalize_id_part(identity_hint);
    if let Some(hint) = normalized_hint {
        if hint != base {
            suffix_parts.push(hint);
        }
    }
    if let Some(tag) = normalize_id_part(tags) {
        suffix_parts.push(tag);
    }
    if suffix_parts.is_empty() {
        return base.to_string();
    }
    format!("{base}::{}", suffix_parts.join("|"))
}

fn pick_existing_account_id_by_identity(
    storage: &codexmanager_core::storage::Storage,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
    fallback_subject_key: &str,
) -> Option<String> {
    let preferred_chatgpt = chatgpt_account_id
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let preferred_workspace = workspace_id
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    let accounts = storage.list_accounts().ok()?;
    if let Some(chatgpt_id) = preferred_chatgpt.as_ref() {
        if let Some(found) = accounts.iter().find(|acc| {
            acc.chatgpt_account_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                == Some(chatgpt_id.as_str())
        }) {
            return Some(found.id.clone());
        }
        return None;
    }
    if let Some(workspace) = preferred_workspace.as_ref() {
        if let Some(found) = accounts.iter().find(|acc| {
            acc.workspace_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                == Some(workspace.as_str())
        }) {
            return Some(found.id.clone());
        }
        return None;
    }
    if accounts.iter().any(|acc| acc.id == fallback_subject_key) {
        return Some(fallback_subject_key.to_string());
    }
    None
}

fn openai_auth_http_client() -> &'static Client {
    OPENAI_AUTH_HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(OPENAI_AUTH_CONNECT_TIMEOUT)
            .timeout(OPENAI_AUTH_TOTAL_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

pub(crate) fn complete_login(state: &str, code: &str) -> Result<(), String> {
    complete_login_with_redirect(state, code, None)
}

pub(crate) fn complete_login_with_redirect(
    state: &str,
    code: &str,
    redirect_uri: Option<&str>,
) -> Result<(), String> {
    // 读取登录会话
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let session = storage
        .get_login_session(state)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "unknown login session".to_string())?;

    // 读取 OAuth 配置
    let issuer = std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let redirect_uri = redirect_uri
        .map(|value| value.to_string())
        .or_else(|| resolve_redirect_uri())
        .unwrap_or_else(|| "http://localhost:1455/auth/callback".to_string());

    // 交换授权码获取 token
    let tokens = exchange_code_for_tokens(
        &issuer,
        &client_id,
        &redirect_uri,
        &session.code_verifier,
        code,
    )
    .map_err(|e| {
        let _ = storage.update_login_session_status(state, "failed", Some(&e));
        e
    })?;

    // 可选兑换平台 key
    let api_key_access_token = obtain_api_key(&issuer, &client_id, &tokens.id_token).ok();
    let claims = parse_id_token_claims(&tokens.id_token).map_err(|e| {
        let _ = storage.update_login_session_status(state, "failed", Some(&e));
        e
    })?;

    // 生成账户记录
    let subject_account_id = claims.sub.clone();
    let label = claims.email.clone().unwrap_or_else(|| subject_account_id.clone());
    let chatgpt_account_id = clean_value(
        claims
            .auth
            .as_ref()
            .and_then(|auth| auth.chatgpt_account_id.clone())
            .or_else(|| extract_chatgpt_account_id(&tokens.id_token))
            .or_else(|| extract_chatgpt_account_id(&tokens.access_token)),
    );
    let workspace_id = clean_value(
        claims
            .workspace_id
            .clone()
            .or_else(|| extract_workspace_id(&tokens.id_token))
            .or_else(|| extract_workspace_id(&tokens.access_token))
            .or_else(|| chatgpt_account_id.clone()),
    );
    let fallback_subject_key = account_key(&subject_account_id, session.tags.as_deref());
    let account_storage_id = build_account_storage_id(
        &subject_account_id,
        chatgpt_account_id
            .as_deref()
            .or(workspace_id.as_deref()),
        session.tags.as_deref(),
    );
    let account_key = pick_existing_account_id_by_identity(
        &storage,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        &fallback_subject_key,
    )
    .unwrap_or(account_storage_id);
    let account = Account {
        id: account_key.clone(),
        label,
        issuer: issuer.clone(),
        chatgpt_account_id,
        workspace_id,
        group_name: session.group_name.clone(),
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).map_err(|e| e.to_string())?;

    // 写入 token
    let token = Token {
        account_id: account_key.clone(),
        id_token: tokens.id_token,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        api_key_access_token,
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).map_err(|e| e.to_string())?;

    storage
        .update_login_session_status(state, "success", None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

fn exchange_code_for_tokens(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<TokenResponse, String> {
    // 请求 token 接口
    let client = openai_auth_http_client();
    let resp = client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoding::encode(code),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(client_id),
            urlencoding::encode(code_verifier)
        ))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("token endpoint returned status {}", resp.status()));
    }
    read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT)
}

pub(crate) fn obtain_api_key(issuer: &str, client_id: &str, id_token: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct ExchangeResp {
        access_token: String,
    }

    // 兑换平台 API Key
    let client = openai_auth_http_client();
    let resp = client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
            urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
            urlencoding::encode(client_id),
            urlencoding::encode("openai-api-key"),
            urlencoding::encode(id_token),
            urlencoding::encode("urn:ietf:params:oauth:token-type:id_token")
        ))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = read_text_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT).unwrap_or_default();
        return Err(format!(
            "api key exchange failed with status {} body {}",
            status, body
        ));
    }
    let body: ExchangeResp = read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT)?;
    Ok(body.access_token)
}

