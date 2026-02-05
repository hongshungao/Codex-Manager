use gpttools_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::header::CONTENT_TYPE;
use reqwest::blocking::Client;
use reqwest::Method;
use std::collections::HashMap;
use tiny_http::{Header, Request, Response, StatusCode};

use crate::account_availability::{evaluate_snapshot, Availability, is_available};
use crate::account_status::set_account_status;
use crate::auth_tokens;
use crate::storage_helpers::{hash_platform_key, open_storage};
use crate::usage_refresh;
use gpttools_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};

fn is_openai_api_base(_base: &str) -> bool {
    false
}

pub(crate) fn handle_gateway_request(mut request: Request) -> Result<(), String> {
    // 处理代理请求（鉴权后转发到上游）
    let debug = std::env::var("GPTTOOLS_GATEWAY_DEBUG").is_ok();
    if request.method().as_str() == "OPTIONS" {
        let response = Response::empty(204);
        let _ = request.respond(response);
        return Ok(());
    }

    if request.url() == "/health" {
        let response = Response::from_string("ok");
        let _ = request.respond(response);
        return Ok(());
    }

    // IMPORTANT: always consume request body before early-returning with auth errors.
    // Some clients (e.g. codex-cli) will report "stream disconnected before completion"
    // if the server responds and closes while the request body is still being sent.
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);

    let Some(platform_key) = extract_platform_key(&request) else {
        if debug {
            let remote = request
                .remote_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "<none>".to_string());
            let auth_scheme = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .and_then(|h| h.value.as_str().split_whitespace().next())
                .unwrap_or("<none>");
            let header_names = request
                .headers()
                .iter()
                .map(|h| h.field.as_str().as_str())
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "gateway auth missing: url={}, remote={}, has_auth={}, auth_scheme={}, has_x_api_key={}, headers=[{}]",
                request.url(),
                remote,
                request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization")),
                auth_scheme,
                request.headers().iter().any(|h| h.field.equiv("x-api-key")),
                header_names,
            );
        }
        let response = Response::from_string("missing api key").with_status_code(401);
        let _ = request.respond(response);
        return Ok(());
    };

    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let key_hash = hash_platform_key(&platform_key);
    let api_key = storage
        .find_api_key_by_hash(&key_hash)
        .map_err(|e| e.to_string())?;
    let Some(api_key) = api_key else {
        if debug {
            eprintln!(
                "gateway auth invalid: url={}, key_hash_prefix={}",
                request.url(),
                &key_hash[..8]
            );
        }
        let response = Response::from_string("invalid api key").with_status_code(403);
        let _ = request.respond(response);
        return Ok(());
    };
    if api_key.status != "active" {
        if debug {
            eprintln!("gateway auth disabled: url={}, key_id={}", request.url(), api_key.id);
        }
        let response = Response::from_string("api key disabled").with_status_code(403);
        let _ = request.respond(response);
        return Ok(());
    }
    let _ = storage.update_api_key_last_used(&key_hash);

    let candidates = collect_gateway_candidates(&storage)?;
    if candidates.is_empty() {
        let response = Response::from_string("no available account").with_status_code(503);
        let _ = request.respond(response);
        return Ok(());
    }

    let upstream_base = std::env::var("GPTTOOLS_UPSTREAM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://chatgpt.com/backend-api/codex".to_string());
    let upstream_fallback_base: Option<String> = None;
    let base = upstream_base.trim_end_matches('/');
    let path = request.url();
    let url = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        // Historically Codex upstream accepted paths without the "/v1" prefix.
        // Keep this as primary, but we'll retry with "/v1" kept if the upstream rejects it.
        format!("{}{}", base, path.trim_start_matches("/v1"))
    } else if base.ends_with("/v1") && path.starts_with("/v1") {
        format!("{}{}", base.trim_end_matches("/v1"), path)
    } else {
        format!("{}{}", base, path)
    };
    let url_alt: Option<String> = None;

    let method = Method::from_bytes(request.method().as_str().as_bytes())
        .map_err(|_| "unsupported method".to_string())?;

    let client = Client::new();
    let upstream_cookie = std::env::var("GPTTOOLS_UPSTREAM_COOKIE").ok();

    let candidate_count = candidates.len();
    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        if is_openai_api_base(base) {
            match try_openai_fallback(
                &client,
                &storage,
                &method,
                &request,
                &body,
                base,
                &account,
                &mut token,
                upstream_cookie.as_deref(),
                debug,
            ) {
                Ok(Some(resp)) => {
                    return respond_with_upstream(request, resp);
                }
                Ok(None) => {
                    let response = Response::from_string("openai upstream unavailable")
                        .with_status_code(502);
                    let _ = request.respond(response);
                    return Ok(());
                }
                Err(err) => {
                    let response = Response::from_string(format!("openai upstream error: {err}"))
                        .with_status_code(502);
                    let _ = request.respond(response);
                    return Ok(());
                }
            }
        }

        let mut builder = client.request(method.clone(), &url);
        let mut has_user_agent = false;

        for header in request.headers() {
            if header.field.equiv("Authorization")
                || header.field.equiv("x-api-key")
                || header.field.equiv("Host")
                || header.field.equiv("Content-Length")
            {
                continue;
            }
            if header.field.equiv("User-Agent") {
                has_user_agent = true;
            }
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(header.field.as_str().as_bytes()),
                HeaderValue::from_str(header.value.as_str()),
            ) {
                builder = builder.header(name, value);
            }
        }
        if !has_user_agent {
            builder = builder.header("User-Agent", "codex-cli");
        }
        if let Some(cookie) = upstream_cookie.as_ref() {
            if !cookie.trim().is_empty() {
                builder = builder.header("Cookie", cookie);
            }
        }

        let auth_token = token.access_token.clone();
        if debug {
            eprintln!(
                "gateway upstream: base={}, token_source=access_token",
                upstream_base
            );
        }
        builder = builder.header("Authorization", format!("Bearer {}", auth_token));
        if let Some(acc) = account
            .chatgpt_account_id
            .as_deref()
            .or_else(|| account.workspace_id.as_deref())
        {
            builder = builder.header("ChatGPT-Account-Id", acc);
        }

        if !body.is_empty() {
            builder = builder.body(body.clone());
        }

        let mut upstream = match builder.send() {
            Ok(resp) => resp,
            Err(err) => {
                let response = Response::from_string(format!("upstream error: {err}"))
                    .with_status_code(502);
                let _ = request.respond(response);
                return Ok(());
            }
        };

        let mut status = upstream.status();
        if should_try_openai_fallback(base, upstream.headers().get(CONTENT_TYPE)) {
            if let Some(fallback_base) = upstream_fallback_base.as_deref() {
                if debug {
                    eprintln!("gateway upstream fallback: from={} to={}", upstream_base, fallback_base);
                }
                match try_openai_fallback(
                    &client,
                    &storage,
                    &method,
                    &request,
                    &body,
                    fallback_base,
                    &account,
                    &mut token,
                    upstream_cookie.as_deref(),
                    debug,
                ) {
                    Ok(Some(resp)) => {
                        if resp.status().is_success() {
                            return respond_with_upstream(request, resp);
                        }
                        upstream = resp;
                        status = upstream.status();
                    }
                    Ok(None) => {
                        let response = Response::from_string(
                            "upstream blocked by Cloudflare; set GPTTOOLS_UPSTREAM_COOKIE or enable OpenAI API-key fallback",
                        )
                        .with_status_code(502);
                        let _ = request.respond(response);
                        return Ok(());
                    }
                    Err(err) => {
                        let response =
                            Response::from_string(format!("upstream fallback error: {err}"))
                                .with_status_code(502);
                        let _ = request.respond(response);
                        return Ok(());
                    }
                }
            } else {
                let response = Response::from_string(
                    "upstream returned HTML challenge; configure GPTTOOLS_UPSTREAM_COOKIE or GPTTOOLS_UPSTREAM_FALLBACK_BASE_URL",
                )
                .with_status_code(502);
                let _ = request.respond(response);
                return Ok(());
            }
        }
        if !status.is_success() {
            log::warn!(
                "gateway upstream non-success: status={}, account_id={}",
                status,
                account.id
            );
        }
        if (status.as_u16() == 400 || status.as_u16() == 404) && url_alt.is_some() {
            let alt_url = url_alt.as_ref().unwrap();
            if debug {
                eprintln!("gateway upstream retry: url={alt_url}");
            }
            let mut retry = client.request(method.clone(), alt_url);
            let mut has_user_agent = false;
            for header in request.headers() {
                if header.field.equiv("Authorization")
                    || header.field.equiv("x-api-key")
                    || header.field.equiv("Host")
                    || header.field.equiv("Content-Length")
                {
                    continue;
                }
                if header.field.equiv("User-Agent") {
                    has_user_agent = true;
                }
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(header.field.as_str().as_bytes()),
                    HeaderValue::from_str(header.value.as_str()),
                ) {
                    retry = retry.header(name, value);
                }
            }
            if !has_user_agent {
                retry = retry.header("User-Agent", "codex-cli");
            }
            if let Some(cookie) = upstream_cookie.as_ref() {
                if !cookie.trim().is_empty() {
                    retry = retry.header("Cookie", cookie);
                }
            }
            retry = retry.header("Authorization", format!("Bearer {}", auth_token));
            if let Some(acc) = account
                .chatgpt_account_id
                .as_deref()
                .or_else(|| account.workspace_id.as_deref())
            {
                retry = retry.header("ChatGPT-Account-Id", acc);
            }
            if !body.is_empty() {
                retry = retry.body(body.clone());
            }
            match retry.send() {
                Ok(resp) => upstream = resp,
                Err(err) => {
                    let response = Response::from_string(format!("upstream error: {err}"))
                        .with_status_code(502);
                    let _ = request.respond(response);
                    return Ok(());
                }
            }
            status = upstream.status();
        }
        if status.is_success() {
            return respond_with_upstream(request, upstream);
        }

        let refresh_result = usage_refresh::refresh_usage_for_account(&account.id);
        let should_failover = should_failover_after_refresh(&storage, &account.id, refresh_result);
        if should_failover && idx + 1 < candidate_count {
            continue;
        }

        return respond_with_upstream(request, upstream);
    }

    Err("no available account".to_string())
}

