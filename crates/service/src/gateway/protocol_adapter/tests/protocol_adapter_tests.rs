use super::{
    adapt_request_for_protocol, adapt_upstream_response, convert_openai_chat_stream_chunk,
    convert_openai_completions_stream_chunk, ResponseAdapter,
};
use crate::apikey_profile::{PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_OPENAI_COMPAT};

#[test]
fn openai_chat_completions_are_adapted_to_responses() {
    let body = br#"{"model":"gpt-5.3-codex","messages":[{"role":"user","content":"hi"}]}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/chat/completions", body)
        .expect("adapt request");
    let value: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        adapted.response_adapter,
        ResponseAdapter::OpenAIChatCompletionsJson
    );
    assert_eq!(
        value
            .get("input")
            .and_then(|input| input.get(0))
            .and_then(|item| item.get("role"))
            .and_then(serde_json::Value::as_str),
        Some("user")
    );
    assert_eq!(
        value
            .get("input")
            .and_then(|input| input.get(0))
            .and_then(|item| item.get("content"))
            .and_then(|content| content.get(0))
            .and_then(|part| part.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("hi")
    );
    assert_eq!(
        value
            .get("stream_passthrough")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}

#[test]
fn openai_chat_completions_stream_uses_sse_adapter() {
    let body =
        br#"{"model":"gpt-5.3-codex","messages":[{"role":"user","content":"hi"}],"stream":true}"#
            .to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/chat/completions", body)
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        adapted.response_adapter,
        ResponseAdapter::OpenAIChatCompletionsSse
    );
}

#[test]
fn openai_chat_completions_stream_passthrough_is_forwarded() {
    let body = br#"{"model":"gpt-5.3-codex","messages":[{"role":"user","content":"hi"}],"stream":false,"stream_passthrough":true}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/chat/completions", body)
        .expect("adapt request");
    let value: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        value
            .get("stream_passthrough")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn openai_responses_passthrough_keeps_responses_path() {
    let body = br#"{"model":"gpt-5.3-codex","input":"hi"}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/responses", body.clone())
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(adapted.body, body);
    assert_eq!(adapted.response_adapter, ResponseAdapter::Passthrough);
}

#[test]
fn openai_completions_are_adapted_to_responses() {
    let body = br#"{"model":"gpt-5.3-codex","prompt":"hello","max_tokens":16}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/completions", body)
        .expect("adapt request");
    let value: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        adapted.response_adapter,
        ResponseAdapter::OpenAICompletionsJson
    );
    assert_eq!(
        value
            .get("input")
            .and_then(|input| input.get(0))
            .and_then(|item| item.get("role"))
            .and_then(serde_json::Value::as_str),
        Some("user")
    );
    assert_eq!(
        value
            .get("input")
            .and_then(|input| input.get(0))
            .and_then(|item| item.get("content"))
            .and_then(|content| content.get(0))
            .and_then(|part| part.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("hello")
    );
}

#[test]
fn openai_completions_stream_uses_sse_adapter() {
    let body = br#"{"model":"gpt-5.3-codex","prompt":"hello","stream":true}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_OPENAI_COMPAT, "/v1/completions", body)
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        adapted.response_adapter,
        ResponseAdapter::OpenAICompletionsSse
    );
}

#[test]
fn openai_chat_response_is_converted_from_responses_json() {
    let upstream = br#"{
        "id":"resp_1",
        "object":"response",
        "created":1700000000,
        "model":"gpt-5.3-codex",
        "output":[{"type":"message","content":[{"type":"output_text","text":"hello world"}]}],
        "usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}
    }"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("application/json"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value.get("object").and_then(serde_json::Value::as_str),
        Some("chat.completion")
    );
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello world")
    );
}

#[test]
fn openai_chat_response_is_converted_from_output_text_item() {
    let upstream = br#"{
        "id":"resp_1",
        "object":"response",
        "created":1700000000,
        "model":"gpt-5.3-codex",
        "output":[{"type":"output_text","text":"plain output item text"}],
        "usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}
    }"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("application/json"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("plain output item text")
    );
}

#[test]
fn openai_chat_stream_response_is_collapsed_to_chat_completion_json() {
    let upstream = br#"data: {"type":"response.output_text.delta","response_id":"resp_1","created":1700000001,"model":"gpt-5.3-codex","delta":"hello "}

data: {"type":"response.output_text.delta","response_id":"resp_1","created":1700000001,"model":"gpt-5.3-codex","delta":"world"}

data: {"type":"response.completed","response":{"id":"resp_1","created":1700000001,"model":"gpt-5.3-codex","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsSse,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello world")
    );
}

