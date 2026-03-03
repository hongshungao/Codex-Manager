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
fn chat_completions_uses_reasoning_effort_and_drops_non_official_keys() {
    let body = json!({
        "model": "gpt-4.1",
        "messages": [{ "role": "user", "content": "hi" }],
        "reasoning": { "effort": "high" },
        "metadata": { "source": "test" },
        "unknown_field": "drop-me"
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        Some("medium"),
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str),
        Some("medium")
    );
    assert!(value.get("reasoning").is_none());
    assert!(value.get("unknown_field").is_none());
    assert!(value.get("metadata").is_some());
}

#[test]
fn chat_completions_accepts_responses_style_payload() {
    let body = json!({
        "model": "gpt-4.1",
        "instructions": "act as reviewer",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ],
        "reasoning": { "effort": "high" },
        "stream": true
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("instructions").is_none());
    assert!(value.get("input").is_none());
    assert_eq!(
        value
            .get("messages")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("role"))
            .and_then(serde_json::Value::as_str),
        Some("system")
    );
    assert_eq!(
        value
            .get("messages")
            .and_then(|v| v.get(1))
            .and_then(|v| v.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        value
            .get("reasoning_effort")
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
    assert_eq!(
        value
            .get("stream_options")
            .and_then(|v| v.get("include_usage"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn chat_completions_normalizes_responses_function_tools() {
    let body = json!({
        "model": "gpt-4.1",
        "input": "ping",
        "tools": [
            {
                "type": "function",
                "name": "ping",
                "description": "ping tool",
                "parameters": { "type": "object", "properties": {} },
                "strict": true
            }
        ],
        "tool_choice": {
            "type": "function",
            "name": "ping"
        }
    });
    let out = apply_request_overrides(
        "/v1/chat/completions",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        None,
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value
            .get("tools")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("function"))
            .and_then(|v| v.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );
    assert_eq!(
        value
            .get("tool_choice")
            .and_then(|v| v.get("function"))
            .and_then(|v| v.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("ping")
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
        Some("https://chatgpt.com/backend-api/codex"),
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
        Some("https://chatgpt.com/backend-api/codex"),
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
}

#[test]
fn responses_stream_and_store_are_forced_for_codex_backend() {
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "store": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(false)
    );
}

#[test]
fn responses_stream_passthrough_keeps_client_stream_flag_when_enabled() {
    let body = json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false,
        "stream_passthrough": true,
        "store": true
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert!(value.get("stream_passthrough").is_none());
}

#[test]
fn responses_retains_only_codex_supported_fields() {
    let body = json!({
        "model": "gpt-5.3-codex",
        "instructions": "stay",
        "input": [{ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }],
        "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": { "effort": "high" },
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "pc_1",
        "encrypted_content": "gAAA_test_payload",
        "text": { "format": { "type": "text" } },
        "service_tier": "default",
        "temperature": 0.7,
        "user": "cherry-studio"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert!(value.get("model").is_some());
    assert!(value.get("instructions").is_some());
    assert!(value.get("input").is_some());
    assert!(value.get("tools").is_some());
    assert!(value.get("tool_choice").is_some());
    assert!(value.get("parallel_tool_calls").is_some());
    assert!(value.get("reasoning").is_some());
    assert!(value.get("stream").is_some());
    assert!(value.get("store").is_some());
    assert!(value.get("include").is_some());
    assert!(value.get("prompt_cache_key").is_some());
    assert!(value.get("encrypted_content").is_some());
    assert!(value.get("text").is_some());
    assert!(value.get("service_tier").is_none());
    assert!(value.get("temperature").is_none());
    assert!(value.get("user").is_none());
}

#[test]
fn responses_passthrough_for_non_codex_upstream() {
    let body = json!({
        "model": "gpt-4.1",
        "input": "hello",
        "stream": false,
        "store": true,
        "service_tier": "default",
        "user": "cherry-studio",
        "encrypted_content": "gAAA_test_payload",
        "unknown_field": "drop-me"
    });
    let out = apply_request_overrides(
        "/v1/responses",
        serde_json::to_vec(&body).expect("serialize request body"),
        None,
        None,
        Some("https://api.openai.com/v1"),
    );
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output body");
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value.get("store").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert!(value.get("service_tier").is_some());
    assert!(value.get("user").is_some());
    assert!(value.get("encrypted_content").is_none());
    assert!(value.get("unknown_field").is_none());
}

#[test]
fn non_matching_endpoint_keeps_non_json_body() {
    let body = b"foo=1&bar=2".to_vec();
    let out = apply_request_overrides("/v1/non-standard", body.clone(), None, None, None);
    assert_eq!(out, body);
}
