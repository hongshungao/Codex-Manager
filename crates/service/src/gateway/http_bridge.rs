use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader, Cursor, Read};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Request, Response, StatusCode};

use super::AccountInFlightGuard;

#[derive(Debug, Clone, Default)]
pub(super) struct UpstreamResponseUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub output_text: Option<String>,
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
        target_text.push_str(source_text.as_str());
    }
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
            usage.and_then(|map| map.get("prompt_tokens_details"))
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
            usage.and_then(|map| map.get("completion_tokens_details"))
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
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(text);
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
            usage.output_text = Some(match usage.output_text {
                Some(existing) if !existing.is_empty() => format!("{existing}\n{text_out}"),
                _ => text_out,
            });
        }
        return Some(usage);
    }
    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
        if !delta.is_empty() {
            usage.output_text = Some(match usage.output_text {
                Some(existing) if !existing.is_empty() => format!("{existing}\n{delta}"),
                _ => delta.to_string(),
            });
        }
        return Some(usage);
    }
    Some(usage)
}

pub(super) fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    _inflight_guard: AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    is_stream: bool,
) -> Result<UpstreamResponseUsage, String> {
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
            if is_json && !is_stream {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                    .ok()
                    .map(|value| parse_usage_from_json(&value))
                    .unwrap_or_default();
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let _ = request.respond(response);
                return Ok(usage);
            }
            if is_sse || is_stream {
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response = Response::new(
                    status,
                    headers,
                    PassthroughSseUsageReader::new(upstream, Arc::clone(&usage_collector)),
                    None,
                    None,
                );
                let _ = request.respond(response);
                let usage = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(usage);
            }
            let len = upstream.content_length().map(|v| v as usize);
            let response = Response::new(status, headers, upstream, len, None);
            let _ = request.respond(response);
            Ok(UpstreamResponseUsage::default())
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
                if let Ok(content_type_header) = Header::from_bytes(
                    b"Content-Type".as_slice(),
                    b"text/event-stream".as_slice(),
                ) {
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
                let _ = request.respond(response);
                let usage = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(usage);
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
            let _ = request.respond(response);
            Ok(usage)
        }
    }
}

struct PassthroughSseUsageReader {
    upstream: BufReader<reqwest::blocking::Response>,
    pending_frame_lines: Vec<String>,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
    finished: bool,
}

impl PassthroughSseUsageReader {
    fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
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
        let Some(parsed) = parse_usage_from_sse_frame(lines) else {
            return;
        };
        if let Ok(mut usage) = self.usage_collector.lock() {
            merge_usage(&mut usage, parsed);
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
                let fragment = value.get("delta").and_then(Value::as_str).unwrap_or_default();
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
                        append_output_text(&mut self.state.output_text, extracted_output_text.as_str());
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
                        usage.get("prompt_tokens_details")
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
                        usage.get("completion_tokens_details")
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
mod tests {
    use super::{parse_usage_from_json, parse_usage_from_sse_frame};
    use serde_json::json;

    #[test]
    fn parse_usage_from_json_reads_cached_and_reasoning_details() {
        let payload = json!({
            "usage": {
                "input_tokens": 321,
                "input_tokens_details": { "cached_tokens": 280 },
                "output_tokens": 55,
                "total_tokens": 376,
                "output_tokens_details": { "reasoning_tokens": 21 }
            }
        });
        let usage = parse_usage_from_json(&payload);
        assert_eq!(usage.input_tokens, Some(321));
        assert_eq!(usage.cached_input_tokens, Some(280));
        assert_eq!(usage.output_tokens, Some(55));
        assert_eq!(usage.total_tokens, Some(376));
        assert_eq!(usage.reasoning_output_tokens, Some(21));
    }

    #[test]
    fn parse_usage_from_json_reads_response_usage_compat_fields() {
        let payload = json!({
            "type": "response.completed",
            "response": {
                "usage": {
                    "prompt_tokens": 100,
                    "prompt_tokens_details": { "cached_tokens": 75 },
                    "completion_tokens": 20,
                    "total_tokens": 120,
                    "completion_tokens_details": { "reasoning_tokens": 9 }
                }
            }
        });
        let usage = parse_usage_from_json(&payload);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cached_input_tokens, Some(75));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(120));
        assert_eq!(usage.reasoning_output_tokens, Some(9));
    }

    #[test]
    fn parse_usage_from_json_merges_response_usage_over_top_level_usage() {
        let payload = json!({
            "usage": {
                "input_tokens": 11,
                "output_tokens": 7,
                "total_tokens": 18
            },
            "response": {
                "usage": {
                    "prompt_tokens": 13,
                    "prompt_tokens_details": { "cached_tokens": 5 },
                    "completion_tokens": 9,
                    "total_tokens": 22
                }
            }
        });
        let usage = parse_usage_from_json(&payload);
        assert_eq!(usage.input_tokens, Some(13));
        assert_eq!(usage.cached_input_tokens, Some(5));
        assert_eq!(usage.output_tokens, Some(9));
        assert_eq!(usage.total_tokens, Some(22));
        assert_eq!(usage.reasoning_output_tokens, None);
    }

    #[test]
    fn parse_usage_from_sse_frame_reads_response_completed_usage() {
        let frame_lines = vec![
            "event: message\n".to_string(),
            r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":88,"input_tokens_details":{"cached_tokens":61},"output_tokens":17,"total_tokens":105,"output_tokens_details":{"reasoning_tokens":6}}}}"#
                .to_string(),
            "\n".to_string(),
        ];
        let usage = parse_usage_from_sse_frame(&frame_lines).expect("extract usage from sse frame");
        assert_eq!(usage.input_tokens, Some(88));
        assert_eq!(usage.cached_input_tokens, Some(61));
        assert_eq!(usage.output_tokens, Some(17));
        assert_eq!(usage.total_tokens, Some(105));
        assert_eq!(usage.reasoning_output_tokens, Some(6));
    }

    #[test]
    fn parse_usage_from_sse_frame_reads_top_level_and_response_usage() {
        let frame_lines = vec![
            "event: message\n".to_string(),
            r#"data: {"type":"response.completed","usage":{"input_tokens":22,"input_tokens_details":{"cached_tokens":10},"output_tokens":11,"total_tokens":33,"output_tokens_details":{"reasoning_tokens":3}},"response":{"usage":{"prompt_tokens":26,"prompt_tokens_details":{"cached_tokens":12},"completion_tokens":15,"total_tokens":41,"completion_tokens_details":{"reasoning_tokens":4}}}}"#
                .to_string(),
            "\n".to_string(),
        ];
        let usage = parse_usage_from_sse_frame(&frame_lines).expect("extract usage from sse frame");
        assert_eq!(usage.input_tokens, Some(26));
        assert_eq!(usage.cached_input_tokens, Some(12));
        assert_eq!(usage.output_tokens, Some(15));
        assert_eq!(usage.total_tokens, Some(41));
        assert_eq!(usage.reasoning_output_tokens, Some(4));
    }
}