#[test]
fn openai_chat_stream_collapse_avoids_done_and_item_text_duplication() {
    let upstream = br#"data: {"type":"response.output_text.delta","response_id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","delta":"hello "}

data: {"type":"response.output_text.delta","response_id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","delta":"world"}

data: {"type":"response.output_text.done","response_id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","text":"hello world"}

data: {"type":"response.content_part.done","response_id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","part":{"type":"output_text","text":"hello world"}}

data: {"type":"response.output_item.done","response_id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","item":{"type":"message","content":[{"type":"output_text","text":"hello world"}]}}

data: {"type":"response.completed","response":{"id":"resp_dup_1","created":1700000010,"model":"gpt-5.3-codex","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("hello world")
    );
}

#[test]
fn openai_chat_stream_response_accepts_output_item_done_text() {
    let upstream = br#"data: {"type":"response.output_item.done","response_id":"resp_2","created":1700000002,"model":"gpt-5.3-codex","item":{"type":"message","content":[{"type":"output_text","text":"from output item"}]}}

data: {"type":"response.completed","response":{"id":"resp_2","created":1700000002,"model":"gpt-5.3-codex","usage":{"input_tokens":8,"output_tokens":3,"total_tokens":11}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("from output item")
    );
}

#[test]
fn openai_chat_stream_response_accepts_output_item_added_text() {
    let upstream = br#"data: {"type":"response.output_item.added","response_id":"resp_2b","created":1700000002,"model":"gpt-5.3-codex","item":{"type":"message","content":[{"type":"output_text","text":"from output item added"}]}}

data: {"type":"response.completed","response":{"id":"resp_2b","created":1700000002,"model":"gpt-5.3-codex","usage":{"input_tokens":8,"output_tokens":3,"total_tokens":11}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("from output item added")
    );
}

#[test]
fn openai_chat_stream_chunk_maps_function_call_argument_delta() {
    let value = serde_json::json!({
        "type": "response.function_call_arguments.delta",
        "response_id": "resp_call_1",
        "created": 1700000100,
        "model": "gpt-5.3-codex",
        "output_index": 0,
        "delta": "{\"x\":1}"
    });
    let mapped =
        convert_openai_chat_stream_chunk(&value).expect("map function_call_arguments.delta");
    assert_eq!(
        mapped.get("object").and_then(serde_json::Value::as_str),
        Some("chat.completion.chunk")
    );
    assert_eq!(
        mapped
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("tool_calls"))
            .and_then(|tool_calls| tool_calls.get(0))
            .and_then(|tool_call| tool_call.get("function"))
            .and_then(|function| function.get("arguments"))
            .and_then(serde_json::Value::as_str),
        Some("{\"x\":1}")
    );
}

#[test]
fn openai_chat_stream_chunk_fallback_maps_unknown_text_event() {
    let value = serde_json::json!({
        "type": "response.output_markdown.delta",
        "response_id": "resp_txt_1",
        "created": 1700000101,
        "model": "gpt-5.3-codex",
        "delta": "fallback text"
    });
    let mapped = convert_openai_chat_stream_chunk(&value).expect("map unknown text event");
    assert_eq!(
        mapped
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("delta"))
            .and_then(|delta| delta.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("fallback text")
    );
}

#[test]
fn openai_completions_stream_chunk_fallback_maps_unknown_text_event() {
    let value = serde_json::json!({
        "type": "response.output_markdown.delta",
        "response_id": "resp_txt_2",
        "created": 1700000102,
        "model": "gpt-5.3-codex",
        "delta": "completion fallback"
    });
    let mapped = convert_openai_completions_stream_chunk(&value).expect("map unknown text event");
    assert_eq!(
        mapped
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("completion fallback")
    );
}

#[test]
fn openai_chat_stream_response_completed_only_still_outputs_text() {
    let upstream = br#"data: {"type":"response.completed","response":{"id":"resp_3","created":1700000003,"model":"gpt-5.3-codex","output":[{"type":"message","content":[{"type":"output_text","text":"completed only text"}]}],"usage":{"input_tokens":8,"output_tokens":3,"total_tokens":11}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAIChatCompletionsJson,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str),
        Some("completed only text")
    );
}

#[test]
fn openai_completions_response_is_converted_from_responses_json() {
    let upstream = br#"{
        "id":"resp_1",
        "object":"response",
        "created":1700000000,
        "model":"gpt-5.3-codex",
        "output":[{"type":"message","content":[{"type":"output_text","text":"hello world"}]}],
        "usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}
    }"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAICompletionsJson,
        Some("application/json"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value.get("object").and_then(serde_json::Value::as_str),
        Some("text_completion")
    );
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("hello world")
    );
}

#[test]
fn openai_completions_stream_completed_only_still_outputs_text() {
    let upstream = br#"data: {"type":"response.completed","response":{"id":"resp_4","created":1700000004,"model":"gpt-5.3-codex","output":[{"type":"message","content":[{"type":"output_text","text":"completed only completion text"}]}],"usage":{"input_tokens":9,"output_tokens":4,"total_tokens":13}}}

data: [DONE]

"#;
    let (body, content_type) = adapt_upstream_response(
        ResponseAdapter::OpenAICompletionsJson,
        Some("text/event-stream"),
        upstream,
    )
    .expect("convert response");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse converted body");
    assert_eq!(content_type, "application/json");
    assert_eq!(
        value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("text"))
            .and_then(serde_json::Value::as_str),
        Some("completed only completion text")
    );
}

#[test]
fn anthropic_messages_are_the_only_path_adapted_to_responses() {
    let body =
        br#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hello"}]}"#.to_vec();
    let adapted = adapt_request_for_protocol(PROTOCOL_ANTHROPIC_NATIVE, "/v1/messages", body)
        .expect("adapt request");
    assert_eq!(adapted.path, "/v1/responses");
    assert_ne!(adapted.response_adapter, ResponseAdapter::Passthrough);
}

#[test]
fn anthropic_chat_completions_still_passthrough() {
    let body =
        br#"{"model":"gpt-5.3-codex","messages":[{"role":"user","content":"hello"}]}"#.to_vec();
    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/chat/completions",
        body.clone(),
    )
    .expect("adapt request");
    assert_eq!(adapted.path, "/v1/chat/completions");
    assert_eq!(adapted.body, body);
    assert_eq!(adapted.response_adapter, ResponseAdapter::Passthrough);
}
