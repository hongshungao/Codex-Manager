use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Cursor, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tiny_http::{Header, Request, Response, StatusCode};

use super::AccountInFlightGuard;

// Env:
// - CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES (default: 131072; 0 disables limit)
// Caps accumulated `output_text` extracted from upstream responses to avoid unbounded memory growth.
const OUTPUT_TEXT_LIMIT_BYTES_ENV: &str = "CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES";
const DEFAULT_OUTPUT_TEXT_LIMIT_BYTES: usize = 128 * 1024;
const OUTPUT_TEXT_TRUNCATED_MARKER: &str = "[output_text truncated]";
static OUTPUT_TEXT_LIMIT_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_OUTPUT_TEXT_LIMIT_BYTES);
static OUTPUT_TEXT_LIMIT_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub(super) struct UpstreamResponseUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub output_text: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct UpstreamResponseBridgeResult {
    pub usage: UpstreamResponseUsage,
    // For streaming responses: whether we observed a terminal marker such as `data: [DONE]`
    // or `type/event: response.completed|response.failed`.
    pub stream_terminal_seen: bool,
    // For streaming responses: terminal error message (e.g. `response.failed` or error payload).
    pub stream_terminal_error: Option<String>,
    // Any IO error while writing the response back to the downstream client.
    pub delivery_error: Option<String>,
    // Optional upstream error hint parsed from non-stream error bodies.
    pub upstream_error_hint: Option<String>,
}

impl UpstreamResponseBridgeResult {
    pub(super) fn is_ok(&self, is_stream: bool) -> bool {
        if self.delivery_error.is_some() {
            return false;
        }
        if is_stream {
            if !self.stream_terminal_seen {
                return false;
            }
            if self.stream_terminal_error.is_some() {
                return false;
            }
        }
        true
    }

    pub(super) fn error_message(&self, is_stream: bool) -> Option<String> {
        if let Some(err) = self.stream_terminal_error.as_ref() {
            return Some(err.clone());
        }
        if is_stream && !self.stream_terminal_seen {
            return Some("stream disconnected before completion".to_string());
        }
        if let Some(err) = self.delivery_error.as_ref() {
            return Some(format!("response write failed: {err}"));
        }
        None
    }
}

fn merge_usage(target: &mut UpstreamResponseUsage, source: UpstreamResponseUsage) {
    if source.input_tokens.is_some() {
        target.input_tokens = source.input_tokens;
    }
    if source.cached_input_tokens.is_some() {
        target.cached_input_tokens = source.cached_input_tokens;
    }
    if source.output_tokens.is_some() {
        target.output_tokens = source.output_tokens;
    }
    if source.total_tokens.is_some() {
        target.total_tokens = source.total_tokens;
    }
    if source.reasoning_output_tokens.is_some() {
        target.reasoning_output_tokens = source.reasoning_output_tokens;
    }
    if let Some(source_text) = source.output_text {
        let target_text = target.output_text.get_or_insert_with(String::new);
        append_output_text_raw(target_text, source_text.as_str());
    }
}

fn usage_has_signal(usage: &UpstreamResponseUsage) -> bool {
    usage.input_tokens.is_some()
        || usage.cached_input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.reasoning_output_tokens.is_some()
        || usage
            .output_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
}

fn parse_usage_from_object(usage: Option<&Map<String, Value>>) -> UpstreamResponseUsage {
    let input_tokens = usage
        .and_then(|map| map.get("input_tokens").and_then(Value::as_i64))
        .or_else(|| usage.and_then(|map| map.get("prompt_tokens").and_then(Value::as_i64)));
    let output_tokens = usage
        .and_then(|map| map.get("output_tokens").and_then(Value::as_i64))
        .or_else(|| usage.and_then(|map| map.get("completion_tokens").and_then(Value::as_i64)));
    let total_tokens = usage.and_then(|map| map.get("total_tokens").and_then(Value::as_i64));
    let cached_input_tokens = usage
        .and_then(|map| map.get("input_tokens_details"))
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .and_then(|map| map.get("prompt_tokens_details"))
                .and_then(Value::as_object)
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        });
    let reasoning_output_tokens = usage
        .and_then(|map| map.get("output_tokens_details"))
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .and_then(|map| map.get("completion_tokens_details"))
                .and_then(Value::as_object)
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_i64)
        });
    UpstreamResponseUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens,
        reasoning_output_tokens,
        output_text: None,
    }
}

fn append_output_text(buffer: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    let limit = output_text_limit_bytes();
    if limit > 0 && buffer.len() >= limit {
        mark_output_text_truncated(buffer, limit);
        return;
    }
    if !buffer.is_empty() {
        if limit > 0 && buffer.len() + 1 > limit {
            mark_output_text_truncated(buffer, limit);
            return;
        }
        buffer.push('\n');
    }
    if limit == 0 {
        buffer.push_str(text);
        return;
    }
    let remaining = limit.saturating_sub(buffer.len());
    if remaining == 0 {
        mark_output_text_truncated(buffer, limit);
        return;
    }
    let slice = truncate_str_to_bytes(text, remaining);
    buffer.push_str(slice);
    if slice.len() < text.len() {
        mark_output_text_truncated(buffer, limit);
    }
}

fn append_output_text_raw(buffer: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    let limit = output_text_limit_bytes();
    if limit > 0 && buffer.len() >= limit {
        mark_output_text_truncated(buffer, limit);
        return;
    }
    if limit == 0 {
        buffer.push_str(text);
        return;
    }
    let remaining = limit.saturating_sub(buffer.len());
    if remaining == 0 {
        mark_output_text_truncated(buffer, limit);
        return;
    }
    let slice = truncate_str_to_bytes(text, remaining);
    buffer.push_str(slice);
    if slice.len() < text.len() {
        mark_output_text_truncated(buffer, limit);
    }
}

fn collect_response_output_text(value: &Value, output: &mut String) {
    match value {
        Value::String(text) => append_output_text(output, text),
        Value::Array(items) => {
            for item in items {
                collect_response_output_text(item, output);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("output_text").and_then(Value::as_str) {
                append_output_text(output, text);
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                append_output_text(output, text);
            }
            if let Some(content) = map.get("content") {
                collect_response_output_text(content, output);
            }
            if let Some(message) = map.get("message") {
                collect_response_output_text(message, output);
            }
            if let Some(output_field) = map.get("output") {
                collect_response_output_text(output_field, output);
            }
            if let Some(delta) = map.get("delta") {
                collect_response_output_text(delta, output);
            }
        }
        _ => {}
    }
}

fn output_text_limit_bytes() -> usize {
    let _ = OUTPUT_TEXT_LIMIT_LOADED.get_or_init(|| {
        let raw = std::env::var(OUTPUT_TEXT_LIMIT_BYTES_ENV).unwrap_or_default();
        let limit = raw
            .trim()
            .parse::<usize>()
            .unwrap_or(DEFAULT_OUTPUT_TEXT_LIMIT_BYTES);
        OUTPUT_TEXT_LIMIT_BYTES.store(limit, Ordering::Relaxed);
    });
    OUTPUT_TEXT_LIMIT_BYTES.load(Ordering::Relaxed)
}

fn truncate_str_to_bytes(text: &str, max_bytes: usize) -> &str {
    if max_bytes >= text.len() {
        return text;
    }
    let mut idx = max_bytes;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    &text[..idx]
}

fn truncate_string_to_bytes(value: &mut String, max_bytes: usize) {
    if max_bytes >= value.len() {
        return;
    }
    let mut idx = max_bytes;
    while idx > 0 && !value.is_char_boundary(idx) {
        idx -= 1;
    }
    value.truncate(idx);
}

fn mark_output_text_truncated(buffer: &mut String, limit: usize) {
    if limit == 0 {
        return;
    }
    if buffer.ends_with(OUTPUT_TEXT_TRUNCATED_MARKER) {
        return;
    }
    // Try to append marker directly when possible.
    let newline_bytes = if buffer.is_empty() { 0 } else { 1 };
    let marker_bytes = OUTPUT_TEXT_TRUNCATED_MARKER.len();
    if buffer.len() + newline_bytes + marker_bytes <= limit {
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(OUTPUT_TEXT_TRUNCATED_MARKER);
        return;
    }
    // Otherwise, shrink buffer to make room for marker.
    if limit <= marker_bytes {
        truncate_string_to_bytes(buffer, limit);
        return;
    }
    let target = limit.saturating_sub(marker_bytes + newline_bytes);
    truncate_string_to_bytes(buffer, target);
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(OUTPUT_TEXT_TRUNCATED_MARKER);
}

fn collect_output_text_from_event_fields(value: &Value, output: &mut String) {
    if let Some(item) = value.get("item") {
        collect_response_output_text(item, output);
    }
    if let Some(output_item) = value.get("output_item") {
        collect_response_output_text(output_item, output);
    }
    if let Some(part) = value.get("part") {
        collect_response_output_text(part, output);
    }
    if let Some(content_part) = value.get("content_part") {
        collect_response_output_text(content_part, output);
    }
}

fn extract_output_text_from_json(value: &Value) -> Option<String> {
    let mut output = String::new();
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        append_output_text(&mut output, text);
    }
    if let Some(response) = value.get("response") {
        collect_response_output_text(response, &mut output);
    }
    if let Some(top_level_output) = value.get("output") {
        collect_response_output_text(top_level_output, &mut output);
    }
    if let Some(choices) = value.get("choices") {
        collect_response_output_text(choices, &mut output);
    }
    if let Some(item) = value.get("item") {
        collect_response_output_text(item, &mut output);
    }
    if let Some(part) = value.get("part") {
        collect_response_output_text(part, &mut output);
    }
    if output.trim().is_empty() {
        None
    } else {
        Some(output)
    }
}

