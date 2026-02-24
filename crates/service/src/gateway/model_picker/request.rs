use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use reqwest::Method;

pub(super) fn send_models_request(
    client: &Client,
    storage: &Storage,
    method: &Method,
    upstream_base: &str,
    path: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
) -> Result<Vec<u8>, String> {
    let (url, _url_alt) = super::super::compute_upstream_url(upstream_base, path);
    let mut builder = client.request(method.clone(), &url);
    builder = builder.header("User-Agent", "codex-cli");
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }

    // 中文注释：OpenAI 基线要求 api_key_access_token，
    // 不这样区分会导致模型列表请求在 OpenAI 上游稳定 401。
    let bearer = if super::super::is_openai_api_base(upstream_base) {
        super::super::resolve_openai_bearer_token(storage, account, token)?
    } else {
        token.access_token.clone()
    };
    builder = builder.header("Authorization", format!("Bearer {}", bearer));
    if let Some(acc) = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref())
    {
        builder = builder.header("ChatGPT-Account-Id", acc);
    }

    let response = builder.send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!("models upstream failed: status={} body={}", status, body));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if super::super::is_html_content_type(content_type) {
        return Err("models upstream returned text/html (cloudflare challenge)".to_string());
    }

    response.bytes().map(|v| v.to_vec()).map_err(|e| e.to_string())
}
