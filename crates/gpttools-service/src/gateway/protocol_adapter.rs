use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::gateway::request_helpers::is_html_content_type;

use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;

mod prompt_cache;

const DEFAULT_ANTHROPIC_MODEL: &str = "gpt-5.3-codex";
const DEFAULT_ANTHROPIC_REASONING: &str = "high";
const DEFAULT_ANTHROPIC_INSTRUCTIONS: &str =
    "You are Codex, a coding assistant that responds clearly and safely.";
const MAX_ANTHROPIC_TOOLS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponseAdapter {
    Passthrough,
    AnthropicJson,
    AnthropicSse,
}

#[derive(Debug)]
pub(super) struct AdaptedGatewayRequest {
    pub(super) path: String,
    pub(super) body: Vec<u8>,
    pub(super) response_adapter: ResponseAdapter,
}

pub(super) fn adapt_request_for_protocol(
    protocol_type: &str,
    path: &str,
    body: Vec<u8>,
) -> Result<AdaptedGatewayRequest, String> {
    if protocol_type != PROTOCOL_ANTHROPIC_NATIVE {
        return Ok(AdaptedGatewayRequest {
            path: path.to_string(),
            body,
            response_adapter: ResponseAdapter::Passthrough,
        });
    }

    if path == "/v1/messages" || path.starts_with("/v1/messages?") {
        let (adapted_body, request_stream) = convert_anthropic_messages_request(&body)?;
        // 说明：non-stream 也统一走 /v1/responses。
        // 在部分账号/环境下 /v1/responses/compact 更容易触发 challenge 或非预期拦截。
        let adapted_path = "/v1/responses".to_string();
        return Ok(AdaptedGatewayRequest {
            path: adapted_path,
            body: adapted_body,
            response_adapter: if request_stream {
                ResponseAdapter::AnthropicSse
            } else {
                ResponseAdapter::AnthropicJson
            },
        });
    }

    Ok(AdaptedGatewayRequest {
        path: path.to_string(),
        body,
        response_adapter: ResponseAdapter::Passthrough,
    })
}

fn convert_anthropic_messages_request(body: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let payload: Value =
        serde_json::from_slice(body).map_err(|_| "invalid claude request json".to_string())?;
    let Some(obj) = payload.as_object() else {
        return Err("claude request body must be an object".to_string());
    };

    let mut messages = Vec::new();

    if let Some(system) = obj.get("system") {
        let system_text = extract_text_content(system)?;
        if !system_text.trim().is_empty() {
            messages.push(json!({
                "role": "system",
                "content": system_text,
            }));
        }
    }

    let source_messages = obj
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "claude messages field is required".to_string())?;
    for message in source_messages {
        let Some(message_obj) = message.as_object() else {
            return Err("invalid claude message item".to_string());
        };
        let role = message_obj
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| "claude message role is required".to_string())?;
        let content = message_obj
            .get("content")
            .ok_or_else(|| "claude message content is required".to_string())?;
        match role {
            "assistant" => append_assistant_messages(&mut messages, content)?,
            "user" => append_user_messages(&mut messages, content)?,
            "tool" => append_tool_role_message(&mut messages, message_obj, content)?,
            other => return Err(format!("unsupported claude message role: {other}")),
        }
    }

    let (instructions, input_items) = convert_chat_messages_to_responses_input(&messages)?;
    let mut out = serde_json::Map::new();
    let resolved_model = resolve_anthropic_upstream_model(obj);
    out.insert("model".to_string(), Value::String(resolved_model));
    let resolved_instructions = instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_ANTHROPIC_INSTRUCTIONS);
    out.insert(
        "instructions".to_string(),
        Value::String(resolved_instructions.to_string()),
    );
    out.insert(
        "text".to_string(),
        json!({
            "format": {
                "type": "text",
            }
        }),
    );
    let resolved_reasoning = obj
        .get("reasoning")
        .and_then(Value::as_object)
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .unwrap_or(DEFAULT_ANTHROPIC_REASONING)
        .to_string();
    out.insert(
        "reasoning".to_string(),
        json!({
            "effort": resolved_reasoning,
        }),
    );
    out.insert("input".to_string(), Value::Array(input_items));

    // 中文注释：参考 CLIProxyAPI 的行为：Claude 入口需要一个稳定的 prompt_cache_key，
    // 并在上游请求头把 Session_id/Conversation_id 与之对齐，才能显著降低 challenge 命中率。
    if let Some(prompt_cache_key) = prompt_cache::resolve_prompt_cache_key(obj, out.get("model")) {
        out.insert(
            "prompt_cache_key".to_string(),
            Value::String(prompt_cache_key),
        );
    }
    // 中文注释：上游 codex responses 对低体积请求携带采样参数时更容易触发 challenge，
    // 这里对 anthropic 入口统一不透传 temperature/top_p，优先稳定性。
    if let Some(tools) = obj.get("tools").and_then(Value::as_array) {
        let mapped_tools = tools
            .iter()
            .filter_map(map_anthropic_tool_definition)
            .take(MAX_ANTHROPIC_TOOLS)
            .collect::<Vec<_>>();
        if !mapped_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(mapped_tools));
            if !obj.contains_key("tool_choice") {
                out.insert("tool_choice".to_string(), Value::String("auto".to_string()));
            }
        }
    }
    if let Some(tool_choice) = obj.get("tool_choice") {
        if !tool_choice.is_null() {
            if let Some(mapped_tool_choice) = map_anthropic_tool_choice(tool_choice) {
                out.insert("tool_choice".to_string(), mapped_tool_choice);
            }
        }
    }
    let request_stream = obj.get("stream").and_then(Value::as_bool).unwrap_or(true);
    // 说明：即使 Claude 请求 stream=false，也统一以 stream=true 请求 upstream，
    // 再在网关侧将 SSE 聚合为 Anthropic JSON，降低 upstream challenge 命中率。
    out.insert("stream".to_string(), Value::Bool(true));
    out.insert("parallel_tool_calls".to_string(), Value::Bool(true));
    out.insert("store".to_string(), Value::Bool(false));
    out.insert(
        "include".to_string(),
        Value::Array(vec![Value::String("reasoning.encrypted_content".to_string())]),
    );

    serde_json::to_vec(&Value::Object(out))
        .map(|bytes| (bytes, request_stream))
        .map_err(|err| format!("convert claude request failed: {err}"))
}