fn parse_usage_from_json(value: &Value) -> UpstreamResponseUsage {
    let mut usage = parse_usage_from_object(value.get("usage").and_then(Value::as_object));
    let response_usage = value
        .get("response")
        .and_then(|response| response.get("usage"))
        .and_then(Value::as_object);
    merge_usage(&mut usage, parse_usage_from_object(response_usage));
    usage.output_text = extract_output_text_from_json(value);
    usage
}

#[cfg(test)]
fn parse_usage_from_sse_frame(lines: &[String]) -> Option<UpstreamResponseUsage> {
    let mut data_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return None;
    }
    let value = serde_json::from_str::<Value>(&data).ok()?;
    let mut usage = parse_usage_from_json(&value);
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        let mut text_out = String::new();
        for choice in choices {
            if let Some(delta) = choice
                .get("delta")
                .and_then(Value::as_object)
                .and_then(|delta| delta.get("content"))
            {
                collect_response_output_text(delta, &mut text_out);
            }
        }
        if !text_out.trim().is_empty() {
            let target = usage.output_text.get_or_insert_with(String::new);
            append_output_text(target, text_out.as_str());
        }
        return Some(usage);
    }
    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
        if !delta.is_empty() {
            let target = usage.output_text.get_or_insert_with(String::new);
            append_output_text(target, delta);
        }
        return Some(usage);
    }
    Some(usage)
}

#[derive(Debug, Clone)]
enum SseTerminal {
    Ok,
    Err(String),
}

#[derive(Debug, Clone, Default)]
struct SseFrameInspection {
    saw_data: bool,
    usage: Option<UpstreamResponseUsage>,
    terminal: Option<SseTerminal>,
}

fn classify_terminal_event_name(name: &str) -> Option<SseTerminal> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized == "done"
        || normalized == "response.completed"
        || normalized.ends_with(".completed")
    {
        return Some(SseTerminal::Ok);
    }
    if normalized == "error"
        || normalized == "response.failed"
        || normalized.ends_with(".failed")
        || normalized.ends_with(".error")
        || normalized.ends_with(".canceled")
        || normalized.ends_with(".cancelled")
        || normalized.ends_with(".incomplete")
    {
        return Some(SseTerminal::Err(normalized));
    }
    None
}

fn is_chat_completion_terminal_chunk(value: &Value) -> bool {
    if value.get("object").and_then(Value::as_str) != Some("chat.completion.chunk") {
        return false;
    }
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("finish_reason")
                    .is_some_and(|finish_reason| !finish_reason.is_null())
            })
        })
}

fn extract_error_message_from_json(value: &Value) -> Option<String> {
    fn extract_message_from_error_map(err_obj: &Map<String, Value>) -> Option<String> {
        let message = err_obj
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| err_obj.get("error").and_then(Value::as_str))
            .map(str::trim)
            .filter(|msg| !msg.is_empty());
        let code = err_obj
            .get("code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let kind = err_obj
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let param = err_obj
            .get("param")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty());

        if let Some(message) = message {
            let mut prefixes = Vec::new();
            if let Some(code) = code {
                prefixes.push(format!("code={code}"));
            }
            if let Some(kind) = kind {
                prefixes.push(format!("type={kind}"));
            }
            if let Some(param) = param {
                prefixes.push(format!("param={param}"));
            }
            return if prefixes.is_empty() {
                Some(message.to_string())
            } else {
                Some(format!("{} {}", prefixes.join(" "), message))
            };
        }

        // Fall back to compact JSON if needed.
        serde_json::to_string(err_obj)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    fn extract_message_from_error_value(err_value: Option<&Value>) -> Option<String> {
        let err_value = err_value?;
        if let Some(message) = err_value.as_str() {
            let msg = message.trim();
            if !msg.is_empty() {
                return Some(msg.to_string());
            }
            return None;
        }
        if let Some(err_obj) = err_value.as_object() {
            return extract_message_from_error_map(err_obj);
        }
        None
    }

    // OpenAI style: { "error": { "message": "..." } }
    if let Some(message) = extract_message_from_error_value(value.get("error")) {
        return Some(message);
    }
    // Responses API streaming often nests the error under response.error for `response.failed`.
    if let Some(message) = extract_message_from_error_value(value.pointer("/response/error")) {
        return Some(message);
    }
    // Some providers nest details under response.status_details.error.
    if let Some(message) =
        extract_message_from_error_value(value.pointer("/response/status_details/error"))
    {
        return Some(message);
    }
    // Some providers emit: { "type": "error", "message": "..." }
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("error"))
    {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            let msg = message.trim();
            if !msg.is_empty() {
                return Some(msg.to_string());
            }
        }
    }
    None
}

fn extract_error_hint_from_body(status_code: u16, body: &[u8]) -> Option<String> {
    if status_code < 400 || body.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = extract_error_message_from_json(&value) {
            return Some(message);
        }
    }
    std::str::from_utf8(body)
        .ok()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| {
            let mut chars = text.chars();
            let snippet = chars.by_ref().take(240).collect::<String>();
            if chars.next().is_some() {
                format!("{snippet}...")
            } else {
                snippet
            }
        })
}

fn inspect_sse_frame(lines: &[String]) -> SseFrameInspection {
    let mut inspection = SseFrameInspection::default();
    let mut data_lines = Vec::new();
    let mut event_name: Option<String> = None;

    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("event:") {
            if event_name.is_none() {
                let v = rest.trim();
                if !v.is_empty() {
                    event_name = Some(v.to_string());
                }
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            inspection.saw_data = true;
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if let Some(name) = event_name.as_deref() {
        inspection.terminal = classify_terminal_event_name(name);
    }

    if data_lines.is_empty() {
        return inspection;
    }

    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        inspection.terminal = Some(SseTerminal::Ok);
        return inspection;
    }

    if let Ok(value) = serde_json::from_str::<Value>(&data) {
        if let Some(message) = extract_error_message_from_json(&value) {
            inspection.terminal = Some(SseTerminal::Err(message));
        } else if let Some(kind) = value.get("type").and_then(Value::as_str) {
            // OpenAI Responses API streaming uses `type` as event name.
            if let Some(terminal) = classify_terminal_event_name(kind) {
                inspection.terminal = Some(terminal);
            }
        } else if is_chat_completion_terminal_chunk(&value) {
            // OpenAI Chat Completions streaming compatibility:
            // some upstreams omit `data: [DONE]` but emit final chunk with `finish_reason`.
            inspection.terminal = Some(SseTerminal::Ok);
        }

        // Always attempt to parse usage; some terminal/error frames also include usage.
        inspection.usage = parse_usage_from_json(&value).into();
        // For compatibility with chat-completions delta frames, augment output_text like the existing parser does.
        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            let mut text_out = String::new();
            for choice in choices {
                if let Some(delta) = choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("content"))
                {
                    collect_response_output_text(delta, &mut text_out);
                }
            }
            if !text_out.trim().is_empty() {
                let usage = inspection
                    .usage
                    .get_or_insert_with(UpstreamResponseUsage::default);
                let target = usage.output_text.get_or_insert_with(String::new);
                append_output_text(target, text_out.as_str());
            }
        } else if let Some(delta) = value.get("delta").and_then(Value::as_str) {
            if !delta.is_empty() {
                let usage = inspection
                    .usage
                    .get_or_insert_with(UpstreamResponseUsage::default);
                let target = usage.output_text.get_or_insert_with(String::new);
                append_output_text(target, delta);
            }
        }
    }

    inspection
}

#[derive(Debug, Clone, Default)]
struct ChatCompletionChoiceSynthesis {
    role: Option<String>,
    content: String,
    finish_reason: Option<Value>,
}

#[derive(Debug, Clone, Default)]
struct ChatCompletionSseSynthesis {
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    system_fingerprint: Option<Value>,
    usage: Option<Value>,
    choices: BTreeMap<i64, ChatCompletionChoiceSynthesis>,
    saw_terminal: bool,
}

