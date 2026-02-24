use super::*;

#[test]
fn anthropic_messages_request_maps_to_responses() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "system": "你是一个助手",
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "你好" }
                ]
            }
        ],
        "max_tokens": 512,
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize request");

    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(adapted.response_adapter, ResponseAdapter::AnthropicJson);

    let value: serde_json::Value = serde_json::from_slice(&adapted.body).expect("adapted json");
    assert_eq!(value["model"], "gpt-5.3-codex");
    assert_eq!(value["instructions"], "你是一个助手");
    assert_eq!(value["text"]["format"]["type"], "text");
    assert_eq!(value["input"][0]["role"], "user");
    assert_eq!(value["input"][0]["content"][0]["text"], "你好");
    assert!(value.get("max_output_tokens").is_none());
    assert_eq!(value["stream"], true);
}

#[test]
fn anthropic_messages_request_sets_prompt_cache_key() {
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [
            { "role": "user", "content": "hello" }
        ],
        "metadata": {
            "user_id": "usr_123"
        }
    });
    let body = serde_json::to_vec(&body).expect("serialize request");

    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    let value: serde_json::Value = serde_json::from_slice(&adapted.body).expect("adapted json");
    let key = value["prompt_cache_key"].as_str().unwrap_or_default();
    assert_eq!(key.len(), 36);
    assert!(key.contains('-'));
}

#[test]
fn anthropic_messages_request_drops_query_params() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "messages": [
            { "role": "user", "content": "hello" }
        ],
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize request");

    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages?beta=true", body)
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
}

#[test]
fn anthropic_tools_request_maps_to_openai_tools_and_tool_choice() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "messages": [
            { "role": "user", "content": "请读取README" }
        ],
        "tools": [
            {
                "name": "read_file",
                "description": "读取文件",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        ],
        "tool_choice": { "type": "tool", "name": "read_file" },
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize request");

    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    let value: serde_json::Value = serde_json::from_slice(&adapted.body).expect("adapted json");
    assert_eq!(value["tools"][0]["type"], "function");
    assert_eq!(value["tools"][0]["name"], "read_file");
    assert_eq!(value["tool_choice"]["type"], "function");
    assert_eq!(value["tool_choice"]["name"], "read_file");
}

#[test]
fn anthropic_tools_request_accepts_type_only_tool_definition() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "messages": [
            { "role": "user", "content": "hello" }
        ],
        "tools": [
            {
                "type": "bash_20250124",
                "input_schema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ],
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize request");
    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    let value: serde_json::Value = serde_json::from_slice(&adapted.body).expect("adapted json");
    assert_eq!(value["tools"][0]["name"], "bash_20250124");
}

#[test]
fn anthropic_stream_request_uses_sse_adapter() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": true
    });
    let body = serde_json::to_vec(&body).expect("serialize request");

    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    assert_eq!(adapted.response_adapter, ResponseAdapter::AnthropicSse);
}

#[test]
fn anthropic_request_ignores_unsupported_block_type() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4",
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "image", "source": "..." }
                ]
            }
        ]
    });
    let body = serde_json::to_vec(&body).expect("serialize request");
    let adapted = adapt_request_for_protocol("anthropic_native", "/v1/messages", body)
        .expect("adapt request");
    let value: serde_json::Value = serde_json::from_slice(&adapted.body).expect("adapted json");
    assert_eq!(value["input"].as_array().map(|items| items.len()), Some(0));
}

#[test]
fn anthropic_json_response_maps_from_openai_shape() {
    let upstream = serde_json::json!({
        "id": "chatcmpl-123",
        "model": "gpt-5.3-codex",
        "choices": [
            {
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "已完成"
                }
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 6
        }
    });
    let upstream = serde_json::to_vec(&upstream).expect("serialize upstream");
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("application/json"),
        &upstream,
    )
    .expect("adapt response");
    assert_eq!(content_type, "application/json");

    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic response");
    assert_eq!(value["type"], "message");
    assert_eq!(value["role"], "assistant");
    assert_eq!(value["stop_reason"], "end_turn");
    assert_eq!(value["usage"]["input_tokens"], 10);
    assert_eq!(value["usage"]["output_tokens"], 6);
}

#[test]
fn anthropic_json_response_maps_from_openai_responses_shape() {
    let upstream = serde_json::json!({
        "id": "resp_123",
        "model": "gpt-5.3-codex",
        "status": "completed",
        "output_text": "已完成",
        "output": [
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    { "type": "output_text", "text": "已完成" }
                ]
            }
        ],
        "usage": {
            "input_tokens": 8,
            "output_tokens": 5
        }
    });
    let upstream = serde_json::to_vec(&upstream).expect("serialize upstream");
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("application/json"),
        &upstream,
    )
    .expect("adapt response");
    assert_eq!(content_type, "application/json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic response");
    assert_eq!(value["type"], "message");
    assert_eq!(value["role"], "assistant");
    assert_eq!(value["usage"]["input_tokens"], 8);
    assert_eq!(value["usage"]["output_tokens"], 5);
}

#[test]
fn anthropic_json_response_maps_openai_tool_calls() {
    let upstream = serde_json::json!({
        "id": "chatcmpl-tool-1",
        "model": "gpt-5.3-codex",
        "choices": [
            {
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_abc",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"README.md\"}"
                            }
                        }
                    ]
                }
            }
        ],
        "usage": {
            "prompt_tokens": 11,
            "completion_tokens": 7
        }
    });
    let upstream = serde_json::to_vec(&upstream).expect("serialize upstream");
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("application/json"),
        &upstream,
    )
    .expect("adapt response");
    assert_eq!(content_type, "application/json");

    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic response");
    assert_eq!(value["stop_reason"], "tool_use");
    assert_eq!(value["content"][0]["type"], "tool_use");
    assert_eq!(value["content"][0]["id"], "call_abc");
    assert_eq!(value["content"][0]["name"], "read_file");
    assert_eq!(value["content"][0]["input"]["path"], "README.md");
}