fn should_try_openai_fallback(base: &str, content_type: Option<&HeaderValue>) -> bool {
    if !base.contains("chatgpt.com/backend-api/codex") {
        return false;
    }
    let Some(content_type) = content_type else {
        return false;
    };
    let Ok(value) = content_type.to_str() else {
        return false;
    };
    is_html_content_type(value)
}

fn is_html_content_type(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("text/html")
}

fn compute_upstream_url(base: &str, path: &str) -> (String, Option<String>) {
    let base = base.trim_end_matches('/');
    let url = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        format!("{}{}", base, path.trim_start_matches("/v1"))
    } else if base.ends_with("/v1") && path.starts_with("/v1") {
        format!("{}{}", base.trim_end_matches("/v1"), path)
    } else {
        format!("{}{}", base, path)
    };
    let url_alt = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        Some(format!("{}{}", base, path))
    } else {
        None
    };
    (url, url_alt)
}

fn try_openai_fallback(
    client: &Client,
    storage: &gpttools_core::storage::Storage,
    method: &Method,
    request: &Request,
    body: &[u8],
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    debug: bool,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let (url, _url_alt) = compute_upstream_url(upstream_base, request.url());
    let client_id = std::env::var("GPTTOOLS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let issuer = account.issuer.clone();
    let bearer = match token.api_key_access_token.as_deref() {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => {
            let issuer_env = std::env::var("GPTTOOLS_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
            let issuer_for_exchange = if issuer.trim().is_empty() { issuer_env } else { issuer };
            let exchanged = auth_tokens::obtain_api_key(&issuer_for_exchange, &client_id, &token.id_token)?;
            token.api_key_access_token = Some(exchanged.clone());
            let _ = storage.insert_token(token);
            exchanged
        }
    };

    let mut builder = client.request(method.clone(), &url);
    let mut has_user_agent = false;
    for header in request.headers() {
        if header.field.equiv("Authorization")
            || header.field.equiv("x-api-key")
            || header.field.equiv("Host")
            || header.field.equiv("Content-Length")
        {
            continue;
        }
        if header.field.equiv("User-Agent") {
            has_user_agent = true;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if !has_user_agent {
        builder = builder.header("User-Agent", "codex-cli");
    }
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }
    if debug {
        eprintln!("gateway upstream: base={}, token_source=api_key_access_token", upstream_base);
    }
    builder = builder.header("Authorization", format!("Bearer {}", bearer));
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }
    let resp = builder.send().map_err(|e| e.to_string())?;
    Ok(Some(resp))
}

fn extract_platform_key(request: &Request) -> Option<String> {
    // 从请求头提取平台 Key
    for header in request.headers() {
        if header.field.equiv("Authorization") {
            let value = header.value.as_str();
            if let Some(rest) = value.strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
        }
        if header.field.equiv("x-api-key") {
            return Some(header.value.as_str().trim().to_string());
        }
    }
    None
}

fn collect_gateway_candidates(storage: &Storage) -> Result<Vec<(Account, Token)>, String> {
    // 选择可用账号作为网关上游候选
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let tokens = storage.list_tokens().map_err(|e| e.to_string())?;
    let snaps = storage
        .latest_usage_snapshots_by_account()
        .map_err(|e| e.to_string())?;
    let mut token_map = HashMap::new();
    for token in tokens {
        token_map.insert(token.account_id.clone(), token);
    }
    let mut snap_map = HashMap::new();
    for snap in snaps {
        snap_map.insert(snap.account_id.clone(), snap);
    }

    let mut out = Vec::new();
    for account in &accounts {
        if account.status != "active" {
            continue;
        }
        let token = match token_map.get(&account.id) {
            Some(token) => token.clone(),
            None => continue,
        };
        let usage = snap_map.get(&account.id);
        if !is_available(usage) {
            continue;
        }
        out.push((account.clone(), token));
    }
    if out.is_empty() {
        let mut fallback = Vec::new();
        for account in &accounts {
            let token = match token_map.get(&account.id) {
                Some(token) => token.clone(),
                None => continue,
            };
            let usage = snap_map.get(&account.id);
            if !fallback_allowed(usage) {
                continue;
            }
            fallback.push((account.clone(), token));
        }
        if !fallback.is_empty() {
            log::warn!("gateway fallback: no active accounts, using {} candidates", fallback.len());
            return Ok(fallback);
        }
    }
    if out.is_empty() {
        log_no_candidates(&accounts, &token_map, &snap_map);
    }
    Ok(out)
}

fn fallback_allowed(usage: Option<&UsageSnapshotRecord>) -> bool {
    if let Some(record) = usage {
        if let Some(value) = record.used_percent {
            if value >= 100.0 {
                return false;
            }
        }
        if let Some(value) = record.secondary_used_percent {
            if value >= 100.0 {
                return false;
            }
        }
    }
    true
}

fn log_no_candidates(
    accounts: &[Account],
    token_map: &HashMap<String, Token>,
    snap_map: &HashMap<String, UsageSnapshotRecord>,
) {
    let db_path = std::env::var("GPTTOOLS_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    log::warn!(
        "gateway no candidates: db_path={}, accounts={}, tokens={}, snapshots={}",
        db_path,
        accounts.len(),
        token_map.len(),
        snap_map.len()
    );
    for account in accounts {
        let usage = snap_map.get(&account.id);
        log::warn!(
            "gateway account: id={}, status={}, has_token={}, primary=({:?}/{:?}) secondary=({:?}/{:?})",
            account.id,
            account.status,
            token_map.contains_key(&account.id),
            usage.and_then(|u| u.used_percent),
            usage.and_then(|u| u.window_minutes),
            usage.and_then(|u| u.secondary_used_percent),
            usage.and_then(|u| u.secondary_window_minutes),
        );
    }
}

fn should_failover_after_refresh(
    storage: &Storage,
    account_id: &str,
    refresh_result: Result<(), String>,
) -> bool {
    match refresh_result {
        Ok(_) => {
            let snap = storage
                .latest_usage_snapshots_by_account()
                .ok()
                .and_then(|snaps| snaps.into_iter().find(|s| s.account_id == account_id));
            match snap.as_ref().map(evaluate_snapshot) {
                Some(Availability::Unavailable(reason)) => {
                    set_account_status(storage, account_id, "inactive", reason);
                    true
                }
                Some(Availability::Available) => false,
                None => {
                    set_account_status(storage, account_id, "inactive", "usage_missing_snapshot");
                    true
                }
            }
        }
        Err(err) => {
            if err.starts_with("usage endpoint status") {
                set_account_status(storage, account_id, "inactive", "usage_unreachable");
                true
            } else {
                false
            }
        }
    }
}

fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
) -> Result<(), String> {
    let status = StatusCode(upstream.status().as_u16());
    let mut headers = Vec::new();
    for (name, value) in upstream.headers().iter() {
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("transfer-encoding")
            || name_str.eq_ignore_ascii_case("content-length")
            || name_str.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
            headers.push(header);
        }
    }
    let len = upstream.content_length().map(|v| v as usize);
    let response = Response::new(status, headers, upstream, len, None);
    let _ = request.respond(response);
    Ok(())
}

