use serde_json::{json, Map, Value};

use crate::gateway::request_helpers::is_html_content_type;

use super::ResponseAdapter;
use json_conversion::convert_openai_json_to_anthropic;
use sse_conversion::{
    convert_anthropic_json_to_sse, convert_anthropic_sse_to_json, convert_openai_sse_to_anthropic,
};

mod json_conversion;
mod sse_conversion;

fn extract_chat_content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    if !text.is_empty() {
                        parts.push(text.to_string());
                    }
                    continue;
                }
                if let Some(obj) = item.as_object() {
                    let item_type = obj.get("type").and_then(Value::as_str).unwrap_or_default();
                    if matches!(item_type, "text" | "input_text" | "output_text") {
                        if let Some(text) = obj.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                parts.push(text.to_string());
                            }
                        }
                    }
                }
            }
            parts.join("")
        }
        _ => String::new(),
    }
}

fn map_chat_choice_to_completion(choice: &Map<String, Value>, default_index: usize) -> Value {
    let mut out = Map::new();
    let index = choice
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or(default_index as i64);
    out.insert("index".to_string(), Value::Number(index.into()));
    let text = extract_chat_content_text(
        choice
            .get("text")
            .or_else(|| choice.get("message").and_then(|v| v.get("content")))
            .or_else(|| choice.get("delta").and_then(|v| v.get("content"))),
    );
    out.insert("text".to_string(), Value::String(text));
    if let Some(finish_reason) = choice.get("finish_reason") {
        if !finish_reason.is_null() {
            out.insert("finish_reason".to_string(), finish_reason.clone());
        }
    }
    if let Some(logprobs) = choice.get("logprobs") {
        out.insert("logprobs".to_string(), logprobs.clone());
    }
    Value::Object(out)
}

fn map_chat_response_to_completions(value: &Value) -> Value {
    if value
        .get("object")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "text_completion")
    {
        return value.clone();
    }

    let id = value
        .get("id")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let created = value
        .get("created")
        .cloned()
        .unwrap_or_else(|| Value::Number(0.into()));
    let model = value
        .get("model")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));

    let mut out = Map::new();
    out.insert("id".to_string(), id);
    out.insert(
        "object".to_string(),
        Value::String("text_completion".to_string()),
    );
    out.insert("created".to_string(), created);
    out.insert("model".to_string(), model);

    let mut choices = Vec::new();
    if let Some(arr) = value.get("choices").and_then(Value::as_array) {
        for (idx, choice) in arr.iter().enumerate() {
            if let Some(choice_obj) = choice.as_object() {
                choices.push(map_chat_choice_to_completion(choice_obj, idx));
            }
        }
    }
    out.insert("choices".to_string(), Value::Array(choices));

    if let Some(usage) = value.get("usage") {
        out.insert("usage".to_string(), usage.clone());
    } else if let Some(usage) = value
        .get("response")
        .and_then(|response| response.get("usage"))
    {
        out.insert("usage".to_string(), usage.clone());
    }
    Value::Object(out)
}

