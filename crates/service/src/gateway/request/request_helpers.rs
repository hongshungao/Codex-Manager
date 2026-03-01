use reqwest::header::HeaderValue;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedRequestMetadata {
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) is_stream: bool,
    pub(crate) request_shape: Option<String>,
    pub(crate) has_prompt_cache_key: bool,
}

pub(crate) fn parse_request_metadata(body: &[u8]) -> ParsedRequestMetadata {
    if body.is_empty() {
        return ParsedRequestMetadata::default();
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return ParsedRequestMetadata::default();
    };
    let Some(object) = value.as_object() else {
        return ParsedRequestMetadata::default();
    };

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());

    let reasoning_effort = value
        .get("reasoning")
        .and_then(|v| v.get("effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .or_else(|| {
            value
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
        });

    let request_shape = Some(summarize_request_shape_from_object(object));
    let has_prompt_cache_key = value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|v| !v.is_empty());

    ParsedRequestMetadata {
        model,
        reasoning_effort,
        is_stream: value.get("stream").and_then(Value::as_bool).unwrap_or(false),
        request_shape,
        has_prompt_cache_key,
    }
}

fn summarize_request_shape_from_object(object: &serde_json::Map<String, Value>) -> String {
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort_unstable();
    let keys_joined = if keys.is_empty() {
        "-".to_string()
    } else {
        keys.join("+")
    };

    let input_count = object
        .get("input")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let messages_count = object
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let tools_count = object
        .get("tools")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let stream_flag = match object.get("stream").and_then(Value::as_bool) {
        Some(true) => "1",
        Some(false) => "0",
        None => "-",
    };
    let has_reasoning = if object.get("reasoning").is_some() { 1 } else { 0 };
    let has_instructions = if object
        .get("instructions")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
    {
        1
    } else {
        0
    };

    let shape = format!(
        "k={keys_joined};i={input_count};m={messages_count};t={tools_count};s={stream_flag};r={has_reasoning};ins={has_instructions}"
    );
    let digest = Sha256::digest(shape.as_bytes());
    let fingerprint = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("fp={fingerprint};{shape}")
}

#[cfg(test)]
pub(crate) fn should_drop_incoming_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.eq_ignore_ascii_case("Authorization")
        || name.eq_ignore_ascii_case("x-api-key")
        || name.eq_ignore_ascii_case("Host")
        || name.eq_ignore_ascii_case("Content-Length")
        // 中文注释：Claude SDK/CLI 会附带 anthropic/x-stainless 指纹头；
        // 直接透传到 ChatGPT upstream 会提高 challenge 概率，这里统一剔除。
        || lower.starts_with("anthropic-")
        || lower.starts_with("x-stainless-")
        // 中文注释：resume 会携带旧会话的账号头；若不剔除会把请求强行绑定到过期/耗尽账号，导致无法切换候选账号。
        || name.eq_ignore_ascii_case("ChatGPT-Account-Id")
}

#[cfg(test)]
pub(crate) fn should_drop_session_affinity_header(name: &str) -> bool {
    // 中文注释：session_id / turn-state 属于会话粘性信号，正常直连时应保留；
    // 仅在 failover 到其他账号时剔除，避免继续命中旧账号会话路由导致“切换无效”。
    name.eq_ignore_ascii_case("session_id") || name.eq_ignore_ascii_case("x-codex-turn-state")
}

#[cfg(test)]
pub(crate) fn should_drop_incoming_header_for_failover(name: &str) -> bool {
    should_drop_incoming_header(name) || should_drop_session_affinity_header(name)
}

pub(crate) fn is_upstream_challenge_response(
    status_code: u16,
    content_type: Option<&HeaderValue>,
) -> bool {
    let is_html = content_type
        .and_then(|v| v.to_str().ok())
        .map(is_html_content_type)
        .unwrap_or(false);
    // 中文注释：429 常见于业务限流/额度，不应统一映射成 Cloudflare challenge；
    // 仅在明确 HTML challenge 时按 challenge 处理，避免误报成 WAF 拦截。
    let _ = status_code;
    is_html
}

pub(crate) fn is_html_content_type(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("text/html")
}

pub(crate) fn normalize_models_path(path: &str) -> String {
    let is_models_path = path == "/v1/models" || path.starts_with("/v1/models?");
    if !is_models_path {
        return path.to_string();
    }
    let has_client_version = path
        .split_once('?')
        .map(|(_, query)| {
            query.split('&').any(|part| {
                part.split('=')
                    .next()
                    .is_some_and(|key| key.eq_ignore_ascii_case("client_version"))
            })
        })
        .unwrap_or(false);
    if has_client_version {
        return path.to_string();
    }
    let client_version = super::DEFAULT_MODELS_CLIENT_VERSION.to_string();
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{path}{separator}client_version={client_version}")
}
