use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::json_conversion::{
    convert_openai_json_to_anthropic, extract_function_call_arguments_raw, map_finish_reason,
    parse_tool_arguments_as_object,
};

pub(super) fn convert_anthropic_json_to_sse(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "invalid anthropic json response".to_string())?;
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "error")
    {
        let mut out = String::new();
        append_sse_event(&mut out, "error", &value);
        return Ok((out.into_bytes(), "text/event-stream"));
    }

    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_codexmanager");
    let response_model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let stop_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out = String::new();
    append_sse_event(
        &mut out,
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": response_id,
                "type": "message",
                "role": "assistant",
                "model": response_model,
                "content": [],
                "stop_reason": Value::Null,
                "stop_sequence": Value::Null,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                }
            }
        }),
    );

    let mut content_block_index = 0usize;
    for block in content {
        let Some(block_obj) = block.as_object() else {
            continue;
        };
        let block_type = block_obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match block_type {
            "text" => {
                let text = block_obj
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": content_block_index,
                        "content_block": { "type": "text", "text": "" }
                    }),
                );
                if !text.is_empty() {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": { "type": "text_delta", "text": text }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": content_block_index,
                    }),
                );
                content_block_index += 1;
            }
            "tool_use" => {
                let tool_input = block_obj.get("input").cloned().unwrap_or_else(|| json!({}));
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": content_block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": block_obj.get("id").cloned().unwrap_or_else(|| Value::String(format!("toolu_{}", content_block_index))),
                            "name": block_obj.get("name").cloned().unwrap_or_else(|| Value::String("tool".to_string())),
                            "input": json!({})
                        }
                    }),
                );
                if let Some(partial_json) = to_tool_input_partial_json(&tool_input) {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": partial_json,
                            }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": content_block_index,
                    }),
                );
                content_block_index += 1;
            }
            _ => {}
        }
    }

    if content_block_index == 0 {
        append_sse_event(
            &mut out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }),
        );
        append_sse_event(
            &mut out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": 0,
            }),
        );
    }

    append_sse_event(
        &mut out,
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
            "usage": { "output_tokens": output_tokens }
        }),
    );
    append_sse_event(&mut out, "message_stop", &json!({ "type": "message_stop" }));

    Ok((out.into_bytes(), "text/event-stream"))
}