fn stream_event_response_id(value: &Value) -> String {
    value
        .get("response_id")
        .and_then(Value::as_str)
        .or_else(|| value.get("id").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("id"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .to_string()
}

fn stream_event_model(value: &Value) -> String {
    value
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("model"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .to_string()
}

fn stream_event_created(value: &Value) -> i64 {
    value
        .get("created")
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("created"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0)
}

fn extract_stream_event_text(value: &Value) -> String {
    if let Some(delta) = value.get("delta") {
        if let Some(text) = delta.as_str() {
            return text.to_string();
        }
        if let Some(text) = delta.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }

    let mut out = String::new();
    if let Some(part) = value.get("part").or_else(|| value.get("content_part")) {
        collect_text_from_response_content(part, &mut out);
        if !out.is_empty() {
            return out;
        }
    }

    if let Some(item) = value.get("item") {
        if let Some(content) = item.get("content") {
            collect_text_from_response_content(content, &mut out);
        } else {
            collect_text_from_response_content(item, &mut out);
        }
    }
    out
}

fn build_openai_completions_text_chunk(value: &Value, text: &str) -> Value {
    json!({
        "id": stream_event_response_id(value),
        "object": "text_completion",
        "created": stream_event_created(value),
        "model": stream_event_model(value),
        "choices": [{
            "index": 0,
            "text": text
        }]
    })
}

fn build_openai_chat_text_chunk(value: &Value, text: &str) -> Value {
    json!({
        "id": stream_event_response_id(value),
        "object": "chat.completion.chunk",
        "created": stream_event_created(value),
        "model": stream_event_model(value),
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": Value::Null
        }]
    })
}

fn map_response_event_to_openai_chat_tool_chunk(value: &Value) -> Option<Value> {
    let chunk_type = value.get("type").and_then(Value::as_str)?;
    let tool_call = match chunk_type {
        "response.output_item.added" | "response.output_item.done" => {
            let item = value
                .get("item")
                .or_else(|| value.get("output_item"))
                .and_then(Value::as_object)?;
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                return None;
            }
            let output_index = value
                .get("output_index")
                .and_then(Value::as_i64)
                .or_else(|| item.get("index").and_then(Value::as_i64))
                .unwrap_or(0);
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = if chunk_type == "response.output_item.added" {
                String::new()
            } else {
                item.get("arguments")
                    .map(|arguments| {
                        arguments.as_str().map(str::to_string).unwrap_or_else(|| {
                            serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
                        })
                    })
                    .unwrap_or_default()
            };
            let mut tool_call = json!({
                "index": output_index,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            });
            if !call_id.is_empty() {
                tool_call["id"] = Value::String(call_id);
            }
            tool_call
        }
        "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
            let arguments = value
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| value.get("arguments").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            if arguments.is_empty() {
                return None;
            }
            let output_index = value
                .get("output_index")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let mut tool_call = json!({
                "index": output_index,
                "function": {
                    "arguments": arguments
                }
            });
            if let Some(call_id) = value
                .get("call_id")
                .or_else(|| value.get("item_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|id| !id.is_empty())
            {
                tool_call["id"] = Value::String(call_id);
            }
            tool_call
        }
        _ => return None,
    };

    let include_role = chunk_type == "response.output_item.added";
    let mut chunk = json!({
        "id": stream_event_response_id(value),
        "object": "chat.completion.chunk",
        "created": stream_event_created(value),
        "model": stream_event_model(value),
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": []
            },
            "finish_reason": Value::Null
        }]
    });
    if include_role {
        chunk["choices"][0]["delta"]["role"] = Value::String("assistant".to_string());
    }
    chunk["choices"][0]["delta"]["tool_calls"] = Value::Array(vec![tool_call]);
    Some(chunk)
}