#[test]
fn anthropic_json_response_maps_responses_function_call_input_field() {
    let upstream = serde_json::json!({
        "id": "resp_tool_input_1",
        "model": "gpt-5.3-codex",
        "status": "completed",
        "output": [
            {
                "type": "function_call",
                "call_id": "call_write_1",
                "name": "Write",
                "input": {
                    "file_path": "new.txt",
                    "content": "hello"
                }
            }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 8
        }
    });
    let upstream = serde_json::to_vec(&upstream).expect("serialize upstream");
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("application/json"),
        &upstream,
    )
    .expect("adapt response");
    assert_eq!(content_type, "application/json");

    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic response");
    assert_eq!(value["stop_reason"], "tool_use");
    assert_eq!(value["content"][0]["type"], "tool_use");
    assert_eq!(value["content"][0]["id"], "call_write_1");
    assert_eq!(value["content"][0]["name"], "Write");
    assert_eq!(value["content"][0]["input"]["file_path"], "new.txt");
    assert_eq!(value["content"][0]["input"]["content"], "hello");
}

#[test]
fn anthropic_json_response_maps_openai_error_body() {
    let upstream = serde_json::json!({
        "error": {
            "message": "The model `gpt-5.3-codex` does not exist or you do not have access to it.",
            "type": "invalid_request_error",
            "param": "model",
            "code": "model_not_found"
        }
    });
    let upstream = serde_json::to_vec(&upstream).expect("serialize upstream");
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("application/json"),
        &upstream,
    )
    .expect("adapt response");
    assert_eq!(content_type, "application/json");

    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic response");
    assert_eq!(value["type"], "error");
    assert_eq!(value["error"]["type"], "invalid_request_error");
    assert!(value["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("gpt-5.3-codex")));
}

#[test]
fn anthropic_json_response_maps_event_stream() {
    let upstream = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"prompt_tokens\":4},\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}],\"usage\":{\"completion_tokens\":6}}\n\n",
        "data: [DONE]\n\n",
    );
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicJson,
        Some("text/event-stream"),
        upstream.as_bytes(),
    )
    .expect("adapt stream to json");
    assert_eq!(content_type, "application/json");

    let value: serde_json::Value = serde_json::from_slice(&body).expect("anthropic json");
    assert_eq!(value["type"], "message");
    assert_eq!(value["content"][0]["type"], "text");
    assert_eq!(value["content"][0]["text"], "你好");
    assert_eq!(value["usage"]["input_tokens"], 4);
    assert_eq!(value["usage"]["output_tokens"], 6);
}

#[test]
fn anthropic_sse_response_maps_event_stream() {
    let upstream = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-5.3-codex\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicSse,
        Some("text/event-stream"),
        upstream.as_bytes(),
    )
    .expect("adapt stream");
    assert_eq!(content_type, "text/event-stream");

    let text = String::from_utf8(body).expect("utf8");
    assert!(text.contains("event: message_start"));
    assert!(text.contains("event: content_block_delta"));
    assert!(text.contains("event: message_stop"));
}

#[test]
fn anthropic_sse_response_maps_openai_responses_completed_event() {
    let upstream = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.3-codex\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"你好\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        "data: [DONE]\n\n",
    );
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicSse,
        Some("text/event-stream"),
        upstream.as_bytes(),
    )
    .expect("adapt stream");
    assert_eq!(content_type, "text/event-stream");
    let text = String::from_utf8(body).expect("utf8");
    assert!(text.contains("event: message_start"));
    assert!(text.contains("event: content_block_delta"));
    assert!(text.contains("event: message_stop"));
}

#[test]
fn anthropic_sse_response_maps_openai_tool_call_deltas() {
    let upstream = concat!(
        "data: {\"id\":\"chatcmpl-1\",\"model\":\"gpt-5.3-codex\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"\"}}]},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"README.md\\\"}\"}}]},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\",\"index\":0}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicSse,
        Some("text/event-stream"),
        upstream.as_bytes(),
    )
    .expect("adapt stream");
    assert_eq!(content_type, "text/event-stream");
    let text = String::from_utf8(body).expect("utf8");
    assert!(text.contains("\"type\":\"tool_use\""));
    assert!(text.contains("\"name\":\"read_file\""));
    assert!(text.contains("\"type\":\"input_json_delta\""));
    assert!(text.contains("event: message_stop"));
}

#[test]
fn anthropic_sse_response_uses_output_item_done_when_completed_output_empty() {
    let upstream = concat!(
        "data: {\"id\":\"resp_1\",\"model\":\"gpt-5.3-codex\",\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_write_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"new.txt\",\"content\":\"hello\"}}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.3-codex\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        "data: [DONE]\n\n",
    );
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::AnthropicSse,
        Some("text/event-stream"),
        upstream.as_bytes(),
    )
    .expect("adapt stream");
    assert_eq!(content_type, "text/event-stream");
    let text = String::from_utf8(body).expect("utf8");
    assert!(text.contains("\"type\":\"tool_use\""));
    assert!(text.contains("\"name\":\"Write\""));
    assert!(text.contains("\"type\":\"input_json_delta\""));
    assert!(text.contains("new.txt"));
    assert!(text.contains("event: message_stop"));
}