pub(super) fn convert_openai_sse_to_anthropic(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let text = std::str::from_utf8(body).map_err(|_| "invalid upstream sse bytes".to_string())?;

    let mut response_id: Option<String> = None;
    let mut model: Option<String> = None;
    let mut finish_reason: Option<String> = None;
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;
    let mut content_text = String::new();
    let mut tool_calls: BTreeMap<usize, StreamingToolCall> = BTreeMap::new();
    let mut completed_response: Option<Value> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line.trim_start_matches("data:").trim();
        if payload == "[DONE]" {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        if let Some(event_type) = value.get("type").and_then(Value::as_str) {
            match event_type {
                "response.output_text.delta" => {
                    if let Some(fragment) = value.get("delta").and_then(Value::as_str) {
                        content_text.push_str(fragment);
                    }
                    continue;
                }
                "response.output_item.done" => {
                    let Some(item_obj) = value.get("item").and_then(Value::as_object) else {
                        continue;
                    };
                    if item_obj
                        .get("type")
                        .and_then(Value::as_str)
                        .map_or(true, |kind| kind != "function_call")
                    {
                        continue;
                    }
                    let index = value
                        .get("output_index")
                        .or_else(|| item_obj.get("index"))
                        .and_then(Value::as_u64)
                        .map(|v| v as usize)
                        .unwrap_or(tool_calls.len());
                    let entry = tool_calls.entry(index).or_default();
                    if let Some(id) = item_obj
                        .get("call_id")
                        .or_else(|| item_obj.get("id"))
                        .and_then(Value::as_str)
                    {
                        entry.id = Some(id.to_string());
                    }
                    if let Some(name) = item_obj.get("name").and_then(Value::as_str) {
                        entry.name = Some(name.to_string());
                    }
                    if let Some(arguments_raw) = extract_function_call_arguments_raw(item_obj) {
                        entry.arguments = arguments_raw;
                    }
                    continue;
                }
                "response.completed" => {
                    if let Some(response) = value.get("response") {
                        completed_response = Some(response.clone());
                        if response_id.is_none() {
                            response_id = response
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        if model.is_none() {
                            model = response
                                .get("model")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        if let Some(usage) = response.get("usage").and_then(Value::as_object) {
                            input_tokens = usage
                                .get("prompt_tokens")
                                .and_then(Value::as_i64)
                                .or_else(|| usage.get("input_tokens").and_then(Value::as_i64))
                                .unwrap_or(input_tokens);
                            output_tokens = usage
                                .get("completion_tokens")
                                .and_then(Value::as_i64)
                                .or_else(|| usage.get("output_tokens").and_then(Value::as_i64))
                                .unwrap_or(output_tokens);
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }

        if response_id.is_none() {
            response_id = value.get("id").and_then(Value::as_str).map(|v| v.to_string());
        }
        if model.is_none() {
            model = value
                .get("model")
                .and_then(Value::as_str)
                .map(|v| v.to_string());
        }
        if let Some(usage) = value.get("usage").and_then(Value::as_object) {
            input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .or_else(|| usage.get("input_tokens").and_then(Value::as_i64))
                .unwrap_or(input_tokens);
            output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .or_else(|| usage.get("output_tokens").and_then(Value::as_i64))
                .unwrap_or(output_tokens);
        }
        if let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        {
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                finish_reason = Some(reason.to_string());
            }
            if let Some(delta) = choice.get("delta") {
                if let Some(fragment) = delta.get("content").and_then(Value::as_str) {
                    content_text.push_str(fragment);
                } else if let Some(arr) = delta.get("content").and_then(Value::as_array) {
                    for item in arr {
                        if let Some(fragment) = item.get("text").and_then(Value::as_str) {
                            content_text.push_str(fragment);
                        }
                    }
                }
                if let Some(delta_tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for item in delta_tool_calls {
                        let Some(tool_obj) = item.as_object() else {
                            continue;
                        };
                        let index = tool_obj
                            .get("index")
                            .and_then(Value::as_u64)
                            .map(|value| value as usize)
                            .unwrap_or(0);
                        let entry = tool_calls.entry(index).or_default();
                        if let Some(id) = tool_obj.get("id").and_then(Value::as_str) {
                            entry.id = Some(id.to_string());
                        }
                        if let Some(function) = tool_obj.get("function").and_then(Value::as_object)
                        {
                            if let Some(name) = function.get("name").and_then(Value::as_str) {
                                entry.name = Some(name.to_string());
                            }
                            if let Some(arguments) =
                                function.get("arguments").and_then(Value::as_str)
                            {
                                entry.arguments.push_str(arguments);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(response) = completed_response {
        let completed_has_effective_output = response
            .get("output_text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
            || response
                .get("output")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty());
        let response_bytes = serde_json::to_vec(&response)
            .map_err(|err| format!("serialize completed response failed: {err}"))?;
        let (anthropic_json, _) = convert_openai_json_to_anthropic(&response_bytes)?;
        if completed_has_effective_output || (content_text.is_empty() && tool_calls.is_empty()) {
            return convert_anthropic_json_to_sse(&anthropic_json);
        }
    }

    let mapped_stop_reason = if tool_calls.is_empty() {
        map_finish_reason(finish_reason.as_deref().unwrap_or("stop"))
    } else {
        "tool_use"
    };
    let response_id = response_id.unwrap_or_else(|| "msg_codexmanager".to_string());
    let response_model = model.unwrap_or_else(|| "unknown".to_string());

    let mut out = String::new();
    let mut content_block_index: usize = 0;
    append_sse_event(
        &mut out,
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": response_id,
                "type": "message",
                "role": "assistant",
                "model": response_model,
                "content": [],
                "stop_reason": Value::Null,
                "stop_sequence": Value::Null,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                }
            }
        }),
    );
    if !content_text.is_empty() {
        append_sse_event(
            &mut out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": content_block_index,
                "content_block": {
                    "type": "text",
                    "text": "",
                }
            }),
        );
        append_sse_event(
            &mut out,
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": content_block_index,
                "delta": {
                    "type": "text_delta",
                    "text": content_text,
                }
            }),
        );
        append_sse_event(
            &mut out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": content_block_index,
            }),
        );
        content_block_index += 1;
    }

    for (idx, tool_call) in tool_calls {
        let tool_name = tool_call
            .name
            .clone()
            .unwrap_or_else(|| "tool".to_string());
        let tool_use_id = tool_call
            .id
            .clone()
            .unwrap_or_else(|| format!("toolu_{idx}"));
        let input = parse_tool_arguments_as_object(&tool_call.arguments);

        append_sse_event(
            &mut out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": content_block_index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool_use_id,
                    "name": tool_name,
                    "input": json!({}),
                }
            }),
        );
        if let Some(partial_json) = to_tool_input_partial_json(&input) {
            append_sse_event(
                &mut out,
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": content_block_index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": partial_json,
                    }
                }),
            );
        }
        append_sse_event(
            &mut out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": content_block_index,
            }),
        );
        content_block_index += 1;
    }
    if content_block_index == 0 {
        append_sse_event(
            &mut out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": "",
                }
            }),
        );
        append_sse_event(
            &mut out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": 0,
            }),
        );
    }

    append_sse_event(
        &mut out,
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": mapped_stop_reason,
                "stop_sequence": Value::Null,
            },
            "usage": {
                "output_tokens": output_tokens,
            }
        }),
    );
    append_sse_event(&mut out, "message_stop", &json!({ "type": "message_stop" }));

    Ok((out.into_bytes(), "text/event-stream"))
}