#[derive(Debug, Clone, Default)]
struct ResponsesSseSynthesis {
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    usage: Option<Value>,
    output_text: String,
    saw_completed: bool,
}

fn append_chat_delta_content(buffer: &mut String, delta_content: &Value) {
    if let Some(text) = delta_content.as_str() {
        buffer.push_str(text);
        return;
    }
    let Some(parts) = delta_content.as_array() else {
        return;
    };
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            buffer.push_str(text);
        }
    }
}

fn update_chat_completion_sse_synthesis(synthesis: &mut ChatCompletionSseSynthesis, value: &Value) {
    if value.get("object").and_then(Value::as_str) != Some("chat.completion.chunk") {
        return;
    }
    if synthesis.id.is_none() {
        synthesis.id = value
            .get("id")
            .and_then(Value::as_str)
            .map(|v| v.to_string());
    }
    if synthesis.model.is_none() {
        synthesis.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(|v| v.to_string());
    }
    if synthesis.created.is_none() {
        synthesis.created = value.get("created").and_then(Value::as_i64);
    }
    if synthesis.system_fingerprint.is_none() {
        synthesis.system_fingerprint = value.get("system_fingerprint").cloned();
    }
    if let Some(usage) = value.get("usage") {
        synthesis.usage = Some(usage.clone());
    }

    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for (position, choice) in choices.iter().enumerate() {
        let index = choice
            .get("index")
            .and_then(Value::as_i64)
            .unwrap_or(position as i64);
        let target = synthesis.choices.entry(index).or_default();
        if target.role.is_none() {
            target.role = choice
                .get("delta")
                .and_then(|delta| delta.get("role"))
                .and_then(Value::as_str)
                .map(|v| v.to_string());
        }
        if let Some(delta_content) = choice.get("delta").and_then(|delta| delta.get("content")) {
            append_chat_delta_content(&mut target.content, delta_content);
        }
        if let Some(finish_reason) = choice.get("finish_reason") {
            if !finish_reason.is_null() {
                target.finish_reason = Some(finish_reason.clone());
                synthesis.saw_terminal = true;
            }
        }
    }
}

fn update_responses_sse_synthesis(synthesis: &mut ResponsesSseSynthesis, value: &Value) {
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };

    if synthesis.id.is_none() {
        synthesis.id = value
            .get("response_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_string));
    }
    if synthesis.model.is_none() {
        synthesis.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("model"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
    }
    if synthesis.created.is_none() {
        synthesis.created = value
            .get("created")
            .and_then(Value::as_i64)
            .or_else(|| value.get("created_at").and_then(Value::as_i64))
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("created"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("created_at"))
                    .and_then(Value::as_i64)
            });
    }

    if let Some(response_usage) = value
        .get("response")
        .and_then(|response| response.get("usage"))
        .cloned()
    {
        synthesis.usage = Some(response_usage);
    } else if synthesis.usage.is_none() {
        if let Some(usage) = value.get("usage").cloned() {
            synthesis.usage = Some(usage);
        }
    }

    let mut text_out = String::new();
    collect_output_text_from_event_fields(value, &mut text_out);
    if let Some(delta) = value.get("delta") {
        collect_response_output_text(delta, &mut text_out);
    }
    if let Some(response) = value.get("response") {
        collect_response_output_text(response, &mut text_out);
    }
    if !text_out.trim().is_empty() {
        append_output_text_raw(&mut synthesis.output_text, text_out.as_str());
    }

    if event_type == "response.completed" {
        synthesis.saw_completed = true;
    }
}

fn response_has_effective_output(response: &Value) -> bool {
    let mut output_text = String::new();
    if let Some(output) = response.get("output") {
        collect_response_output_text(output, &mut output_text);
    }
    !output_text.trim().is_empty()
}

fn build_response_output_items_from_text(text: &str) -> Value {
    Value::Array(vec![json!({
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": text
        }]
    })])
}

fn enrich_completed_response_with_sse_text(
    completed_response: Value,
    synthesis: &ResponsesSseSynthesis,
) -> Value {
    let mut response = completed_response;
    let Some(response_obj) = response.as_object_mut() else {
        return response;
    };

    if response_obj
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|id| id.is_empty())
    {
        if let Some(id) = synthesis.id.as_ref() {
            response_obj.insert("id".to_string(), Value::String(id.clone()));
        }
    }
    if response_obj
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.is_empty())
    {
        if let Some(model) = synthesis.model.as_ref() {
            response_obj.insert("model".to_string(), Value::String(model.clone()));
        }
    }
    if response_obj
        .get("created")
        .and_then(Value::as_i64)
        .is_none()
    {
        if let Some(created) = synthesis.created {
            response_obj.insert("created".to_string(), Value::Number(created.into()));
        }
    }
    if !response_obj.contains_key("object") {
        response_obj.insert("object".to_string(), Value::String("response".to_string()));
    }
    if !response_obj.contains_key("status") {
        response_obj.insert("status".to_string(), Value::String("completed".to_string()));
    }

    if response_obj.get("usage").is_none() {
        if let Some(usage) = synthesis.usage.as_ref() {
            response_obj.insert("usage".to_string(), usage.clone());
        }
    }

    let has_effective_output = response_has_effective_output(&Value::Object(response_obj.clone()));
    if !has_effective_output && !synthesis.output_text.trim().is_empty() {
        response_obj.insert(
            "output".to_string(),
            build_response_output_items_from_text(synthesis.output_text.as_str()),
        );
    }
    if response_obj
        .get("output_text")
        .and_then(Value::as_str)
        .is_none_or(|text| text.trim().is_empty())
        && !synthesis.output_text.trim().is_empty()
    {
        response_obj.insert(
            "output_text".to_string(),
            Value::String(synthesis.output_text.trim().to_string()),
        );
    }

    Value::Object(response_obj.clone())
}

fn synthesize_response_body_from_sse(synthesis: &ResponsesSseSynthesis) -> Option<Vec<u8>> {
    if !synthesis.saw_completed || synthesis.output_text.trim().is_empty() {
        return None;
    }
    let created = synthesis.created.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    });
    let mut out = serde_json::Map::new();
    out.insert(
        "id".to_string(),
        Value::String(
            synthesis
                .id
                .clone()
                .unwrap_or_else(|| "resp_proxy".to_string()),
        ),
    );
    out.insert("object".to_string(), Value::String("response".to_string()));
    out.insert("created".to_string(), Value::Number(created.into()));
    out.insert(
        "model".to_string(),
        Value::String(
            synthesis
                .model
                .clone()
                .unwrap_or_else(|| "gpt-5.3-codex".to_string()),
        ),
    );
    out.insert("status".to_string(), Value::String("completed".to_string()));
    out.insert(
        "output".to_string(),
        build_response_output_items_from_text(synthesis.output_text.trim()),
    );
    out.insert(
        "output_text".to_string(),
        Value::String(synthesis.output_text.trim().to_string()),
    );
    if let Some(usage) = synthesis.usage.clone() {
        out.insert("usage".to_string(), usage);
    }
    serde_json::to_vec(&Value::Object(out)).ok()
}

