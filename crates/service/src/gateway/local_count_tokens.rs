use serde_json::{json, Value};
use tiny_http::Response;

use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;

fn accumulate_text_len(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(accumulate_text_len).sum(),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.chars().count();
            }
            if let Some(content) = map.get("content") {
                return accumulate_text_len(content);
            }
            if let Some(input) = map.get("input") {
                return accumulate_text_len(input);
            }
            map.values().map(accumulate_text_len).sum()
        }
        _ => 0,
    }
}

fn estimate_input_tokens_from_anthropic_messages(body: &[u8]) -> Result<u64, String> {
    let payload: Value =
        serde_json::from_slice(body).map_err(|_| "invalid claude request json".to_string())?;
    let Some(object) = payload.as_object() else {
        return Err("claude request body must be an object".to_string());
    };

    let mut char_count = 0usize;
    if let Some(system) = object.get("system") {
        char_count += accumulate_text_len(system);
    }
    if let Some(messages) = object.get("messages").and_then(Value::as_array) {
        for message in messages {
            if let Some(content) = message.get("content") {
                char_count += accumulate_text_len(content);
            }
        }
    }

    // 中文注释：count_tokens 仅用于本地预算估计，采用稳定的轻量估算（约 4 chars/token）。
    let estimated = ((char_count as u64) / 4).max(1);
    Ok(estimated)
}

pub(super) fn maybe_respond_local_count_tokens(
    request: tiny_http::Request,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    path: &str,
    request_method: &str,
    body: &[u8],
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<tiny_http::Request>, String> {
    let is_anthropic_count_tokens = protocol_type == PROTOCOL_ANTHROPIC_NATIVE
        && request_method.eq_ignore_ascii_case("POST")
        && (path == "/v1/messages/count_tokens" || path.starts_with("/v1/messages/count_tokens?"));
    if !is_anthropic_count_tokens {
        return Ok(Some(request));
    }

    match estimate_input_tokens_from_anthropic_messages(body) {
        Ok(input_tokens) => {
            let output = json!({ "input_tokens": input_tokens }).to_string();
            super::trace_log::log_attempt_result(trace_id, "-", None, 200, None);
            super::trace_log::log_request_final(trace_id, 200, None, None, None, 0);
            super::record_gateway_request_outcome(path, 200, Some(protocol_type));
            super::write_request_log(
                storage,
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(200),
                super::request_log::RequestLogUsage {
                    input_tokens: Some(input_tokens.min(i64::MAX as u64) as i64),
                    cached_input_tokens: Some(0),
                    output_tokens: Some(0),
                    total_tokens: Some(input_tokens.min(i64::MAX as u64) as i64),
                    reasoning_output_tokens: Some(0),
                },
                None,
            );
            let response = Response::from_string(output)
                .with_status_code(200)
                .with_header(
                    tiny_http::Header::from_bytes(
                        b"content-type".as_slice(),
                        b"application/json".as_slice(),
                    )
                    .map_err(|_| "build content-type header failed".to_string())?,
                );
            let _ = request.respond(response);
            Ok(None)
        }
        Err(err) => {
            super::trace_log::log_attempt_result(trace_id, "-", None, 400, Some(err.as_str()));
            super::trace_log::log_request_final(trace_id, 400, None, None, Some(err.as_str()), 0);
            super::record_gateway_request_outcome(path, 400, Some(protocol_type));
            super::write_request_log(
                storage,
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(400),
                super::request_log::RequestLogUsage::default(),
                Some(err.as_str()),
            );
            let response = Response::from_string(err.clone()).with_status_code(400);
            let _ = request.respond(response);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_input_tokens_uses_messages_and_system_text() {
        let body = br#"{
            "model":"gpt-5.3-codex",
            "system":"abcdabcd",
            "messages":[
                {"role":"user","content":"abcd"},
                {"role":"assistant","content":[{"type":"text","text":"abcdabcd"}]}
            ]
        }"#;
        let count = estimate_input_tokens_from_anthropic_messages(body).expect("estimate failed");
        assert_eq!(count, 5);
    }
}
