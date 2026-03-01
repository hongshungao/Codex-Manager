use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::blocking::Client;
use reqwest::Method;
use serde_json::Value;
use std::time::Instant;
use tiny_http::Request;

fn body_has_encrypted_content_hint(body: &[u8]) -> bool {
    // Fast path: avoid JSON parsing unless we hit the recovery path.
    std::str::from_utf8(body)
        .ok()
        .is_some_and(|text| text.contains("\"encrypted_content\""))
}

fn strip_encrypted_content_value(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = map.remove("encrypted_content").is_some();
            for v in map.values_mut() {
                if strip_encrypted_content_value(v) {
                    changed = true;
                }
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                if strip_encrypted_content_value(item) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

fn strip_encrypted_content_from_body(body: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    if !strip_encrypted_content_value(&mut value) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

fn should_compact_upstream_headers() -> bool {
    super::cpa_no_cookie_header_mode_enabled()
}

pub(super) fn try_openai_fallback(
    client: &Client,
    storage: &Storage,
    method: &Method,
    request_path: &str,
    _request: &Request,
    incoming_headers: &super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let (url, _url_alt) = super::compute_upstream_url(upstream_base, request_path);
    let bearer = super::resolve_openai_bearer_token(storage, account, token)?;
    let attempt_started_at = Instant::now();
    let compact_headers_mode = should_compact_upstream_headers();

    // `x-codex-turn-state` is an org-scoped encrypted blob. When we hit API-key fallback
    // (often a different org than the ChatGPT workspace), forwarding it can trigger:
    // `invalid_encrypted_content` / organization_id mismatch. In that case, prefer
    // resetting session affinity to keep the request usable.
    let strip_session_affinity = strip_session_affinity || incoming_headers.turn_state().is_some();
    let body_for_request = if strip_session_affinity && body_has_encrypted_content_hint(body.as_ref()) {
        strip_encrypted_content_from_body(body.as_ref())
            .map(Bytes::from)
            .unwrap_or_else(|| body.clone())
    } else {
        body.clone()
    };

    let account_id = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref());
    let include_account_id = !compact_headers_mode && !super::is_openai_api_base(upstream_base);
    let header_input = super::upstream::header_profile::CodexUpstreamHeaderInput {
        auth_token: bearer.as_str(),
        account_id,
        include_account_id,
        include_openai_beta: !compact_headers_mode,
        upstream_cookie,
        incoming_session_id: incoming_headers.session_id(),
        fallback_session_id: None,
        incoming_turn_state: incoming_headers.turn_state(),
        include_turn_state: !compact_headers_mode,
        incoming_conversation_id: incoming_headers.conversation_id(),
        fallback_conversation_id: None,
        include_conversation_id: !compact_headers_mode,
        strip_session_affinity,
        is_stream,
        has_body: !body.is_empty(),
    };
    let upstream_headers =
        super::upstream::header_profile::build_codex_upstream_headers(header_input);
    if debug {
        log::debug!(
            "event=gateway_upstream_token_source path={} account_id={} token_source=api_key_access_token upstream_base={}",
            request_path,
            account_id.unwrap_or("-"),
            upstream_base
        );
    }
    let build_request = |http: &Client| {
        let mut builder = http.request(method.clone(), &url);
        for (name, value) in upstream_headers.iter() {
            builder = builder.header(name, value);
        }
        if !body_for_request.is_empty() {
            builder = builder.body(body_for_request.clone());
        }
        builder
    };
    let resp = match build_request(client).send() {
        Ok(resp) => resp,
        Err(first_err) => {
            let fresh = super::fresh_upstream_client_for_account(account.id.as_str());
            match build_request(&fresh).send() {
                Ok(resp) => resp,
                Err(second_err) => {
                    let duration_ms = super::duration_to_millis(attempt_started_at.elapsed());
                    super::metrics::record_gateway_upstream_attempt(duration_ms, true);
                    return Err(format!(
                        "{}; retry_after_fresh_client: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    let duration_ms = super::duration_to_millis(attempt_started_at.elapsed());
    super::metrics::record_gateway_upstream_attempt(duration_ms, false);
    Ok(Some(resp))
}