fn synthesize_chat_completion_body(synthesis: &ChatCompletionSseSynthesis) -> Option<Vec<u8>> {
    if !synthesis.saw_terminal || synthesis.choices.is_empty() {
        return None;
    }
    let created = synthesis.created.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    });

    let choices = synthesis
        .choices
        .iter()
        .map(|(index, choice)| {
            json!({
                "index": index,
                "message": {
                    "role": choice.role.clone().unwrap_or_else(|| "assistant".to_string()),
                    "content": choice.content,
                },
                "finish_reason": choice.finish_reason.clone().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let mut out = serde_json::Map::new();
    out.insert(
        "id".to_string(),
        Value::String(
            synthesis
                .id
                .clone()
                .unwrap_or_else(|| "chatcmpl_proxy".to_string()),
        ),
    );
    out.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    out.insert("created".to_string(), Value::Number(created.into()));
    out.insert(
        "model".to_string(),
        Value::String(
            synthesis
                .model
                .clone()
                .unwrap_or_else(|| "gpt-5.3-codex".to_string()),
        ),
    );
    out.insert("choices".to_string(), Value::Array(choices));
    if let Some(system_fingerprint) = synthesis.system_fingerprint.clone() {
        out.insert("system_fingerprint".to_string(), system_fingerprint);
    }
    if let Some(usage) = synthesis.usage.clone() {
        out.insert("usage".to_string(), usage);
    }
    serde_json::to_vec(&Value::Object(out)).ok()
}

fn extract_sse_event_name(lines: &[String]) -> Option<String> {
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("event:") {
            let event_name = rest.trim();
            if !event_name.is_empty() {
                return Some(event_name.to_string());
            }
        }
    }
    None
}

fn normalize_sse_event_name_for_type(event_name: &str) -> Option<&str> {
    let normalized = event_name.trim();
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("message") {
        return None;
    }
    Some(normalized)
}

fn extract_sse_frame_payload(lines: &[String]) -> Option<String> {
    let mut data_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if !data_lines.is_empty() {
        return Some(data_lines.join("\n"));
    }

    // 兼容非标准上游：有些网关会返回 JSONL（无 `data:` 前缀）。
    let mut raw_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(':')
            || trimmed.starts_with("event:")
            || trimmed.starts_with("id:")
            || trimmed.starts_with("retry:")
        {
            continue;
        }
        raw_lines.push(trimmed.to_string());
    }
    if raw_lines.is_empty() {
        return None;
    }
    Some(raw_lines.join("\n"))
}

fn ensure_value_has_sse_event_type(lines: &[String], value: &mut Value) {
    let Some(event_name) = extract_sse_event_name(lines) else {
        return;
    };
    let Some(event_type) = normalize_sse_event_name_for_type(event_name.as_str()) else {
        return;
    };
    let needs_type = value
        .get("type")
        .and_then(Value::as_str)
        .map(|kind| kind.trim().is_empty())
        .unwrap_or(true);
    if !needs_type {
        return;
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("type".to_string(), Value::String(event_type.to_string()));
    }
}

fn parse_sse_frame_json(lines: &[String]) -> Option<Value> {
    let data = extract_sse_frame_payload(lines)?;
    if data.trim() == "[DONE]" {
        return None;
    }
    let mut value = serde_json::from_str::<Value>(&data).ok()?;
    ensure_value_has_sse_event_type(lines, &mut value);
    Some(value)
}

fn collect_non_stream_json_from_sse_bytes(
    payload: &[u8],
) -> (Option<Vec<u8>>, UpstreamResponseUsage) {
    let mut usage = UpstreamResponseUsage::default();
    let mut completed_response: Option<Value> = None;
    let mut responses_sse_synthesis = ResponsesSseSynthesis::default();
    let mut chat_completion_synthesis = ChatCompletionSseSynthesis::default();
    let mut frame_lines: Vec<String> = Vec::new();

    let mut reader = BufReader::new(Cursor::new(payload));
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(read) = reader.read_line(&mut line) else {
            break;
        };
        if read == 0 {
            break;
        }
        if line == "\n" || line == "\r\n" {
            if frame_lines.is_empty() {
                continue;
            }
            let frame = std::mem::take(&mut frame_lines);
            let inspection = inspect_sse_frame(&frame);
            if let Some(parsed_usage) = inspection.usage {
                merge_usage(&mut usage, parsed_usage);
            }
            if let Some(value) = parse_sse_frame_json(&frame) {
                update_responses_sse_synthesis(&mut responses_sse_synthesis, &value);
                update_chat_completion_sse_synthesis(&mut chat_completion_synthesis, &value);
                if value
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind == "response.completed")
                {
                    if let Some(response_obj) = value.get("response") {
                        completed_response = Some(response_obj.clone());
                    }
                }
            }
            continue;
        }
        frame_lines.push(line.clone());
    }

    if !frame_lines.is_empty() {
        let inspection = inspect_sse_frame(&frame_lines);
        if let Some(parsed_usage) = inspection.usage {
            merge_usage(&mut usage, parsed_usage);
        }
        if let Some(value) = parse_sse_frame_json(&frame_lines) {
            update_responses_sse_synthesis(&mut responses_sse_synthesis, &value);
            update_chat_completion_sse_synthesis(&mut chat_completion_synthesis, &value);
            if value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "response.completed")
            {
                if let Some(response_obj) = value.get("response") {
                    completed_response = Some(response_obj.clone());
                }
            }
        }
    }

    let body = completed_response
        .map(|value| enrich_completed_response_with_sse_text(value, &responses_sse_synthesis))
        .and_then(|value| serde_json::to_vec(&value).ok())
        .or_else(|| synthesize_response_body_from_sse(&responses_sse_synthesis))
        .or_else(|| synthesize_chat_completion_body(&chat_completion_synthesis));
    (body, usage)
}