pub(super) fn convert_openai_completions_stream_chunk(value: &Value) -> Option<Value> {
    if let Some(chunk_type) = value.get("type").and_then(Value::as_str) {
        match chunk_type {
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.content_part.delta"
            | "response.content_part.done"
            | "response.output_item.done" => {
                let is_tool_call_item = value
                    .get("item")
                    .or_else(|| value.get("output_item"))
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    .is_some_and(|item_type| item_type == "function_call");
                if is_tool_call_item {
                    return None;
                }
                let text = extract_stream_event_text(value);
                if text.is_empty() {
                    return None;
                }
                return Some(build_openai_completions_text_chunk(value, text.as_str()));
            }
            "response.completed" => {
                let response = value.get("response").unwrap_or(&Value::Null);
                let usage = response
                    .get("usage")
                    .cloned()
                    .or_else(|| value.get("usage").cloned());
                let mut out = json!({
                    "id": response.get("id").and_then(Value::as_str).unwrap_or(""),
                    "object": "text_completion",
                    "created": response.get("created").and_then(Value::as_i64).unwrap_or(0),
                    "model": response.get("model").and_then(Value::as_str).unwrap_or(""),
                    "choices": [{
                        "index": 0,
                        "text": "",
                        "finish_reason": "stop"
                    }]
                });
                if let Some(usage) = usage {
                    if let Some(out_obj) = out.as_object_mut() {
                        out_obj.insert("usage".to_string(), usage);
                    }
                }
                return Some(out);
            }
            _ => {}
        }

        if chunk_type == "response.function_call_arguments.delta"
            || chunk_type == "response.function_call_arguments.done"
        {
            return None;
        }
        let text = extract_stream_event_text(value);
        if !text.is_empty() {
            return Some(build_openai_completions_text_chunk(value, text.as_str()));
        }
    }

    if value
        .get("object")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "chat.completion.chunk")
    {
        let mapped = map_chat_response_to_completions(value);
        let has_usage = mapped.get("usage").is_some();
        let has_meaningful_choice =
            mapped
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|choices| {
                    choices.iter().any(|choice| {
                        choice
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.is_empty())
                            || choice.get("finish_reason").is_some()
                    })
                });
        if has_usage || has_meaningful_choice {
            return Some(mapped);
        }
        return None;
    }

    if value.get("choices").is_some() || value.get("usage").is_some() {
        return Some(map_chat_response_to_completions(value));
    }
    None
}

