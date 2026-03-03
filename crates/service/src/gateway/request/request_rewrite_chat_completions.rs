use serde_json::{json, Value};

use super::request_rewrite_shared::{
    path_matches_template, retain_fields_by_templates, TemplateAllowlist,
};

fn is_chat_completions_create_path(path: &str) -> bool {
    path_matches_template(path, "/v1/chat/completions")
}

fn is_stream_request(obj: &serde_json::Map<String, Value>) -> bool {
    obj.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

fn map_responses_role_to_chat(role: &str) -> &'static str {
    match role {
        "developer" => "system",
        "assistant" => "assistant",
        "tool" => "tool",
        _ => "user",
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        other => serde_json::to_string(other).ok(),
    }
}

fn flatten_responses_message_content(content: &Value) -> Option<Value> {
    match content {
        Value::String(text) => Some(Value::String(text.clone())),
        Value::Array(items) => {
            let mut text_parts = Vec::new();
            let mut multimodal_parts = Vec::new();
            for item in items {
                let Some(item_obj) = item.as_object() else {
                    continue;
                };
                let item_type = item_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match item_type {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
                            text_parts.push(text.to_string());
                        }
                    }
                    "input_image" => {
                        if let Some(image_url) = item_obj.get("image_url").and_then(Value::as_str) {
                            multimodal_parts.push(json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": image_url
                                }
                            }));
                        }
                    }
                    _ => {}
                }
            }
            if !multimodal_parts.is_empty() {
                if !text_parts.is_empty() {
                    multimodal_parts.insert(
                        0,
                        json!({
                            "type": "text",
                            "text": text_parts.join("\n")
                        }),
                    );
                }
                return Some(Value::Array(multimodal_parts));
            }
            if text_parts.is_empty() {
                None
            } else {
                Some(Value::String(text_parts.join("\n")))
            }
        }
        _ => None,
    }
}

fn convert_responses_input_item_to_chat_messages(item: &Value, out: &mut Vec<Value>) {
    let Some(item_obj) = item.as_object() else {
        return;
    };
    let item_type = item_obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match item_type {
        "function_call_output" => {
            let call_id = item_obj
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let output = item_obj
                .get("output")
                .and_then(value_to_string)
                .unwrap_or_default();
            if call_id.is_empty() && output.is_empty() {
                return;
            }
            out.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        "message" => {
            let role = item_obj
                .get("role")
                .and_then(Value::as_str)
                .map(map_responses_role_to_chat)
                .unwrap_or("user");
            let Some(content) = item_obj
                .get("content")
                .and_then(flatten_responses_message_content)
            else {
                return;
            };
            out.push(json!({
                "role": role,
                "content": content
            }));
        }
        _ => {
            if item_obj.get("role").is_some() && item_obj.get("content").is_some() {
                let role = item_obj
                    .get("role")
                    .and_then(Value::as_str)
                    .map(map_responses_role_to_chat)
                    .unwrap_or("user");
                if let Some(content) = item_obj
                    .get("content")
                    .and_then(flatten_responses_message_content)
                {
                    out.push(json!({
                        "role": role,
                        "content": content
                    }));
                }
            }
        }
    }
}

fn normalize_responses_tools_to_chat(obj: &mut serde_json::Map<String, Value>) -> bool {
    let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for tool in tools.iter_mut() {
        let Some(tool_obj) = tool.as_object_mut() else {
            continue;
        };
        let is_function = tool_obj
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| kind == "function")
            .unwrap_or(false);
        if !is_function || tool_obj.contains_key("function") {
            continue;
        }
        let mut fn_obj = serde_json::Map::new();
        if let Some(name) = tool_obj.remove("name") {
            fn_obj.insert("name".to_string(), name);
        }
        if let Some(description) = tool_obj.remove("description") {
            fn_obj.insert("description".to_string(), description);
        }
        if let Some(parameters) = tool_obj.remove("parameters") {
            fn_obj.insert("parameters".to_string(), parameters);
        }
        if let Some(strict) = tool_obj.remove("strict") {
            fn_obj.insert("strict".to_string(), strict);
        }
        if fn_obj.is_empty() {
            continue;
        }
        tool_obj.insert("function".to_string(), Value::Object(fn_obj));
        changed = true;
    }
    changed
}

