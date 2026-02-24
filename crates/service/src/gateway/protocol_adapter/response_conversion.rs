use serde_json::json;

use crate::gateway::request_helpers::is_html_content_type;

use super::ResponseAdapter;
use json_conversion::convert_openai_json_to_anthropic;
use sse_conversion::{
    convert_anthropic_json_to_sse, convert_anthropic_sse_to_json, convert_openai_sse_to_anthropic,
};

mod json_conversion;
mod sse_conversion;

pub(super) fn adapt_upstream_response(
    adapter: ResponseAdapter,
    upstream_content_type: Option<&str>,
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    match adapter {
        ResponseAdapter::Passthrough => Ok((body.to_vec(), "application/octet-stream")),
        ResponseAdapter::AnthropicJson => {
            if upstream_content_type.is_some_and(is_html_content_type) {
                return Err("upstream returned html challenge".to_string());
            }
            let is_sse = upstream_content_type
                .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
                .unwrap_or(false);
            if is_sse || looks_like_sse_payload(body) {
                let (anthropic_sse, _) = convert_openai_sse_to_anthropic(body)?;
                return convert_anthropic_sse_to_json(&anthropic_sse);
            }
            convert_openai_json_to_anthropic(body)
        }
        ResponseAdapter::AnthropicSse => {
            if upstream_content_type.is_some_and(is_html_content_type) {
                return Err("upstream returned html challenge".to_string());
            }
            let is_json = upstream_content_type
                .map(|value| value.trim().to_ascii_lowercase().starts_with("application/json"))
                .unwrap_or(false);
            if is_json {
                let (anthropic_json, _) = convert_openai_json_to_anthropic(body)?;
                return convert_anthropic_json_to_sse(&anthropic_json);
            }
            convert_openai_sse_to_anthropic(body)
        }
    }
}

pub(super) fn build_anthropic_error_body(message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": message,
        }
    }))
    .unwrap_or_else(|_| b"{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"unknown error\"}}".to_vec())
}

fn looks_like_sse_payload(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let trimmed = text.trim_start();
    trimmed.starts_with("data:")
        || trimmed.starts_with("event:")
        || text.contains("\ndata:")
        || text.contains("\nevent:")
}