fn convert_openai_json_to_completions(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "invalid upstream json payload".to_string())?;
    let mapped = if value
        .get("object")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "response")
        || value.get("output").is_some()
    {
        let chat_mapped = map_openai_response_to_chat_completion(&value);
        map_chat_response_to_completions(&chat_mapped)
    } else {
        map_chat_response_to_completions(&value)
    };
    let bytes = serde_json::to_vec(&mapped)
        .map_err(|err| format!("serialize completions json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

fn convert_openai_sse_to_completions_json(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    let text = std::str::from_utf8(body).map_err(|_| "invalid upstream sse bytes".to_string())?;
    let mut id = String::new();
    let mut model = String::new();
    let mut created: i64 = 0;
    let mut text_out = String::new();
    let mut finish_reason: Option<Value> = None;
    let mut usage: Option<Value> = None;
    let mut completed_response: Option<Value> = None;
    let mut data_lines = Vec::<String>::new();
    let mut saw_text_delta = false;

    let flush_frame = |lines: &mut Vec<String>,
                       id: &mut String,
                       model: &mut String,
                       created: &mut i64,
                       text_out: &mut String,
                       finish_reason: &mut Option<Value>,
                       usage: &mut Option<Value>,
                       completed_response: &mut Option<Value>,
                       saw_text_delta: &mut bool| {
        if lines.is_empty() {
            return;
        }
        let data = lines.join("\n");
        lines.clear();
        if data.trim() == "[DONE]" {
            if finish_reason.is_none() {
                *finish_reason = Some(Value::String("stop".to_string()));
            }
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            return;
        };
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "response.output_text.delta" {
            *saw_text_delta = true;
        }
        if matches!(
            event_type,
            "response.output_text.done"
                | "response.content_part.added"
                | "response.content_part.delta"
                | "response.content_part.done"
        ) {
            return;
        }
        if matches!(
            event_type,
            "response.output_item.added" | "response.output_item.done"
        ) {
            let is_function_call_item = value
                .get("item")
                .or_else(|| value.get("output_item"))
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|item_type| item_type == "function_call");
            if !is_function_call_item && *saw_text_delta {
                return;
            }
        }
        if value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "response.completed")
        {
            if let Some(response) = value.get("response") {
                *completed_response = Some(response.clone());
            }
        }
        let Some(chunk) = convert_openai_completions_stream_chunk(&value) else {
            return;
        };
        if let Some(v) = chunk.get("id").and_then(Value::as_str) {
            if !v.is_empty() {
                *id = v.to_string();
            }
        }
        if let Some(v) = chunk.get("model").and_then(Value::as_str) {
            if !v.is_empty() {
                *model = v.to_string();
            }
        }
        if let Some(v) = chunk.get("created").and_then(Value::as_i64) {
            *created = v;
        }
        if let Some(chunk_usage) = chunk.get("usage") {
            *usage = Some(chunk_usage.clone());
        }
        if let Some(choices) = chunk.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(text_piece) = choice.get("text").and_then(Value::as_str) {
                    text_out.push_str(text_piece);
                }
                if let Some(reason) = choice.get("finish_reason") {
                    if !reason.is_null() {
                        *finish_reason = Some(reason.clone());
                    }
                }
            }
        }
    };

    for line in text.lines() {
        if line.starts_with("data:") {
            data_lines.push(line[5..].trim_start().to_string());
            continue;
        }
        if line.trim().is_empty() {
            flush_frame(
                &mut data_lines,
                &mut id,
                &mut model,
                &mut created,
                &mut text_out,
                &mut finish_reason,
                &mut usage,
                &mut completed_response,
                &mut saw_text_delta,
            );
        }
    }
    flush_frame(
        &mut data_lines,
        &mut id,
        &mut model,
        &mut created,
        &mut text_out,
        &mut finish_reason,
        &mut usage,
        &mut completed_response,
        &mut saw_text_delta,
    );

    if text_out.is_empty() {
        if let Some(response) = completed_response.as_ref() {
            let chat_completion = map_openai_response_to_chat_completion(response);
            let completion = map_chat_response_to_completions(&chat_completion);
            if let Some(v) = completion.get("id").and_then(Value::as_str) {
                if !v.is_empty() {
                    id = v.to_string();
                }
            }
            if let Some(v) = completion.get("model").and_then(Value::as_str) {
                if !v.is_empty() {
                    model = v.to_string();
                }
            }
            if let Some(v) = completion.get("created").and_then(Value::as_i64) {
                created = v;
            }
            if usage.is_none() {
                if let Some(v) = completion.get("usage") {
                    usage = Some(v.clone());
                }
            }
            if let Some(choice) = completion
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
            {
                if let Some(v) = choice.get("text").and_then(Value::as_str) {
                    text_out.push_str(v);
                }
                if finish_reason.is_none() {
                    if let Some(v) = choice.get("finish_reason") {
                        if !v.is_null() {
                            finish_reason = Some(v.clone());
                        }
                    }
                }
            }
        }
    }

    let mut out = json!({
        "id": id,
        "object": "text_completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "text": text_out
        }]
    });
    if let Some(reason) = finish_reason {
        if let Some(choice_obj) = out
            .get_mut("choices")
            .and_then(Value::as_array_mut)
            .and_then(|choices| choices.get_mut(0))
            .and_then(Value::as_object_mut)
        {
            choice_obj.insert("finish_reason".to_string(), reason);
        }
    }
    if let Some(usage) = usage {
        if let Some(out_obj) = out.as_object_mut() {
            out_obj.insert("usage".to_string(), usage);
        }
    }

    let bytes = serde_json::to_vec(&out)
        .map_err(|err| format!("serialize completions json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

fn collect_text_from_response_content(content: &Value, out: &mut String) {
    match content {
        Value::String(text) => out.push_str(text),
        Value::Array(items) => {
            for item in items {
                let Some(item_obj) = item.as_object() else {
                    if let Some(text) = item.as_str() {
                        out.push_str(text);
                    }
                    continue;
                };
                let item_type = item_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(item_type, "text" | "input_text" | "output_text") {
                    if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
            if let Some(content) = map.get("content") {
                collect_text_from_response_content(content, out);
            }
        }
        _ => {}
    }
}

fn append_text_from_response_output_item(item_obj: &Map<String, Value>, out: &mut String) {
    if let Some(content) = item_obj.get("content") {
        collect_text_from_response_content(content, out);
    }
    if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
        out.push_str(text);
    }
    if let Some(delta) = item_obj.get("delta").and_then(Value::as_str) {
        out.push_str(delta);
    }
    if let Some(part) = item_obj
        .get("part")
        .or_else(|| item_obj.get("content_part"))
    {
        collect_text_from_response_content(part, out);
    }
}

fn map_openai_response_to_chat_completion(value: &Value) -> Value {
    if value
        .get("object")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "chat.completion")
    {
        return value.clone();
    }
    let source = value.get("response").unwrap_or(value);
    let id = source
        .get("id")
        .cloned()
        .or_else(|| value.get("id").cloned())
        .unwrap_or_else(|| Value::String(String::new()));
    let created = source
        .get("created")
        .cloned()
        .or_else(|| value.get("created").cloned())
        .unwrap_or_else(|| Value::Number(0.into()));
    let model = source
        .get("model")
        .cloned()
        .or_else(|| value.get("model").cloned())
        .unwrap_or_else(|| Value::String(String::new()));
    let usage = source
        .get("usage")
        .cloned()
        .or_else(|| value.get("usage").cloned());

    let mut assistant_text = String::new();
    let mut tool_calls = Vec::<Value>::new();
    if let Some(output_items) = source.get("output").and_then(Value::as_array) {
        for (idx, item) in output_items.iter().enumerate() {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            let item_type = item_obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if item_type == "message" {
                append_text_from_response_output_item(item_obj, &mut assistant_text);
                continue;
            }
            if item_type == "function_call" {
                let call_id = item_obj
                    .get("call_id")
                    .or_else(|| item_obj.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{idx}"));
                let name = item_obj
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| "tool".to_string());
                let arguments = item_obj
                    .get("arguments")
                    .map(|arguments| {
                        if let Some(text) = arguments.as_str() {
                            text.to_string()
                        } else {
                            serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
                        }
                    })
                    .unwrap_or_else(|| "{}".to_string());
                tool_calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments
                    }
                }));
                continue;
            }
            append_text_from_response_output_item(item_obj, &mut assistant_text);
        }
    }
    if assistant_text.is_empty() {
        if let Some(fallback_text) = source.get("output_text").and_then(Value::as_str) {
            assistant_text.push_str(fallback_text);
        }
    }

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if !assistant_text.is_empty() {
        message.insert("content".to_string(), Value::String(assistant_text));
    } else {
        message.insert("content".to_string(), Value::Null);
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let mut out = serde_json::Map::new();
    out.insert("id".to_string(), id);
    out.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    out.insert("created".to_string(), created);
    out.insert("model".to_string(), model);
    out.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": "stop"
        })]),
    );
    if let Some(usage) = usage {
        out.insert("usage".to_string(), usage);
    }
    Value::Object(out)
}