#[cfg(test)]
mod availability_tests {
    use super::should_failover_after_refresh;
    use super::{compute_upstream_url, is_html_content_type};
    use gpttools_core::storage::{now_ts, Account, Storage, UsageSnapshotRecord};

    #[test]
    fn failover_on_missing_usage() {
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
        storage.insert_usage_snapshot(&record).expect("insert usage");

        let should_failover = should_failover_after_refresh(&storage, "acc-1", Ok(()));
        assert!(should_failover);
    }

    #[test]
    fn html_content_type_detection() {
        assert!(is_html_content_type("text/html; charset=utf-8"));
        assert!(is_html_content_type("TEXT/HTML"));
        assert!(!is_html_content_type("application/json"));
    }

    #[test]
    fn compute_url_trims_v1_prefix_for_codex_backend() {
        let (url, alt) = compute_upstream_url("https://chatgpt.com/backend-api/codex", "/v1/models");
        assert_eq!(url, "https://chatgpt.com/backend-api/codex/models");
        assert_eq!(
            alt.as_deref(),
            Some("https://chatgpt.com/backend-api/codex/v1/models")
        );
        let (url, alt) = compute_upstream_url("https://api.openai.com/v1", "/v1/models");
        assert_eq!(url, "https://api.openai.com/v1/models");
        assert!(alt.is_none());
    }


}