pub(super) fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    _inflight_guard: AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    is_stream: bool,
) -> Result<UpstreamResponseBridgeResult, String> {
    match response_adapter {
        super::ResponseAdapter::Passthrough => {
            let upstream_content_type = upstream
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            let is_json = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().contains("application/json"))
                .unwrap_or(false);
            let is_sse = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            // 非 SSE 响应（即使客户端 stream=true）也按普通 body 回传，
            // 避免把 JSON 错误体按 SSE 解析导致 "stream disconnected before completion" 误报。
            if !is_sse {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let (_, sse_usage) = collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else if usage_has_signal(&sse_usage) {
                    sse_usage
                } else {
                    UpstreamResponseUsage::default()
                };
                let upstream_error_hint =
                    extract_error_hint_from_body(status.0, upstream_body.as_ref());
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint,
                });
            }
            if is_json && !is_stream {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                    .ok()
                    .map(|value| parse_usage_from_json(&value))
                    .unwrap_or_default();
                let upstream_error_hint =
                    extract_error_hint_from_body(status.0, upstream_body.as_ref());
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint,
                });
            }
            if is_sse && !is_stream {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let (synthesized_body, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized_body.unwrap_or_else(|| upstream_body.to_vec());
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    merge_usage(&mut usage, parse_usage_from_json(&value));
                }
                let upstream_error_hint = extract_error_hint_from_body(status.0, &body);
                headers.retain(|header| {
                    !header
                        .field
                        .as_str()
                        .as_str()
                        .eq_ignore_ascii_case("Content-Type")
                });
                if let Ok(content_type_header) =
                    Header::from_bytes(b"Content-Type".as_slice(), b"application/json".as_slice())
                {
                    headers.push(content_type_header);
                }
                let len = Some(body.len());
                let response =
                    Response::new(status, headers, std::io::Cursor::new(body), len, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint,
                });
            }
            if is_sse || is_stream {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response = Response::new(
                    status,
                    headers,
                    PassthroughSseUsageReader::new(upstream, Arc::clone(&usage_collector)),
                    None,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(UpstreamResponseBridgeResult {
                    usage: collector.usage,
                    stream_terminal_seen: collector.saw_terminal,
                    stream_terminal_error: collector.terminal_error,
                    delivery_error,
                    upstream_error_hint: None,
                });
            }
            let len = upstream.content_length().map(|v| v as usize);
            let response = Response::new(status, headers, upstream, len, None);
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            Ok(UpstreamResponseBridgeResult {
                usage: UpstreamResponseUsage::default(),
                stream_terminal_seen: true,
                stream_terminal_error: None,
                delivery_error,
                upstream_error_hint: None,
            })
        }
        super::ResponseAdapter::OpenAIChatCompletionsJson
        | super::ResponseAdapter::OpenAIChatCompletionsSse
        | super::ResponseAdapter::OpenAICompletionsJson
        | super::ResponseAdapter::OpenAICompletionsSse => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                    || name_str.eq_ignore_ascii_case("content-type")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            let upstream_content_type = upstream
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            let is_sse = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            let use_openai_sse_adapter = matches!(
                response_adapter,
                super::ResponseAdapter::OpenAIChatCompletionsSse
                    | super::ResponseAdapter::OpenAICompletionsSse
            );

            if use_openai_sse_adapter && is_stream && !is_sse {
                log::warn!(
                    "event=gateway_openai_stream_content_type_mismatch adapter={:?} upstream_content_type={}",
                    response_adapter,
                    upstream_content_type
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("-")
                );
            }

            if use_openai_sse_adapter && (is_stream || is_sse) && is_sse {
                if let Ok(content_type_header) =
                    Header::from_bytes(b"Content-Type".as_slice(), b"text/event-stream".as_slice())
                {
                    headers.push(content_type_header);
                }
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let delivery_error = if response_adapter
                    == super::ResponseAdapter::OpenAIChatCompletionsSse
                {
                    let response = Response::new(
                        status,
                        headers,
                        OpenAIChatCompletionsSseReader::new(upstream, Arc::clone(&usage_collector)),
                        None,
                        None,
                    );
                    request.respond(response).err().map(|err| err.to_string())
                } else {
                    let response = Response::new(
                        status,
                        headers,
                        OpenAICompletionsSseReader::new(upstream, Arc::clone(&usage_collector)),
                        None,
                        None,
                    );
                    request.respond(response).err().map(|err| err.to_string())
                };
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let output_text_empty = collector
                    .usage
                    .output_text
                    .as_deref()
                    .map(str::trim)
                    .is_none_or(str::is_empty);
                if output_text_empty {
                    log::warn!(
                        "event=gateway_openai_stream_empty_output adapter={:?} terminal_seen={} terminal_error={} output_tokens={:?}",
                        response_adapter,
                        collector.saw_terminal,
                        collector.terminal_error.as_deref().unwrap_or("-"),
                        collector.usage.output_tokens
                    );
                }
                return Ok(UpstreamResponseBridgeResult {
                    usage: collector.usage,
                    stream_terminal_seen: collector.saw_terminal,
                    stream_terminal_error: collector.terminal_error,
                    delivery_error,
                    upstream_error_hint: None,
                });
            }

            let upstream_body = upstream
                .bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let mut usage = if is_sse {
                let (_, parsed) = collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                parsed
            } else {
                UpstreamResponseUsage::default()
            };
            if let Ok(value) = serde_json::from_slice::<Value>(upstream_body.as_ref()) {
                merge_usage(&mut usage, parse_usage_from_json(&value));
            }
            let (mut body, mut content_type) = match super::adapt_upstream_response(
                response_adapter,
                upstream_content_type.as_deref(),
                upstream_body.as_ref(),
            ) {
                Ok(result) => result,
                Err(err) => (
                    serde_json::to_vec(&json!({
                        "error": {
                            "message": format!("response conversion failed: {err}"),
                            "type": "server_error"
                        }
                    }))
                    .unwrap_or_else(|_| {
                        b"{\"error\":{\"message\":\"response conversion failed\",\"type\":\"server_error\"}}"
                            .to_vec()
                    }),
                    "application/json",
                ),
            };
            if use_openai_sse_adapter
                && is_stream
                && status.0 < 400
                && !content_type.eq_ignore_ascii_case("text/event-stream")
            {
                if let Ok(mapped_json) = serde_json::from_slice::<Value>(body.as_ref()) {
                    merge_usage(&mut usage, parse_usage_from_json(&mapped_json));
                    body = if response_adapter == super::ResponseAdapter::OpenAIChatCompletionsSse {
                        synthesize_chat_completion_sse_from_json(&mapped_json)
                    } else {
                        synthesize_completions_sse_from_json(&mapped_json)
                    };
                    content_type = "text/event-stream";
                    log::warn!(
                        "event=gateway_openai_stream_synthetic_sse adapter={:?} status={} upstream_content_type={}",
                        response_adapter,
                        status.0,
                        upstream_content_type
                            .as_deref()
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("-")
                    );
                }
            }
            if let Ok(content_type_header) =
                Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes())
            {
                headers.push(content_type_header);
            }
            let len = Some(body.len());
            let response = Response::new(status, headers, std::io::Cursor::new(body), len, None);
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            let upstream_error_hint =
                extract_error_hint_from_body(status.0, upstream_body.as_ref());
            Ok(UpstreamResponseBridgeResult {
                usage,
                stream_terminal_seen: true,
                stream_terminal_error: None,
                delivery_error,
                upstream_error_hint,
            })
        }
        super::ResponseAdapter::AnthropicJson | super::ResponseAdapter::AnthropicSse => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                    || name_str.eq_ignore_ascii_case("content-type")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            let upstream_content_type = upstream
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());

            if response_adapter == super::ResponseAdapter::AnthropicSse
                && (is_stream
                    || upstream_content_type
                        .as_deref()
                        .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                        .unwrap_or(false))
            {
                if let Ok(content_type_header) =
                    Header::from_bytes(b"Content-Type".as_slice(), b"text/event-stream".as_slice())
                {
                    headers.push(content_type_header);
                }
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response = Response::new(
                    status,
                    headers,
                    AnthropicSseReader::new(upstream, Arc::clone(&usage_collector)),
                    None,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let usage = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: None,
                });
            }

            let upstream_body = upstream
                .bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                .ok()
                .map(|value| parse_usage_from_json(&value))
                .unwrap_or_default();

            let (body, content_type) = match super::adapt_upstream_response(
                response_adapter,
                upstream_content_type.as_deref(),
                upstream_body.as_ref(),
            ) {
                Ok(result) => result,
                Err(err) => (
                    super::build_anthropic_error_body(&format!(
                        "response conversion failed: {err}"
                    )),
                    "application/json",
                ),
            };
            if let Ok(content_type_header) =
                Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes())
            {
                headers.push(content_type_header);
            }

            let len = Some(body.len());
            let response = Response::new(status, headers, std::io::Cursor::new(body), len, None);
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            let upstream_error_hint =
                extract_error_hint_from_body(status.0, upstream_body.as_ref());
            Ok(UpstreamResponseBridgeResult {
                usage,
                stream_terminal_seen: true,
                stream_terminal_error: None,
                delivery_error,
                upstream_error_hint,
            })
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PassthroughSseCollector {
    usage: UpstreamResponseUsage,
    saw_terminal: bool,
    terminal_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct OpenAIStreamMeta {
    response_id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
}

fn update_openai_stream_meta(meta: &mut OpenAIStreamMeta, value: &Value) {
    let response = value.get("response");

    if meta.response_id.is_none() {
        meta.response_id = value
            .get("response_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                response
                    .and_then(|response| response.get("id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
            });
    }

    if meta.model.is_none() {
        meta.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string)
            .or_else(|| {
                response
                    .and_then(|response| response.get("model"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(str::to_string)
            });
    }

    if meta.created.is_none() {
        meta.created = value
            .get("created")
            .and_then(Value::as_i64)
            .or_else(|| value.get("created_at").and_then(Value::as_i64))
            .or_else(|| {
                response
                    .and_then(|response| response.get("created"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                response
                    .and_then(|response| response.get("created_at"))
                    .and_then(Value::as_i64)
            });
    }
}

fn apply_openai_stream_meta_defaults(mapped: &mut Value, meta: &OpenAIStreamMeta) {
    let Some(mapped_obj) = mapped.as_object_mut() else {
        return;
    };
    if let Some(id) = meta.response_id.as_deref() {
        let needs_id = mapped_obj
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|current| current.is_empty());
        if needs_id {
            mapped_obj.insert("id".to_string(), Value::String(id.to_string()));
        }
    }
    if let Some(model) = meta.model.as_deref() {
        let needs_model = mapped_obj
            .get("model")
            .and_then(Value::as_str)
            .is_none_or(|current| current.is_empty());
        if needs_model {
            mapped_obj.insert("model".to_string(), Value::String(model.to_string()));
        }
    }
    if let Some(created) = meta.created {
        let needs_created = mapped_obj
            .get("created")
            .and_then(Value::as_i64)
            .is_none_or(|current| current == 0);
        if needs_created {
            mapped_obj.insert("created".to_string(), Value::Number(created.into()));
        }
    }
}

fn extract_openai_completed_output_text(value: &Value) -> Option<String> {
    let response = value.get("response").unwrap_or(value);
    let mut output_text = String::new();
    collect_response_output_text(response, &mut output_text);
    let trimmed = output_text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn map_chunk_has_chat_text(mapped: &Value) -> bool {
    mapped
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("content"))
                    .and_then(Value::as_str)
                    .is_some_and(|content| !content.is_empty())
            })
        })
}

fn map_chunk_has_completion_text(mapped: &Value) -> bool {
    mapped
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            })
        })
}

fn is_function_call_output_item(value: &Value) -> bool {
    value
        .get("item")
        .or_else(|| value.get("output_item"))
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|item_type| item_type == "function_call")
}

fn should_skip_chat_live_text_event(event_type: &str, value: &Value) -> bool {
    match event_type {
        // `done` / `content_part.*` 往往携带已聚合全文，直播模式再转发会导致重复拼接。
        "response.output_text.done"
        | "response.content_part.added"
        | "response.content_part.delta"
        | "response.content_part.done" => true,
        // function_call 仍需透传；普通 message 的 output_item.* 文本在直播模式跳过避免重复。
        "response.output_item.added" | "response.output_item.done" => {
            !is_function_call_output_item(value)
        }
        _ => false,
    }
}

fn should_skip_completion_live_text_event(event_type: &str, value: &Value) -> bool {
    match event_type {
        "response.output_text.done"
        | "response.content_part.added"
        | "response.content_part.delta"
        | "response.content_part.done" => true,
        "response.output_item.added" | "response.output_item.done" => {
            !is_function_call_output_item(value)
        }
        _ => false,
    }
}

fn normalize_chat_chunk_delta_role(mapped: &mut Value, role_emitted: &mut bool) {
    let Some(choices) = mapped.get_mut("choices").and_then(Value::as_array_mut) else {
        return;
    };
    let mut saw_role = false;
    for choice in choices {
        let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut) else {
            continue;
        };
        if delta.contains_key("role") {
            if *role_emitted {
                delta.remove("role");
            } else {
                saw_role = true;
            }
        }
    }
    if saw_role {
        *role_emitted = true;
    }
}

fn build_chat_fallback_content_chunk(meta: &OpenAIStreamMeta, content: &str) -> Value {
    json!({
        "id": meta.response_id.clone().unwrap_or_default(),
        "object": "chat.completion.chunk",
        "created": meta.created.unwrap_or(0),
        "model": meta.model.clone().unwrap_or_default(),
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": Value::Null
        }]
    })
}

