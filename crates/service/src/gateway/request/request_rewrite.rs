use serde_json::Value;

pub(super) fn compute_upstream_url(base: &str, path: &str) -> (String, Option<String>) {
    let base = base.trim_end_matches('/');
    let url = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        // 中文注释：兼容 ChatGPT backend-api/codex 的路径约定；不做映射会导致 /v1/* 请求 404。
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

fn path_supports_reasoning_override(path: &str) -> bool {
    path.starts_with("/v1/responses") || path.starts_with("/v1/chat/completions")
}

fn is_stream_request(obj: &serde_json::Map<String, Value>) -> bool {
    obj.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

fn ensure_chat_completions_stream_usage_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !path.starts_with("/v1/chat/completions") {
        return false;
    }
    if !is_stream_request(obj) {
        return false;
    }
    let mut changed = false;
    let stream_options = obj
        .entry("stream_options".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !stream_options.is_object() {
        *stream_options = Value::Object(serde_json::Map::new());
        changed = true;
    }
    if let Some(stream_options_obj) = stream_options.as_object_mut() {
        let has_include_usage = stream_options_obj
            .get("include_usage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_include_usage {
            stream_options_obj.insert("include_usage".to_string(), Value::Bool(true));
            changed = true;
        }
    }
    changed
}

fn ensure_responses_instructions(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !path.starts_with("/v1/responses") {
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

fn ensure_responses_input_list(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !path.starts_with("/v1/responses") {
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

fn ensure_responses_stream_true(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !path.starts_with("/v1/responses") {
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

fn ensure_responses_store_false(path: &str, obj: &mut serde_json::Map<String, Value>) -> bool {
    if !path.starts_with("/v1/responses") {
        return false;
    }
    let store = obj
        .entry("store".to_string())
        .or_insert(Value::Bool(false));
    if store.as_bool() == Some(false) {
        return false;
    }
    // 中文注释：Codex upstream 对 /responses 要求 store=false；
    // 用户端若显式传 true，这里统一改写避免上游 400。
    *store = Value::Bool(false);
    true
}

pub(super) fn apply_request_overrides(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Vec<u8> {
    let normalized_model = model_slug.map(str::trim).filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .map(str::to_string);
    if path == "/v1/models" || path.starts_with("/v1/models?") {
        return body;
    }
    if body.is_empty() {
        return body;
    }
    let Ok(mut payload) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    let mut changed = false;
    if let Some(model) = normalized_model {
        obj.insert("model".to_string(), Value::String(model.to_string()));
        changed = true;
    }
    if let Some(level) = normalized_reasoning {
        if path_supports_reasoning_override(path) {
            let reasoning = obj
                .entry("reasoning".to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if !reasoning.is_object() {
                // 中文注释：某些客户端会把 reasoning 误传成字符串；不矫正为对象会导致 effort 覆盖失效。
                *reasoning = Value::Object(serde_json::Map::new());
                changed = true;
            }
            if let Some(reasoning_obj) = reasoning.as_object_mut() {
                reasoning_obj.insert("effort".to_string(), Value::String(level));
                changed = true;
            }
        }
    }
    if ensure_chat_completions_stream_usage_override(path, obj) {
        changed = true;
    }
    if ensure_responses_input_list(path, obj) {
        changed = true;
    }
    if ensure_responses_stream_true(path, obj) {
        changed = true;
    }
    if ensure_responses_store_false(path, obj) {
        changed = true;
    }
    if ensure_responses_instructions(path, obj) {
        changed = true;
    }
    if !changed {
        return body;
    }
    serde_json::to_vec(&payload).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::apply_request_overrides;
    use serde_json::json;

    #[test]
    fn chat_completions_stream_enforces_include_usage() {
        let body = json!({
            "model": "gpt-4o",
            "stream": true,
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let out = apply_request_overrides(
            "/v1/chat/completions",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value
                .get("stream_options")
                .and_then(|v| v.get("include_usage"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn chat_completions_stream_preserves_options_while_enabling_usage() {
        let body = json!({
            "model": "gpt-4o",
            "stream": true,
            "stream_options": { "include_usage": false, "foo": "bar" },
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let out = apply_request_overrides(
            "/v1/chat/completions",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value
                .get("stream_options")
                .and_then(|v| v.get("include_usage"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value
                .get("stream_options")
                .and_then(|v| v.get("foo"))
                .and_then(serde_json::Value::as_str),
            Some("bar")
        );
    }

    #[test]
    fn responses_overrides_model_and_reasoning_effort() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "reasoning": { "effort": "high" },
            "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }]
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            Some("gpt-5.3-codex"),
            Some("medium"),
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-5.3-codex")
        );
        assert_eq!(
            value
                .get("reasoning")
                .and_then(|v| v.get("effort"))
                .and_then(serde_json::Value::as_str),
            Some("medium")
        );
        assert_eq!(
            value
                .get("instructions")
                .and_then(serde_json::Value::as_str),
            Some("")
        );
    }

    #[test]
    fn responses_keeps_existing_instructions() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "instructions": "already-set",
            "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }]
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value
                .get("instructions")
                .and_then(serde_json::Value::as_str),
            Some("already-set")
        );
    }

    #[test]
    fn responses_input_string_normalized_to_list() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "input": "hello"
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("message")
        );
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("role"))
                .and_then(serde_json::Value::as_str),
            Some("user")
        );
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("content"))
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("input_text")
        );
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("content"))
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("text"))
                .and_then(serde_json::Value::as_str),
            Some("hello")
        );
    }

    #[test]
    fn responses_input_object_wrapped_to_list() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "input": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.as_array())
                .map(std::vec::Vec::len),
            Some(1)
        );
        assert_eq!(
            value
                .get("input")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("message")
        );
    }

    #[test]
    fn responses_stream_is_forced_to_true() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "input": "hello",
            "stream": false
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn responses_store_is_forced_to_false() {
        let body = json!({
            "model": "gpt-5.3-codex",
            "input": "hello",
            "store": true
        });
        let out = apply_request_overrides(
            "/v1/responses",
            serde_json::to_vec(&body).expect("serialize request body"),
            None,
            None,
        );
        let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
        assert_eq!(
            value.get("store").and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

}