fn normalize_responses_tool_choice_to_chat(obj: &mut serde_json::Map<String, Value>) -> bool {
    let Some(tool_choice_obj) = obj.get_mut("tool_choice").and_then(Value::as_object_mut) else {
        return false;
    };
    let is_function = tool_choice_obj
        .get("type")
        .and_then(Value::as_str)
        .map(|kind| kind == "function")
        .unwrap_or(false);
    if !is_function || tool_choice_obj.contains_key("function") {
        return false;
    }
    let Some(name) = tool_choice_obj.remove("name") else {
        return false;
    };
    tool_choice_obj.insert("function".to_string(), json!({ "name": name }));
    true
}

pub(super) fn normalize_responses_payload(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_chat_completions_create_path(path) || obj.contains_key("messages") {
        return false;
    }
    let mut changed = false;
    let mut messages = Vec::<Value>::new();

    if let Some(instructions) = obj
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({
            "role": "system",
            "content": instructions
        }));
    }

    if let Some(input) = obj.get("input") {
        match input {
            Value::String(text) => {
                if !text.trim().is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": text
                    }));
                }
            }
            Value::Array(items) => {
                for item in items {
                    convert_responses_input_item_to_chat_messages(item, &mut messages);
                }
            }
            Value::Object(_) => {
                convert_responses_input_item_to_chat_messages(input, &mut messages);
            }
            _ => {}
        }
    }

    if !messages.is_empty() {
        obj.insert("messages".to_string(), Value::Array(messages));
        changed = true;
    }
    if obj.remove("instructions").is_some() {
        changed = true;
    }
    if obj.remove("input").is_some() {
        changed = true;
    }
    if normalize_responses_tools_to_chat(obj) {
        changed = true;
    }
    if normalize_responses_tool_choice_to_chat(obj) {
        changed = true;
    }
    changed
}

pub(super) fn ensure_stream_usage_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_chat_completions_create_path(path) {
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

pub(super) fn ensure_reasoning_effort(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_chat_completions_create_path(path) {
        return false;
    }

    let mut changed = false;
    if !obj.contains_key("reasoning_effort") {
        let effort = obj
            .get("reasoning")
            .and_then(|v| v.get("effort"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(effort) = effort {
            obj.insert("reasoning_effort".to_string(), Value::String(effort));
            changed = true;
        }
    }
    if obj.remove("reasoning").is_some() {
        changed = true;
    }
    changed
}

pub(super) fn apply_reasoning_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
    reasoning_effort: Option<&str>,
) -> bool {
    if !is_chat_completions_create_path(path) {
        return false;
    }
    let Some(level) = reasoning_effort else {
        return false;
    };
    obj.insert(
        "reasoning_effort".to_string(),
        Value::String(level.to_string()),
    );
    true
}

fn is_supported_openai_chat_completions_create_key(key: &str) -> bool {
    matches!(
        key,
        "messages"
            | "model"
            | "audio"
            | "frequency_penalty"
            | "function_call"
            | "functions"
            | "logit_bias"
            | "logprobs"
            | "max_completion_tokens"
            | "max_tokens"
            | "metadata"
            | "modalities"
            | "n"
            | "parallel_tool_calls"
            | "prediction"
            | "presence_penalty"
            | "reasoning_effort"
            | "response_format"
            | "seed"
            | "service_tier"
            | "stop"
            | "store"
            | "stream"
            | "stream_options"
            | "temperature"
            | "tool_choice"
            | "tools"
            | "top_logprobs"
            | "top_p"
            | "user"
            | "web_search_options"
    )
}

fn is_supported_openai_chat_completions_metadata_update_key(key: &str) -> bool {
    matches!(key, "metadata")
}

const CHAT_COMPLETIONS_ALLOWLISTS: &[TemplateAllowlist] = &[
    TemplateAllowlist {
        template: "/v1/chat/completions",
        allow: is_supported_openai_chat_completions_create_key,
    },
    TemplateAllowlist {
        template: "/v1/chat/completions/{completion_id}",
        allow: is_supported_openai_chat_completions_metadata_update_key,
    },
];

pub(super) fn retain_official_fields(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> Vec<String> {
    retain_fields_by_templates(path, obj, CHAT_COMPLETIONS_ALLOWLISTS)
}