fn build_completion_fallback_text_chunk(meta: &OpenAIStreamMeta, text: &str) -> Value {
    json!({
        "id": meta.response_id.clone().unwrap_or_default(),
        "object": "text_completion",
        "created": meta.created.unwrap_or(0),
        "model": meta.model.clone().unwrap_or_default(),
        "choices": [{
            "index": 0,
            "text": text
        }]
    })
}

fn append_sse_data_frame(buffer: &mut String, payload: &Value) {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    buffer.push_str("data: ");
    buffer.push_str(data.as_str());
    buffer.push_str("\n\n");
}

fn collect_text_for_sse_delta(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let mut text = String::new();
    collect_response_output_text(value, &mut text);
    text.trim().to_string()
}

fn synthesize_chat_completion_sse_from_json(value: &Value) -> Vec<u8> {
    let Some(root) = value.as_object() else {
        return b"data: [DONE]\n\n".to_vec();
    };
    if root.contains_key("error") {
        let mut out = String::new();
        append_sse_data_frame(&mut out, value);
        out.push_str("data: [DONE]\n\n");
        return out.into_bytes();
    }

    let id = root
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    let created = root.get("created").and_then(Value::as_i64).unwrap_or(0);
    let model = root
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let mut out = String::new();
    let mut finish_reason = Value::String("stop".to_string());
    let usage = root.get("usage").cloned();

    let first_choice = root
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .cloned();
    if let Some(choice) = first_choice {
        if let Some(reason) = choice.get("finish_reason") {
            if !reason.is_null() {
                finish_reason = reason.clone();
            }
        }

        let mut delta = serde_json::Map::new();
        delta.insert("role".to_string(), Value::String("assistant".to_string()));
        let message = choice.get("message");
        let content = collect_text_for_sse_delta(message.and_then(|msg| msg.get("content")));
        if !content.is_empty() {
            delta.insert("content".to_string(), Value::String(content));
        }
        if let Some(tool_calls) = message
            .and_then(|msg| msg.get("tool_calls"))
            .and_then(Value::as_array)
            .filter(|tool_calls| !tool_calls.is_empty())
        {
            delta.insert("tool_calls".to_string(), Value::Array(tool_calls.to_vec()));
        }
        if delta.get("content").is_some() || delta.get("tool_calls").is_some() {
            let content_chunk = json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": Value::Object(delta),
                    "finish_reason": Value::Null
                }]
            });
            append_sse_data_frame(&mut out, &content_chunk);
        }
    }

    let mut finish_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": finish_reason
        }]
    });
    if let Some(usage) = usage {
        if let Some(finish_obj) = finish_chunk.as_object_mut() {
            finish_obj.insert("usage".to_string(), usage);
        }
    }
    append_sse_data_frame(&mut out, &finish_chunk);
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn synthesize_completions_sse_from_json(value: &Value) -> Vec<u8> {
    let Some(root) = value.as_object() else {
        return b"data: [DONE]\n\n".to_vec();
    };
    if root.contains_key("error") {
        let mut out = String::new();
        append_sse_data_frame(&mut out, value);
        out.push_str("data: [DONE]\n\n");
        return out.into_bytes();
    }

    let id = root
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    let created = root.get("created").and_then(Value::as_i64).unwrap_or(0);
    let model = root
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();

    let mut out = String::new();
    let mut finish_reason = Value::String("stop".to_string());
    let usage = root.get("usage").cloned();

    let first_choice = root
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .cloned();
    if let Some(choice) = first_choice {
        if let Some(reason) = choice.get("finish_reason") {
            if !reason.is_null() {
                finish_reason = reason.clone();
            }
        }
        let text = collect_text_for_sse_delta(choice.get("text"));
        if !text.is_empty() {
            let content_chunk = json!({
                "id": id,
                "object": "text_completion",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "text": text,
                    "finish_reason": Value::Null
                }]
            });
            append_sse_data_frame(&mut out, &content_chunk);
        }
    }

    let mut finish_chunk = json!({
        "id": id,
        "object": "text_completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "text": "",
            "finish_reason": finish_reason
        }]
    });
    if let Some(usage) = usage {
        if let Some(finish_obj) = finish_chunk.as_object_mut() {
            finish_obj.insert("usage".to_string(), usage);
        }
    }
    append_sse_data_frame(&mut out, &finish_chunk);
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn collector_output_text_trimmed(
    usage_collector: &Arc<Mutex<PassthroughSseCollector>>,
) -> Option<String> {
    usage_collector
        .lock()
        .ok()
        .and_then(|collector| collector.usage.output_text.clone())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn mark_collector_terminal_success(usage_collector: &Arc<Mutex<PassthroughSseCollector>>) {
    if let Ok(mut collector) = usage_collector.lock() {
        collector.saw_terminal = true;
        collector.terminal_error = None;
    }
}

struct PassthroughSseUsageReader {
    upstream: BufReader<reqwest::blocking::Response>,
    pending_frame_lines: Vec<String>,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    finished: bool,
}

impl PassthroughSseUsageReader {
    fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    ) -> Self {
        Self {
            upstream: BufReader::new(upstream),
            pending_frame_lines: Vec::new(),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            finished: false,
        }
    }

    fn update_usage_from_frame(&self, lines: &[String]) {
        let inspection = inspect_sse_frame(lines);
        if inspection.usage.is_none() && inspection.terminal.is_none() {
            return;
        }
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(parsed) = inspection.usage {
                merge_usage(&mut collector.usage, parsed);
            }
            if let Some(terminal) = inspection.terminal {
                collector.saw_terminal = true;
                if let SseTerminal::Err(message) = terminal {
                    collector.terminal_error = Some(message);
                }
            }
        }
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        let mut line = String::new();
        let read = self.upstream.read_line(&mut line)?;
        if read == 0 {
            if !self.pending_frame_lines.is_empty() {
                let frame = std::mem::take(&mut self.pending_frame_lines);
                self.update_usage_from_frame(&frame);
            }
            if let Ok(mut collector) = self.usage_collector.lock() {
                if !collector.saw_terminal {
                    collector
                        .terminal_error
                        .get_or_insert_with(|| "stream disconnected before completion".to_string());
                }
            }
            self.finished = true;
            return Ok(Vec::new());
        }
        if line == "\n" || line == "\r\n" {
            if !self.pending_frame_lines.is_empty() {
                let frame = std::mem::take(&mut self.pending_frame_lines);
                self.update_usage_from_frame(&frame);
            }
        } else {
            self.pending_frame_lines.push(line.clone());
        }
        Ok(line.into_bytes())
    }
}

impl Read for PassthroughSseUsageReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}

struct OpenAICompletionsSseReader {
    upstream: BufReader<reqwest::blocking::Response>,
    pending_frame_lines: Vec<String>,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    stream_meta: OpenAIStreamMeta,
    emitted_text_delta: bool,
    finished: bool,
}

