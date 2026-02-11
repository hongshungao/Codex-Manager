use gpttools_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, extract_workspace_name, parse_id_token_claims,
    DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use gpttools_core::storage::{now_ts, Account, Token};
use reqwest::blocking::Client;

use crate::auth_callback::resolve_redirect_uri;
use crate::storage_helpers::{account_key, open_storage};

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
    let issuer = std::env::var("GPTTOOLS_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("GPTTOOLS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
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
    let account_id = claims.sub.clone();
    let label = claims.email.clone().unwrap_or_else(|| account_id.clone());
    let chatgpt_account_id = clean_value(
        claims
            .auth
            .as_ref()
            .and_then(|auth| auth.chatgpt_account_id.clone())
            .or_else(|| extract_chatgpt_account_id(&tokens.id_token))
            .or_else(|| extract_chatgpt_account_id(&tokens.access_token))
            .or_else(|| Some(account_id.clone())),
    );
    let workspace_id = clean_value(
        claims
            .workspace_id
            .clone()
            .or_else(|| extract_workspace_id(&tokens.id_token))
            .or_else(|| extract_workspace_id(&tokens.access_token))
            .or_else(|| chatgpt_account_id.clone()),
    );
    let workspace_name = clean_value(
        extract_workspace_name(&tokens.id_token)
            .or_else(|| extract_workspace_name(&tokens.access_token)),
    );
    let account_key = account_key(&account_id, session.tags.as_deref());
    let account = Account {
        id: account_key.clone(),
        label,
        issuer: issuer.clone(),
        chatgpt_account_id,
        workspace_id,
        workspace_name,
        note: session.note.clone(),
        tags: session.tags.clone(),
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
    let client = Client::new();
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
    resp.json().map_err(|e| e.to_string())
}

pub(crate) fn obtain_api_key(issuer: &str, client_id: &str, id_token: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct ExchangeResp {
        access_token: String,
    }

    // 兑换平台 API Key
    let client = Client::new();
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
        let body = resp.text().unwrap_or_default();
        return Err(format!(
            "api key exchange failed with status {} body {}",
            status, body
        ));
    }
    let body: ExchangeResp = resp.json().map_err(|e| e.to_string())?;
    Ok(body.access_token)
}