fn resolve_anthropic_upstream_model(source: &serde_json::Map<String, Value>) -> String {
    let requested_model = source
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match requested_model {
        Some(model) if model.to_ascii_lowercase().contains("codex") => model.to_string(),
        _ => DEFAULT_ANTHROPIC_MODEL.to_string(),
    }
}

fn append_assistant_messages(messages: &mut Vec<Value>, content: &Value) -> Result<(), String> {
    if let Some(text) = content.as_str() {
        messages.push(json!({
            "role": "assistant",
            "content": text,
        }));
        return Ok(());
    }

    let blocks = if let Some(array) = content.as_array() {
        array.to_vec()
    } else if content.is_object() {
        vec![content.clone()]
    } else {
        return Err("unsupported assistant content".to_string());
    };

    let mut text_content = String::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        let Some(block_obj) = block.as_object() else {
            return Err("invalid assistant content block".to_string());
        };
        let block_type = block_obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "assistant content block missing type".to_string())?;
        match block_type {
            "text" => {
                if let Some(text) = block_obj.get("text").and_then(Value::as_str) {
                    text_content.push_str(text);
                }
            }
            "tool_use" => {
                let id = block_obj
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("toolu_{}", tool_calls.len()));
                let Some(name) = block_obj
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    continue;
                };
                let input = block_obj.get("input").cloned().unwrap_or_else(|| json!({}));
                let arguments = serde_json::to_string(&input)
                    .map_err(|err| format!("serialize tool_use input failed: {err}"))?;
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }));
            }
            _ => continue,
        }
    }

    let mut message_obj = serde_json::Map::new();
    message_obj.insert("role".to_string(), Value::String("assistant".to_string()));
    message_obj.insert("content".to_string(), Value::String(text_content));
    if !tool_calls.is_empty() {
        message_obj.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    messages.push(Value::Object(message_obj));
    Ok(())
}