impl OpenAICompletionsSseReader {
    fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    ) -> Self {
        Self {
            upstream: BufReader::new(upstream),
            pending_frame_lines: Vec::new(),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            stream_meta: OpenAIStreamMeta::default(),
            emitted_text_delta: false,
            finished: false,
        }
    }

    fn update_usage_from_frame(&self, lines: &[String]) {
        let inspection = inspect_sse_frame(lines);
        if inspection.usage.is_none() && inspection.terminal.is_none() {
            return;
        }
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(parsed) = inspection.usage {
                merge_usage(&mut collector.usage, parsed);
            }
            if let Some(terminal) = inspection.terminal {
                collector.saw_terminal = true;
                if let SseTerminal::Err(message) = terminal {
                    collector.terminal_error = Some(message);
                }
            }
        }
    }

    fn try_build_completion_fallback_stream(&mut self, include_done: bool) -> Option<Vec<u8>> {
        if self.emitted_text_delta {
            return None;
        }
        let fallback_text = collector_output_text_trimmed(&self.usage_collector)?;
        let mut fallback_chunk =
            build_completion_fallback_text_chunk(&self.stream_meta, fallback_text.as_str());
        apply_openai_stream_meta_defaults(&mut fallback_chunk, &self.stream_meta);
        let payload = serde_json::to_string(&fallback_chunk).unwrap_or_else(|_| "{}".to_string());
        let mut out = format!("data: {payload}\n\n");
        self.emitted_text_delta = true;
        if include_done {
            out.push_str("data: [DONE]\n\n");
            self.finished = true;
        }
        mark_collector_terminal_success(&self.usage_collector);
        Some(out.into_bytes())
    }

    fn map_frame_to_completions_sse(&mut self, lines: &[String]) -> Vec<u8> {
        let Some(data) = extract_sse_frame_payload(lines) else {
            return Vec::new();
        };
        if data.trim() == "[DONE]" {
            if let Some(fallback) = self.try_build_completion_fallback_stream(true) {
                return fallback;
            }
            self.finished = true;
            return b"data: [DONE]\n\n".to_vec();
        }

        let Some(value) = parse_sse_frame_json(lines) else {
            return Vec::new();
        };
        update_openai_stream_meta(&mut self.stream_meta, &value);
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "response.created" {
            return Vec::new();
        }

        let mut out = String::new();
        if event_type == "response.completed" && !self.emitted_text_delta {
            if let Some(fallback_text) = extract_openai_completed_output_text(&value) {
                let mut fallback_chunk =
                    build_completion_fallback_text_chunk(&self.stream_meta, fallback_text.as_str());
                apply_openai_stream_meta_defaults(&mut fallback_chunk, &self.stream_meta);
                let payload =
                    serde_json::to_string(&fallback_chunk).unwrap_or_else(|_| "{}".to_string());
                out.push_str(format!("data: {payload}\n\n").as_str());
                self.emitted_text_delta = true;
            }
        }

        if should_skip_completion_live_text_event(event_type, &value) {
            return out.into_bytes();
        }

        if let Some(mut mapped) = super::convert_openai_completions_stream_chunk(&value) {
            apply_openai_stream_meta_defaults(&mut mapped, &self.stream_meta);
            if map_chunk_has_completion_text(&mapped) {
                self.emitted_text_delta = true;
            }
            let payload = serde_json::to_string(&mapped).unwrap_or_else(|_| "{}".to_string());
            out.push_str(format!("data: {payload}\n\n").as_str());
        }

        if event_type == "response.completed" {
            out.push_str("data: [DONE]\n\n");
            self.finished = true;
        }

        out.into_bytes()
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.upstream.read_line(&mut line)?;
            if read == 0 {
                if !self.pending_frame_lines.is_empty() {
                    let frame = std::mem::take(&mut self.pending_frame_lines);
                    self.update_usage_from_frame(&frame);
                    let mapped = self.map_frame_to_completions_sse(&frame);
                    if !mapped.is_empty() {
                        return Ok(mapped);
                    }
                }
                if let Some(fallback) = self.try_build_completion_fallback_stream(true) {
                    return Ok(fallback);
                }
                if let Ok(mut collector) = self.usage_collector.lock() {
                    if !collector.saw_terminal {
                        if self.emitted_text_delta {
                            collector.saw_terminal = true;
                        } else {
                            collector.terminal_error.get_or_insert_with(|| {
                                "stream disconnected before completion".to_string()
                            });
                        }
                    }
                }
                self.finished = true;
                return Ok(Vec::new());
            }
            if line == "\n" || line == "\r\n" {
                if self.pending_frame_lines.is_empty() {
                    continue;
                }
                let frame = std::mem::take(&mut self.pending_frame_lines);
                self.update_usage_from_frame(&frame);
                let mapped = self.map_frame_to_completions_sse(&frame);
                if !mapped.is_empty() {
                    return Ok(mapped);
                }
                continue;
            }
            self.pending_frame_lines.push(line.clone());
        }
    }
}

impl Read for OpenAICompletionsSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}

struct OpenAIChatCompletionsSseReader {
    upstream: BufReader<reqwest::blocking::Response>,
    pending_frame_lines: Vec<String>,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    stream_meta: OpenAIStreamMeta,
    emitted_text_delta: bool,
    emitted_assistant_role: bool,
    finished: bool,
}

impl OpenAIChatCompletionsSseReader {
    fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    ) -> Self {
        Self {
            upstream: BufReader::new(upstream),
            pending_frame_lines: Vec::new(),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            stream_meta: OpenAIStreamMeta::default(),
            emitted_text_delta: false,
            emitted_assistant_role: false,
            finished: false,
        }
    }

    fn update_usage_from_frame(&self, lines: &[String]) {
        let inspection = inspect_sse_frame(lines);
        if inspection.usage.is_none() && inspection.terminal.is_none() {
            return;
        }
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(parsed) = inspection.usage {
                merge_usage(&mut collector.usage, parsed);
            }
            if let Some(terminal) = inspection.terminal {
                collector.saw_terminal = true;
                if let SseTerminal::Err(message) = terminal {
                    collector.terminal_error = Some(message);
                }
            }
        }
    }

    fn try_build_chat_fallback_stream(&mut self, include_done: bool) -> Option<Vec<u8>> {
        if self.emitted_text_delta {
            return None;
        }
        let fallback_content = collector_output_text_trimmed(&self.usage_collector)?;
        let mut fallback_chunk =
            build_chat_fallback_content_chunk(&self.stream_meta, fallback_content.as_str());
        apply_openai_stream_meta_defaults(&mut fallback_chunk, &self.stream_meta);
        normalize_chat_chunk_delta_role(&mut fallback_chunk, &mut self.emitted_assistant_role);
        let payload = serde_json::to_string(&fallback_chunk).unwrap_or_else(|_| "{}".to_string());
        let mut out = format!("data: {payload}\n\n");
        self.emitted_text_delta = true;
        if include_done {
            out.push_str("data: [DONE]\n\n");
            self.finished = true;
        }
        mark_collector_terminal_success(&self.usage_collector);
        Some(out.into_bytes())
    }

    fn map_frame_to_chat_completions_sse(&mut self, lines: &[String]) -> Vec<u8> {
        let Some(data) = extract_sse_frame_payload(lines) else {
            return Vec::new();
        };
        if data.trim() == "[DONE]" {
            if let Some(fallback) = self.try_build_chat_fallback_stream(true) {
                return fallback;
            }
            self.finished = true;
            return b"data: [DONE]\n\n".to_vec();
        }

        let Some(value) = parse_sse_frame_json(lines) else {
            return Vec::new();
        };
        update_openai_stream_meta(&mut self.stream_meta, &value);
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "response.created" {
            return Vec::new();
        }

        let mut out = String::new();
        if event_type == "response.completed" && !self.emitted_text_delta {
            if let Some(fallback_content) = extract_openai_completed_output_text(&value) {
                let mut fallback_chunk =
                    build_chat_fallback_content_chunk(&self.stream_meta, fallback_content.as_str());
                apply_openai_stream_meta_defaults(&mut fallback_chunk, &self.stream_meta);
                normalize_chat_chunk_delta_role(
                    &mut fallback_chunk,
                    &mut self.emitted_assistant_role,
                );
                let payload =
                    serde_json::to_string(&fallback_chunk).unwrap_or_else(|_| "{}".to_string());
                out.push_str(format!("data: {payload}\n\n").as_str());
                self.emitted_text_delta = true;
            }
        }

        if should_skip_chat_live_text_event(event_type, &value) {
            return out.into_bytes();
        }

        if let Some(mut mapped) = super::convert_openai_chat_stream_chunk(&value) {
            apply_openai_stream_meta_defaults(&mut mapped, &self.stream_meta);
            normalize_chat_chunk_delta_role(&mut mapped, &mut self.emitted_assistant_role);
            if map_chunk_has_chat_text(&mapped) {
                self.emitted_text_delta = true;
            }
            let payload = serde_json::to_string(&mapped).unwrap_or_else(|_| "{}".to_string());
            out.push_str(format!("data: {payload}\n\n").as_str());
        }

        if event_type == "response.completed" {
            out.push_str("data: [DONE]\n\n");
            self.finished = true;
        }

        out.into_bytes()
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.upstream.read_line(&mut line)?;
            if read == 0 {
                if !self.pending_frame_lines.is_empty() {
                    let frame = std::mem::take(&mut self.pending_frame_lines);
                    self.update_usage_from_frame(&frame);
                    let mapped = self.map_frame_to_chat_completions_sse(&frame);
                    if !mapped.is_empty() {
                        return Ok(mapped);
                    }
                }
                if let Some(fallback) = self.try_build_chat_fallback_stream(true) {
                    return Ok(fallback);
                }
                if let Ok(mut collector) = self.usage_collector.lock() {
                    if !collector.saw_terminal {
                        if self.emitted_text_delta {
                            collector.saw_terminal = true;
                        } else {
                            collector.terminal_error.get_or_insert_with(|| {
                                "stream disconnected before completion".to_string()
                            });
                        }
                    }
                }
                self.finished = true;
                return Ok(Vec::new());
            }
            if line == "\n" || line == "\r\n" {
                if self.pending_frame_lines.is_empty() {
                    continue;
                }
                let frame = std::mem::take(&mut self.pending_frame_lines);
                self.update_usage_from_frame(&frame);
                let mapped = self.map_frame_to_chat_completions_sse(&frame);
                if !mapped.is_empty() {
                    return Ok(mapped);
                }
                continue;
            }
            self.pending_frame_lines.push(line.clone());
        }
    }
}

impl Read for OpenAIChatCompletionsSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}

struct AnthropicSseReader {
    upstream: BufReader<reqwest::blocking::Response>,
    pending_frame_lines: Vec<String>,
    out_cursor: Cursor<Vec<u8>>,
    state: AnthropicSseState,
    usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
}

#[derive(Default)]
struct AnthropicSseState {
    started: bool,
    finished: bool,
    text_block_index: Option<usize>,
    next_block_index: usize,
    response_id: Option<String>,
    model: Option<String>,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    reasoning_output_tokens: i64,
    output_text: String,
    stop_reason: Option<&'static str>,
}

