use serde_json::Value;

use super::request_rewrite_shared::{path_matches_template, retain_fields_with_allowlist};

pub(super) fn is_responses_path(path: &str) -> bool {
    path_matches_template(path, "/v1/responses")
}

pub(super) fn ensure_instructions(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    if obj.contains_key("instructions") {
        return false;
    }
    // 中文注释：对齐 CPA 的 Codex 请求构造：缺失 instructions 时补空字符串，
    // 避免部分上游对字段存在性更严格导致的 400。
    obj.insert("instructions".to_string(), Value::String(String::new()));
    true
}

pub(super) fn ensure_input_list(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let Some(input) = obj.get_mut("input") else {
        return false;
    };
    match input {
        Value::String(text) => {
            let mut content_part = serde_json::Map::new();
            content_part.insert("type".to_string(), Value::String("input_text".to_string()));
            content_part.insert("text".to_string(), Value::String(text.clone()));

            let mut message_item = serde_json::Map::new();
            message_item.insert("type".to_string(), Value::String("message".to_string()));
            message_item.insert("role".to_string(), Value::String("user".to_string()));
            message_item.insert(
                "content".to_string(),
                Value::Array(vec![Value::Object(content_part)]),
            );
            *input = Value::Array(vec![Value::Object(message_item)]);
            true
        }
        Value::Object(_) => {
            *input = Value::Array(vec![input.clone()]);
            true
        }
        _ => false,
    }
}

pub(super) fn ensure_stream_true(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let stream = obj
        .entry("stream".to_string())
        .or_insert(Value::Bool(false));
    if stream.as_bool() == Some(true) {
        return false;
    }
    // 中文注释：对齐 CPA 的 Codex executor：/responses 固定走上游 SSE，
    // 后续由网关按下游协议再聚合/透传，避免 backend-api/codex 在非流式形态返回 400。
    *stream = Value::Bool(true);
    true
}

pub(super) fn take_stream_passthrough_flag(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    obj.remove("stream_passthrough")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub(super) fn ensure_store_false(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let store = obj.entry("store".to_string()).or_insert(Value::Bool(false));
    if store.as_bool() == Some(false) {
        return false;
    }
    // 中文注释：Codex upstream 对 /responses 要求 store=false；
    // 用户端若显式传 true，这里统一改写避免上游 400。
    *store = Value::Bool(false);
    true
}

pub(super) fn apply_reasoning_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
    reasoning_effort: Option<&str>,
) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let Some(level) = reasoning_effort else {
        return false;
    };
    let reasoning = obj
        .entry("reasoning".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !reasoning.is_object() {
        // 中文注释：某些客户端会把 reasoning 误传成字符串；不矫正为对象会导致 effort 覆盖失效。
        *reasoning = Value::Object(serde_json::Map::new());
    }
    if let Some(reasoning_obj) = reasoning.as_object_mut() {
        reasoning_obj.insert("effort".to_string(), Value::String(level.to_string()));
        return true;
    }
    false
}

fn is_supported_openai_responses_key(key: &str) -> bool {
    matches!(
        key,
        "include"
            | "input"
            | "instructions"
            | "max_output_tokens"
            | "metadata"
            | "model"
            | "parallel_tool_calls"
            | "previous_response_id"
            | "reasoning"
            | "service_tier"
            | "store"
            | "stream"
            | "temperature"
            | "text"
            | "tool_choice"
            | "tools"
            | "top_p"
            | "truncation"
            | "user"
            | "stream_passthrough"
    )
}

pub(super) fn retain_official_fields(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> Vec<String> {
    if !is_responses_path(path) {
        return Vec::new();
    }
    retain_fields_with_allowlist(obj, is_supported_openai_responses_key)
}

fn is_supported_codex_responses_key(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "instructions"
            | "input"
            | "tools"
            | "tool_choice"
            | "parallel_tool_calls"
            | "reasoning"
            | "store"
            | "stream"
            | "include"
            | "prompt_cache_key"
            | "encrypted_content"
            | "text"
    )
}

pub(super) fn retain_codex_fields(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> Vec<String> {
    if !is_responses_path(path) {
        return Vec::new();
    }
    // 中文注释：仅保留 Codex CLI /responses 固定字段集合，其他字段全部丢弃，
    // 防止不同客户端夹带参数触发 Codex upstream 的 400。
    retain_fields_with_allowlist(obj, is_supported_codex_responses_key)
}