fn append_user_messages(messages: &mut Vec<Value>, content: &Value) -> Result<(), String> {
    if let Some(text) = content.as_str() {
        if !text.trim().is_empty() {
            messages.push(json!({
                "role": "user",
                "content": text,
            }));
        }
        return Ok(());
    }

    let blocks = if let Some(array) = content.as_array() {
        array.to_vec()
    } else if content.is_object() {
        vec![content.clone()]
    } else {
        return Err("unsupported user content".to_string());
    };

    let mut pending_text = String::new();
    for block in blocks {
        let Some(block_obj) = block.as_object() else {
            return Err("invalid user content block".to_string());
        };
        let block_type = block_obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "user content block missing type".to_string())?;
        match block_type {
            "text" => {
                if let Some(text) = block_obj.get("text").and_then(Value::as_str) {
                    pending_text.push_str(text);
                }
            }
            "tool_result" => {
                flush_user_text(messages, &mut pending_text);
                let tool_use_id = block_obj
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .or_else(|| block_obj.get("id").and_then(Value::as_str))
                    .unwrap_or_default();
                if tool_use_id.is_empty() {
                    continue;
                }
                let mut tool_content = extract_tool_result_content(block_obj.get("content"))?;
                if block_obj
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    tool_content = format!("[tool_error] {tool_content}");
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": tool_content,
                }));
            }
            _ => continue,
        }
    }
    flush_user_text(messages, &mut pending_text);
    Ok(())
}

fn append_tool_role_message(
    messages: &mut Vec<Value>,
    message_obj: &serde_json::Map<String, Value>,
    content: &Value,
) -> Result<(), String> {
    let tool_call_id = message_obj
        .get("tool_call_id")
        .or_else(|| message_obj.get("tool_use_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "tool role message missing tool_call_id".to_string())?;
    let tool_content = extract_tool_result_content(Some(content))?;
    messages.push(json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": tool_content,
    }));
    Ok(())
}

fn flush_user_text(messages: &mut Vec<Value>, pending_text: &mut String) {
    if pending_text.trim().is_empty() {
        pending_text.clear();
        return;
    }
    messages.push(json!({
        "role": "user",
        "content": pending_text.clone(),
    }));
    pending_text.clear();
}

fn convert_chat_messages_to_responses_input(
    messages: &[Value],
) -> Result<(Option<String>, Vec<Value>), String> {
    let mut instructions_parts = Vec::new();
    let mut input_items = Vec::new();

    for message in messages {
        let Some(message_obj) = message.as_object() else {
            continue;
        };
        let Some(role) = message_obj.get("role").and_then(Value::as_str) else {
            continue;
        };
        match role {
            "system" => {
                if let Some(content) = message_obj.get("content").and_then(Value::as_str) {
                    if !content.trim().is_empty() {
                        instructions_parts.push(content.to_string());
                    }
                }
            }
            "user" => {
                if let Some(content) = message_obj.get("content").and_then(Value::as_str) {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        input_items.push(json!({
                            "type": "message",
                            "role": "user",
                            "content": [{ "type": "input_text", "text": trimmed }]
                        }));
                    }
                }
            }
            "assistant" => {
                if let Some(content) = message_obj.get("content").and_then(Value::as_str) {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        input_items.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": trimmed }]
                        }));
                    }
                }
                if let Some(tool_calls) = message_obj.get("tool_calls").and_then(Value::as_array) {
                    for (index, tool_call) in tool_calls.iter().enumerate() {
                        let Some(tool_obj) = tool_call.as_object() else {
                            continue;
                        };
                        let call_id = tool_obj
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("call_{index}"));
                        let Some(function_name) = tool_obj
                            .get("function")
                            .and_then(|value| value.get("name"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        else {
                            continue;
                        };
                        let arguments = tool_obj
                            .get("function")
                            .and_then(|value| value.get("arguments"))
                            .map(|value| {
                                if let Some(text) = value.as_str() {
                                    text.to_string()
                                } else {
                                    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
                                }
                            })
                            .unwrap_or_else(|| "{}".to_string());
                        input_items.push(json!({
                            "type": "function_call",
                            "call_id": call_id,
                            "name": function_name,
                            "arguments": arguments
                        }));
                    }
                }
            }
            "tool" => {
                let call_id = message_obj
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "tool role message missing tool_call_id".to_string())?;
                let output = message_obj
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                input_items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }));
            }
            _ => {}
        }
    }

    let instructions = if instructions_parts.is_empty() {
        None
    } else {
        Some(instructions_parts.join("\n\n"))
    };
    Ok((instructions, input_items))
}

fn extract_tool_result_content(value: Option<&Value>) -> Result<String, String> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if value.is_null() {
        return Ok(String::new());
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(array) = value.as_array() {
        let mut out = String::new();
        for item in array {
            if let Some(text) = item.as_str() {
                out.push_str(text);
                continue;
            }
            if let Some(item_obj) = item.as_object() {
                let item_type = item_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if item_type == "text" {
                    if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                        continue;
                    }
                }
            }
            out.push_str(&serde_json::to_string(item).unwrap_or_else(|_| "".to_string()));
        }
        return Ok(out);
    }
    if let Some(item_obj) = value.as_object() {
        let item_type = item_obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if item_type == "text" {
            if let Some(text) = item_obj.get("text").and_then(Value::as_str) {
                return Ok(text.to_string());
            }
        }
    }
    serde_json::to_string(value).map_err(|err| format!("serialize tool_result content failed: {err}"))
}