impl AnthropicSseReader {
    fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
    ) -> Self {
        Self {
            upstream: BufReader::new(upstream),
            pending_frame_lines: Vec::new(),
            out_cursor: Cursor::new(Vec::new()),
            state: AnthropicSseState::default(),
            usage_collector,
        }
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.upstream.read_line(&mut line)?;
            if read == 0 {
                return Ok(self.finish_stream());
            }
            if line == "\n" || line == "\r\n" {
                let frame = std::mem::take(&mut self.pending_frame_lines);
                let mapped = self.process_sse_frame(&frame);
                if !mapped.is_empty() {
                    return Ok(mapped);
                }
                continue;
            }
            self.pending_frame_lines.push(line.clone());
        }
    }

    fn process_sse_frame(&mut self, lines: &[String]) -> Vec<u8> {
        let mut data_lines = Vec::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }
        if data_lines.is_empty() {
            return Vec::new();
        }
        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            return self.finish_stream();
        }

        let value = match serde_json::from_str::<Value>(&data) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        self.consume_openai_event(&value)
    }

    fn consume_openai_event(&mut self, value: &Value) -> Vec<u8> {
        self.capture_response_meta(value);
        let mut out = String::new();
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return Vec::new();
        };
        match event_type {
            "response.output_text.delta" => {
                let fragment = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if fragment.is_empty() {
                    return Vec::new();
                }
                append_output_text(&mut self.state.output_text, fragment);
                self.ensure_message_start(&mut out);
                self.ensure_text_block_start(&mut out);
                let text_index = self.state.text_block_index.unwrap_or(0);
                append_sse_event(
                    &mut out,
                    "content_block_delta",
                    &json!({
                        "type": "content_block_delta",
                        "index": text_index,
                        "delta": {
                            "type": "text_delta",
                            "text": fragment
                        }
                    }),
                );
                self.state.stop_reason.get_or_insert("end_turn");
            }
            "response.output_item.done" => {
                collect_output_text_from_event_fields(value, &mut self.state.output_text);
                let Some(item_obj) = value
                    .get("item")
                    .or_else(|| value.get("output_item"))
                    .and_then(Value::as_object)
                else {
                    return Vec::new();
                };
                if item_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind != "function_call")
                {
                    return Vec::new();
                }
                self.ensure_message_start(&mut out);
                self.close_text_block(&mut out);
                let block_index = self.state.next_block_index;
                self.state.next_block_index = self.state.next_block_index.saturating_add(1);
                let tool_use_id = item_obj
                    .get("call_id")
                    .or_else(|| item_obj.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("toolu_unknown");
                let tool_name = item_obj
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tool_use_id,
                            "name": tool_name,
                            "input": {}
                        }
                    }),
                );
                if let Some(partial_json) =
                    extract_function_call_input(item_obj).and_then(tool_input_partial_json)
                {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": block_index,
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
                        "index": block_index
                    }),
                );
                self.state.stop_reason = Some("tool_use");
            }
            _ if event_type.starts_with("response.output_item.")
                || event_type.starts_with("response.content_part.") =>
            {
                collect_output_text_from_event_fields(value, &mut self.state.output_text);
            }
            "response.completed" => {
                if let Some(response) = value.get("response") {
                    let mut extracted_output_text = String::new();
                    collect_response_output_text(response, &mut extracted_output_text);
                    if !extracted_output_text.trim().is_empty() {
                        // 若已在流式过程中发过文本增量，不再重复把 completed 全文再发一遍。
                        if self.state.text_block_index.is_none() {
                            append_output_text(
                                &mut self.state.output_text,
                                extracted_output_text.as_str(),
                            );
                            self.ensure_message_start(&mut out);
                            self.ensure_text_block_start(&mut out);
                            let text_index = self.state.text_block_index.unwrap_or(0);
                            append_sse_event(
                                &mut out,
                                "content_block_delta",
                                &json!({
                                    "type": "content_block_delta",
                                    "index": text_index,
                                    "delta": {
                                        "type": "text_delta",
                                        "text": extracted_output_text
                                    }
                                }),
                            );
                        }
                        self.state.stop_reason.get_or_insert("end_turn");
                    }
                }
            }
            _ => {}
        }
        out.into_bytes()
    }

    fn capture_response_meta(&mut self, value: &Value) {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            self.state.response_id = Some(id.to_string());
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.state.model = Some(model.to_string());
        }
        if let Some(response) = value.get("response").and_then(Value::as_object) {
            if let Some(id) = response.get("id").and_then(Value::as_str) {
                self.state.response_id = Some(id.to_string());
            }
            if let Some(model) = response.get("model").and_then(Value::as_str) {
                self.state.model = Some(model.to_string());
            }
            if let Some(usage) = response.get("usage").and_then(Value::as_object) {
                self.state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| usage.get("prompt_tokens").and_then(Value::as_i64))
                    .unwrap_or(self.state.input_tokens);
                self.state.cached_input_tokens = usage
                    .get("input_tokens_details")
                    .and_then(Value::as_object)
                    .and_then(|details| details.get("cached_tokens"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        usage
                            .get("prompt_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(self.state.cached_input_tokens);
                self.state.output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| usage.get("completion_tokens").and_then(Value::as_i64))
                    .unwrap_or(self.state.output_tokens);
                self.state.total_tokens = usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .or(self.state.total_tokens);
                self.state.reasoning_output_tokens = usage
                    .get("output_tokens_details")
                    .and_then(Value::as_object)
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        usage
                            .get("completion_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("reasoning_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(self.state.reasoning_output_tokens);
            }
        }
    }

    fn ensure_message_start(&mut self, out: &mut String) {
        if self.state.started {
            return;
        }
        self.state.started = true;
        append_sse_event(
            out,
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": self.state.response_id.clone().unwrap_or_else(|| "msg_proxy".to_string()),
                    "type": "message",
                    "role": "assistant",
                    "model": self.state.model.clone().unwrap_or_else(|| "gpt-5.3-codex".to_string()),
                    "content": [],
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": {
                        "input_tokens": self.state.input_tokens.max(0),
                        "output_tokens": 0
                    }
                }
            }),
        );
    }

    fn ensure_text_block_start(&mut self, out: &mut String) {
        if self.state.text_block_index.is_some() {
            return;
        }
        let index = self.state.next_block_index;
        self.state.next_block_index = self.state.next_block_index.saturating_add(1);
        self.state.text_block_index = Some(index);
        append_sse_event(
            out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        );
    }

    fn close_text_block(&mut self, out: &mut String) {
        let Some(index) = self.state.text_block_index.take() else {
            return;
        };
        append_sse_event(
            out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": index
            }),
        );
    }

    fn finish_stream(&mut self) -> Vec<u8> {
        if self.state.finished {
            return Vec::new();
        }
        self.state.finished = true;
        if let Ok(mut usage) = self.usage_collector.lock() {
            usage.input_tokens = Some(self.state.input_tokens.max(0));
            usage.cached_input_tokens = Some(self.state.cached_input_tokens.max(0));
            usage.output_tokens = Some(self.state.output_tokens.max(0));
            usage.total_tokens = self.state.total_tokens.map(|value| value.max(0));
            usage.reasoning_output_tokens = Some(self.state.reasoning_output_tokens.max(0));
            if !self.state.output_text.trim().is_empty() {
                usage.output_text = Some(self.state.output_text.clone());
            }
        }
        let mut out = String::new();
        self.ensure_message_start(&mut out);
        self.close_text_block(&mut out);
        append_sse_event(
            &mut out,
            "message_delta",
            &json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": self.state.stop_reason.unwrap_or("end_turn"),
                    "stop_sequence": Value::Null
                },
                "usage": {
                    "output_tokens": self.state.output_tokens.max(0)
                }
            }),
        );
        append_sse_event(&mut out, "message_stop", &json!({ "type": "message_stop" }));
        out.into_bytes()
    }
}

impl Read for AnthropicSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.state.finished {
                return Ok(0);
            }
            let next = self.next_chunk()?;
            self.out_cursor = Cursor::new(next);
        }
    }
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

fn extract_function_call_input(item_obj: &Map<String, Value>) -> Option<Value> {
    const ARGUMENT_KEYS: [&str; 5] = [
        "arguments",
        "input",
        "arguments_json",
        "parsed_arguments",
        "args",
    ];
    for key in ARGUMENT_KEYS {
        let Some(value) = item_obj.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return Some(parsed);
            }
            return Some(Value::String(trimmed.to_string()));
        }
        return Some(value.clone());
    }
    None
}

fn tool_input_partial_json(value: Value) -> Option<String> {
    let serialized = serde_json::to_string(&value).ok()?;
    let trimmed = serialized.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
#[path = "tests/http_bridge_tests.rs"]
mod tests;