pub(super) fn convert_openai_chat_stream_chunk(value: &Value) -> Option<Value> {
    if value
        .get("object")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "chat.completion.chunk")
    {
        return Some(value.clone());
    }

    if let Some(chunk_type) = value.get("type").and_then(Value::as_str) {
        match chunk_type {
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.content_part.added"
            | "response.content_part.delta"
            | "response.content_part.done" => {
                let text = extract_stream_event_text(value);
                if text.is_empty() {
                    return None;
                }
                return Some(build_openai_chat_text_chunk(value, text.as_str()));
            }
            "response.output_item.added" | "response.output_item.done" => {
                if let Some(tool_chunk) = map_response_event_to_openai_chat_tool_chunk(value) {
                    return Some(tool_chunk);
                }
                let text = extract_stream_event_text(value);
                if text.is_empty() {
                    return None;
                }
                return Some(build_openai_chat_text_chunk(value, text.as_str()));
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                return map_response_event_to_openai_chat_tool_chunk(value);
            }
            "response.completed" => {
                let response = value.get("response").unwrap_or(&Value::Null);
                let fallback_id = stream_event_response_id(value);
                let fallback_model = stream_event_model(value);
                let fallback_created = stream_event_created(value);
                let response_id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or(fallback_id);
                let response_model = response
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or(fallback_model);
                let response_created = response
                    .get("created")
                    .and_then(Value::as_i64)
                    .or_else(|| response.get("created_at").and_then(Value::as_i64))
                    .unwrap_or(fallback_created);
                let usage = response
                    .get("usage")
                    .cloned()
                    .or_else(|| value.get("usage").cloned());
                let finish_reason = if response
                    .get("output")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type")
                                .and_then(Value::as_str)
                                .is_some_and(|item_type| item_type == "function_call")
                        })
                    }) {
                    "tool_calls"
                } else {
                    "stop"
                };
                let mut out = json!({
                    "id": response_id,
                    "object": "chat.completion.chunk",
                    "created": response_created,
                    "model": response_model,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason
                    }]
                });
                if let Some(usage) = usage {
                    if let Some(out_obj) = out.as_object_mut() {
                        out_obj.insert("usage".to_string(), usage);
                    }
                }
                return Some(out);
            }
            _ => {}
        }

        if let Some(tool_chunk) = map_response_event_to_openai_chat_tool_chunk(value) {
            return Some(tool_chunk);
        }
        let text = extract_stream_event_text(value);
        if !text.is_empty() {
            return Some(build_openai_chat_text_chunk(value, text.as_str()));
        }
    }
    None
}