fn map_anthropic_tool_definition(value: &Value) -> Option<Value> {
    let Some(obj) = value.as_object() else {
        return None;
    };
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| obj.get("type").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let parameters = obj
        .get("input_schema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));

    let mut tool_obj = serde_json::Map::new();
    tool_obj.insert("type".to_string(), Value::String("function".to_string()));
    tool_obj.insert("name".to_string(), Value::String(name.to_string()));
    if !description.is_empty() {
        tool_obj.insert("description".to_string(), Value::String(description));
    }
    tool_obj.insert("parameters".to_string(), parameters);

    Some(Value::Object(tool_obj))
}

fn map_anthropic_tool_choice(value: &Value) -> Option<Value> {
    if let Some(text) = value.as_str() {
        return Some(Value::String(text.to_string()));
    }
    let Some(obj) = value.as_object() else {
        return None;
    };
    let choice_type = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    match choice_type {
        "auto" => Some(Value::String("auto".to_string())),
        "any" => Some(Value::String("required".to_string())),
        "none" => Some(Value::String("none".to_string())),
        "tool" => {
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            Some(json!({
                "type": "function",
                "name": name
            }))
        }
        _ => None,
    }
}

fn extract_text_content(value: &Value) -> Result<String, String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }

    if let Some(block) = value.as_object() {
        return extract_text_from_block(block);
    }

    if let Some(array) = value.as_array() {
        let mut parts = Vec::new();
        for item in array {
            let Some(block) = item.as_object() else {
                return Err("invalid claude content block".to_string());
            };
            parts.push(extract_text_from_block(block)?);
        }
        return Ok(parts.join(""));
    }

    Err("unsupported claude content".to_string())
}

fn extract_text_from_block(block: &serde_json::Map<String, Value>) -> Result<String, String> {
    let block_type = block
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "claude content block missing type".to_string())?;
    if block_type != "text" {
        return Err(format!(
            "unsupported claude content block type: {block_type}"
        ));
    }
    block
        .get("text")
        .and_then(Value::as_str)
        .map(|v| v.to_string())
        .ok_or_else(|| "claude text block missing text".to_string())
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

fn convert_openai_json_to_anthropic(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
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
        .unwrap_or("msg_gpttools");

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
        .unwrap_or("msg_gpttools");

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

fn convert_anthropic_json_to_sse(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
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
        .unwrap_or("msg_gpttools");
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

fn convert_openai_sse_to_anthropic(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    let text = String::from_utf8(body.to_vec()).map_err(|_| "invalid upstream sse bytes".to_string())?;

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
    let response_id = response_id.unwrap_or_else(|| "msg_gpttools".to_string());
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

fn convert_anthropic_sse_to_json(body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    let text = String::from_utf8(body.to_vec()).map_err(|_| "invalid anthropic sse bytes".to_string())?;
    let mut current_event: Option<String> = None;
    let mut response_id = "msg_gpttools".to_string();
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
                        .unwrap_or("msg_gpttools")
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

fn parse_tool_arguments_as_object(raw: &str) -> Value {
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

fn to_tool_input_partial_json(value: &Value) -> Option<String> {
    let serialized = serde_json::to_string(value).ok()?;
    let trimmed = serialized.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    Some(trimmed.to_string())
}

fn extract_function_call_input_object(item_obj: &serde_json::Map<String, Value>) -> Value {
    let Some(arguments_raw) = extract_function_call_arguments_raw(item_obj) else {
        return json!({});
    };
    parse_tool_arguments_as_object(&arguments_raw)
}

fn extract_function_call_arguments_raw(item_obj: &serde_json::Map<String, Value>) -> Option<String> {
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

fn append_sse_event(buffer: &mut String, event_name: &str, payload: &Value) {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    buffer.push_str("event: ");
    buffer.push_str(event_name);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(&data);
    buffer.push_str("\n\n");
}

fn map_finish_reason(reason: &str) -> &'static str {
    match reason {
        "tool_calls" => "tool_use",
        "length" => "max_tokens",
        "stop" => "end_turn",
        _ => "end_turn",
    }
}