pub(super) fn convert_anthropic_sse_to_json(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let text = std::str::from_utf8(body).map_err(|_| "invalid anthropic sse bytes".to_string())?;
    let mut current_event: Option<String> = None;
    let mut response_id = "msg_codexmanager".to_string();
    let mut response_model = "unknown".to_string();
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;
    let mut stop_reason = "end_turn".to_string();
    let mut content_blocks: BTreeMap<usize, Value> = BTreeMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with("event:") {
            current_event = Some(line.trim_start_matches("event:").trim().to_string());
            continue;
        }
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line.trim_start_matches("data:").trim();
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        if current_event.as_deref() == Some("error") {
            let bytes = serde_json::to_vec(&value)
                .map_err(|err| format!("serialize anthropic error json failed: {err}"))?;
            return Ok((bytes, "application/json"));
        }
        match current_event.as_deref() {
            Some("message_start") => {
                if let Some(message) = value.get("message") {
                    response_id = message
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("msg_codexmanager")
                        .to_string();
                    response_model = message
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    input_tokens = message
                        .get("usage")
                        .and_then(|usage| usage.get("input_tokens"))
                        .and_then(Value::as_i64)
                        .unwrap_or(input_tokens);
                }
            }
            Some("content_block_start") => {
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .unwrap_or(content_blocks.len());
                if let Some(block) = value.get("content_block") {
                    content_blocks.insert(index, block.clone());
                }
            }
            Some("content_block_delta") => {
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .unwrap_or(0);
                let delta_type = value
                    .get("delta")
                    .and_then(|delta| delta.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if delta_type == "input_json_delta" {
                    let partial_json = value
                        .get("delta")
                        .and_then(|delta| delta.get("partial_json"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let input_value = parse_tool_arguments_as_object(partial_json);
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "tool_use",
                            "input": {},
                        })
                    });
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("input".to_string(), input_value);
                    }
                } else {
                    let fragment = value
                        .get("delta")
                        .and_then(|delta| delta.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "text",
                            "text": "",
                        })
                    });
                    if let Some(existing) = entry.get("text").and_then(Value::as_str) {
                        let mut merged = existing.to_string();
                        merged.push_str(fragment);
                        if let Some(obj) = entry.as_object_mut() {
                            obj.insert("text".to_string(), Value::String(merged));
                        }
                    }
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    stop_reason = reason.to_string();
                }
                output_tokens = value
                    .get("usage")
                    .and_then(|usage| usage.get("output_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(output_tokens);
            }
            _ => {}
        }
    }

    let mut blocks = content_blocks
        .into_iter()
        .map(|(_, block)| block)
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": "",
        }));
    }

    let out = json!({
        "id": response_id,
        "type": "message",
        "role": "assistant",
        "model": response_model,
        "content": blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    });
    let bytes =
        serde_json::to_vec(&out).map_err(|err| format!("serialize anthropic json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

#[derive(Default)]
struct StreamingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}


fn to_tool_input_partial_json(value: &Value) -> Option<String> {
    let serialized = serde_json::to_string(value).ok()?;
    if serialized == "{}" {
        return None;
    }
    Some(serialized)
}


fn append_sse_event(buffer: &mut String, event_name: &str, payload: &Value) {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    buffer.push_str("event: ");
    buffer.push_str(event_name);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(&data);
    buffer.push_str("\n\n");
}