fn convert_openai_json_to_chat_completions(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "invalid upstream json payload".to_string())?;
    let mapped = map_openai_response_to_chat_completion(&value);
    let bytes = serde_json::to_vec(&mapped)
        .map_err(|err| format!("serialize chat.completion json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

fn convert_openai_sse_to_chat_completions_json(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let text = std::str::from_utf8(body).map_err(|_| "invalid upstream sse bytes".to_string())?;
    let mut id = String::new();
    let mut model = String::new();
    let mut created: i64 = 0;
    let mut content = String::new();
    let mut finish_reason: Option<Value> = None;
    let mut usage: Option<Value> = None;
    let mut completed_response: Option<Value> = None;
    let mut data_lines = Vec::<String>::new();
    let mut saw_text_delta = false;

    let flush_frame = |lines: &mut Vec<String>,
                       id: &mut String,
                       model: &mut String,
                       created: &mut i64,
                       content: &mut String,
                       finish_reason: &mut Option<Value>,
                       usage: &mut Option<Value>,
                       completed_response: &mut Option<Value>,
                       saw_text_delta: &mut bool| {
        if lines.is_empty() {
            return;
        }
        let data = lines.join("\n");
        lines.clear();
        if data.trim() == "[DONE]" {
            if finish_reason.is_none() {
                *finish_reason = Some(Value::String("stop".to_string()));
            }
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            return;
        };
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "response.output_text.delta" {
            *saw_text_delta = true;
        }
        if matches!(
            event_type,
            "response.output_text.done"
                | "response.content_part.added"
                | "response.content_part.delta"
                | "response.content_part.done"
        ) {
            return;
        }
        if matches!(
            event_type,
            "response.output_item.added" | "response.output_item.done"
        ) {
            let is_function_call_item = value
                .get("item")
                .or_else(|| value.get("output_item"))
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|item_type| item_type == "function_call");
            if !is_function_call_item && *saw_text_delta {
                return;
            }
        }
        if value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "response.completed")
        {
            if let Some(response) = value.get("response") {
                *completed_response = Some(response.clone());
            }
        }
        let Some(chunk) = convert_openai_chat_stream_chunk(&value) else {
            return;
        };
        if let Some(v) = chunk.get("id").and_then(Value::as_str) {
            if !v.is_empty() {
                *id = v.to_string();
            }
        }
        if let Some(v) = chunk.get("model").and_then(Value::as_str) {
            if !v.is_empty() {
                *model = v.to_string();
            }
        }
        if let Some(v) = chunk.get("created").and_then(Value::as_i64) {
            *created = v;
        }
        if let Some(chunk_usage) = chunk.get("usage") {
            *usage = Some(chunk_usage.clone());
        }
        if let Some(choices) = chunk.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(delta) = choice.get("delta") {
                    if let Some(text_piece) = delta.get("content").and_then(Value::as_str) {
                        content.push_str(text_piece);
                    }
                }
                if let Some(reason) = choice.get("finish_reason") {
                    if !reason.is_null() {
                        *finish_reason = Some(reason.clone());
                    }
                }
            }
        }
    };

    for line in text.lines() {
        if line.starts_with("data:") {
            data_lines.push(line[5..].trim_start().to_string());
            continue;
        }
        if line.trim().is_empty() {
            flush_frame(
                &mut data_lines,
                &mut id,
                &mut model,
                &mut created,
                &mut content,
                &mut finish_reason,
                &mut usage,
                &mut completed_response,
                &mut saw_text_delta,
            );
        }
    }
    flush_frame(
        &mut data_lines,
        &mut id,
        &mut model,
        &mut created,
        &mut content,
        &mut finish_reason,
        &mut usage,
        &mut completed_response,
        &mut saw_text_delta,
    );

    if content.is_empty() {
        if let Some(response) = completed_response.as_ref() {
            let completion = map_openai_response_to_chat_completion(response);
            if let Some(v) = completion.get("id").and_then(Value::as_str) {
                if !v.is_empty() {
                    id = v.to_string();
                }
            }
            if let Some(v) = completion.get("model").and_then(Value::as_str) {
                if !v.is_empty() {
                    model = v.to_string();
                }
            }
            if let Some(v) = completion.get("created").and_then(Value::as_i64) {
                created = v;
            }
            if usage.is_none() {
                if let Some(v) = completion.get("usage") {
                    usage = Some(v.clone());
                }
            }
            if let Some(choice) = completion
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
            {
                if let Some(message) = choice.get("message") {
                    content = extract_chat_content_text(message.get("content"));
                }
                if finish_reason.is_none() {
                    if let Some(v) = choice.get("finish_reason") {
                        if !v.is_null() {
                            finish_reason = Some(v.clone());
                        }
                    }
                }
            }
        }
    }

    let mut out = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": finish_reason.unwrap_or(Value::String("stop".to_string()))
        }]
    });
    if let Some(usage) = usage {
        if let Some(out_obj) = out.as_object_mut() {
            out_obj.insert("usage".to_string(), usage);
        }
    }

    let bytes = serde_json::to_vec(&out)
        .map_err(|err| format!("serialize chat.completion json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

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
                .map(|value| {
                    value
                        .trim()
                        .to_ascii_lowercase()
                        .starts_with("application/json")
                })
                .unwrap_or(false);
            if is_json {
                let (anthropic_json, _) = convert_openai_json_to_anthropic(body)?;
                return convert_anthropic_json_to_sse(&anthropic_json);
            }
            convert_openai_sse_to_anthropic(body)
        }
        ResponseAdapter::OpenAIChatCompletionsJson | ResponseAdapter::OpenAIChatCompletionsSse => {
            if upstream_content_type.is_some_and(is_html_content_type) {
                return Err("upstream returned html challenge".to_string());
            }
            let is_sse = upstream_content_type
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            if is_sse || looks_like_sse_payload(body) {
                return convert_openai_sse_to_chat_completions_json(body);
            }
            convert_openai_json_to_chat_completions(body)
        }
        ResponseAdapter::OpenAICompletionsJson | ResponseAdapter::OpenAICompletionsSse => {
            if upstream_content_type.is_some_and(is_html_content_type) {
                return Err("upstream returned html challenge".to_string());
            }
            let is_sse = upstream_content_type
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            if is_sse || looks_like_sse_payload(body) {
                return convert_openai_sse_to_completions_json(body);
            }
            convert_openai_json_to_completions(body)
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
    .unwrap_or_else(|_| {
        b"{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"unknown error\"}}"
            .to_vec()
    })
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
