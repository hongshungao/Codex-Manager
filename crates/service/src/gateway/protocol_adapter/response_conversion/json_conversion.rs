use serde_json::{json, Value};

fn map_openai_error_to_anthropic(value: &Value) -> Option<Value> {
    let error = value.get("error")?.as_object()?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("upstream request failed")
        .to_string();
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("api_error");

    let mapped_error_type = match error_type {
        "authentication_error" => "authentication_error",
        "permission_error" => "permission_error",
        "rate_limit_error" => "rate_limit_error",
        "invalid_request_error" | "not_found_error" => "invalid_request_error",
        _ => "api_error",
    };

    Some(json!({
        "type": "error",
        "error": {
            "type": mapped_error_type,
            "message": message,
        }
    }))
}

pub(super) fn convert_openai_json_to_anthropic(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "invalid upstream json response".to_string())?;
    if let Some(error_payload) = map_openai_error_to_anthropic(&value) {
        return serde_json::to_vec(&error_payload)
            .map(|bytes| (bytes, "application/json"))
            .map_err(|err| format!("serialize claude error response failed: {err}"));
    }

    let payload = if value.get("choices").is_some() {
        build_anthropic_message_from_chat_completions(&value)?
    } else {
        build_anthropic_message_from_responses(&value)?
    };

    serde_json::to_vec(&payload)
        .map(|bytes| (bytes, "application/json"))
        .map_err(|err| format!("serialize claude response failed: {err}"))
}

fn build_anthropic_message_from_chat_completions(value: &Value) -> Result<Value, String> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_codexmanager");

    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| "missing upstream choice".to_string())?;

    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing upstream message object".to_string())?;
    let content_text = extract_openai_text_content(message.get("content").unwrap_or(&Value::Null))?;
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stop_reason = if !tool_calls.is_empty() {
        "tool_use".to_string()
    } else {
        map_finish_reason(
            choice
                .get("finish_reason")
                .and_then(Value::as_str)
                .unwrap_or("stop"),
        )
        .to_string()
    };

    let mut content_blocks = Vec::new();
    if !content_text.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": content_text,
        }));
    }
    for (index, tool_call) in tool_calls.iter().enumerate() {
        let tool_use_id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .map(|v| v.to_string())
            .unwrap_or_else(|| format!("toolu_{index}"));
        let function_name = tool_call
            .get("function")
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| "missing tool call function name".to_string())?;
        let arguments_raw = tool_call
            .get("function")
            .and_then(|item| item.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let input = parse_tool_arguments_as_object(arguments_raw);
        content_blocks.push(json!({
            "type": "tool_use",
            "id": tool_use_id,
            "name": function_name,
            "input": input,
        }));
    }
    if content_blocks.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": "",
        }));
    }

    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("prompt_tokens"))
        .or_else(|| value.get("usage").and_then(|usage| usage.get("input_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("completion_tokens"))
        .or_else(|| value.get("usage").and_then(|usage| usage.get("output_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    Ok(json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    }))
}

fn build_anthropic_message_from_responses(value: &Value) -> Result<Value, String> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_codexmanager");

    let mut content_blocks = Vec::new();
    let mut has_tool_use = false;

    if let Some(output_text) = value.get("output_text").and_then(Value::as_str) {
        if !output_text.is_empty() {
            content_blocks.push(json!({
                "type": "text",
                "text": output_text,
            }));
        }
    }

    if let Some(output_items) = value.get("output").and_then(Value::as_array) {
        for output_item in output_items {
            let Some(item_obj) = output_item.as_object() else {
                continue;
            };
            let item_type = item_obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match item_type {
                "message" => {
                    let content = item_obj.get("content").and_then(Value::as_array);
                    if let Some(content) = content {
                        for block in content {
                            let Some(block_obj) = block.as_object() else {
                                continue;
                            };
                            let block_type = block_obj
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if block_type == "output_text" || block_type == "text" {
                                if let Some(text) = block_obj.get("text").and_then(Value::as_str) {
                                    content_blocks.push(json!({
                                        "type": "text",
                                        "text": text,
                                    }));
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let tool_use_id = item_obj
                        .get("call_id")
                        .or_else(|| item_obj.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("toolu_{}", content_blocks.len()));
                    let Some(function_name) = item_obj
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    else {
                        continue;
                    };
                    let input = extract_function_call_input_object(item_obj);
                    content_blocks.push(json!({
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": function_name,
                        "input": input,
                    }));
                    has_tool_use = true;
                }
                _ => {}
            }
        }
    }

    if content_blocks.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": "",
        }));
    }

    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .or_else(|| value.get("usage").and_then(|usage| usage.get("prompt_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("output_tokens"))
        .or_else(|| value.get("usage").and_then(|usage| usage.get("completion_tokens")))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let stop_reason = if has_tool_use {
        "tool_use".to_string()
    } else {
        "end_turn".to_string()
    };

    Ok(json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    }))
}


fn extract_openai_text_content(value: &Value) -> Result<String, String> {
    if value.is_null() {
        return Ok(String::new());
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    let Some(items) = value.as_array() else {
        return Err("unsupported upstream content format".to_string());
    };
    let mut parts = Vec::new();
    for item in items {
        let Some(item_obj) = item.as_object() else {
            continue;
        };
        if item_obj
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "text")
        {
            if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }
    Ok(parts.join(""))
}


pub(super) fn parse_tool_arguments_as_object(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    let parsed = serde_json::from_str::<Value>(trimmed).ok();
    match parsed {
        Some(Value::Object(obj)) => Value::Object(obj),
        Some(Value::String(inner)) => {
            let nested = serde_json::from_str::<Value>(inner.trim()).ok();
            if let Some(Value::Object(obj)) = nested {
                Value::Object(obj)
            } else {
                json!({ "value": inner })
            }
        }
        Some(other) => json!({ "value": other }),
        None => json!({}),
    }
}


fn extract_function_call_input_object(item_obj: &serde_json::Map<String, Value>) -> Value {
    let Some(arguments_raw) = extract_function_call_arguments_raw(item_obj) else {
        return json!({});
    };
    parse_tool_arguments_as_object(&arguments_raw)
}

pub(super) fn extract_function_call_arguments_raw(
    item_obj: &serde_json::Map<String, Value>,
) -> Option<String> {
    const ARGUMENT_KEYS: [&str; 5] = ["arguments", "input", "arguments_json", "parsed_arguments", "args"];
    for key in ARGUMENT_KEYS {
        let Some(value) = item_obj.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
            continue;
        }
        if let Ok(serialized) = serde_json::to_string(value) {
            let trimmed = serialized.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}


pub(super) fn map_finish_reason(reason: &str) -> &'static str {
    match reason {
        "tool_calls" => "tool_use",
        "length" => "max_tokens",
        "stop" => "end_turn",
        _ => "end_turn",
    }
}
