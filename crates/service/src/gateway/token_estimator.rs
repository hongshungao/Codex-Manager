use serde_json::Value;
use tiktoken_rs::{cl100k_base, get_bpe_from_model};

fn estimate_text_tokens_with_model(text: &str, model: Option<&str>) -> Option<i64> {
    if text.trim().is_empty() {
        return Some(0);
    }
    let token_count = if let Some(model) = model.map(str::trim).filter(|v| !v.is_empty()) {
        match get_bpe_from_model(model) {
            Ok(bpe) => bpe.encode_with_special_tokens(text).len(),
            Err(_) => cl100k_base().ok()?.encode_with_special_tokens(text).len(),
        }
    } else {
        cl100k_base().ok()?.encode_with_special_tokens(text).len()
    };
    Some(token_count.min(i64::MAX as usize) as i64)
}

fn append_text_segment(buf: &mut String, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(trimmed);
}

fn collect_content_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => append_text_segment(out, text),
        Value::Array(items) => {
            for item in items {
                collect_content_text(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                append_text_segment(out, text);
            }
            if let Some(content) = map.get("content") {
                collect_content_text(content, out);
            }
            if let Some(input) = map.get("input") {
                collect_content_text(input, out);
            }
            if let Some(message) = map.get("message") {
                collect_content_text(message, out);
            }
        }
        _ => {}
    }
}

fn collect_responses_input_text(payload: &Value) -> String {
    let mut out = String::new();
    let Some(obj) = payload.as_object() else {
        return out;
    };
    if let Some(instructions) = obj.get("instructions") {
        collect_content_text(instructions, &mut out);
    }
    if let Some(input) = obj.get("input") {
        collect_content_text(input, &mut out);
    }
    if let Some(messages) = obj.get("messages") {
        collect_content_text(messages, &mut out);
    }
    out
}

fn collect_chat_completions_input_text(payload: &Value) -> String {
    let mut out = String::new();
    let Some(obj) = payload.as_object() else {
        return out;
    };
    if let Some(messages) = obj.get("messages") {
        collect_content_text(messages, &mut out);
    }
    if let Some(prompt) = obj.get("prompt") {
        collect_content_text(prompt, &mut out);
    }
    if let Some(system) = obj.get("system") {
        collect_content_text(system, &mut out);
    }
    out
}

fn collect_anthropic_input_text(payload: &Value) -> String {
    let mut out = String::new();
    let Some(obj) = payload.as_object() else {
        return out;
    };
    if let Some(system) = obj.get("system") {
        collect_content_text(system, &mut out);
    }
    if let Some(messages) = obj.get("messages") {
        collect_content_text(messages, &mut out);
    }
    out
}

pub(super) fn estimate_input_tokens(path: &str, body: &[u8], model: Option<&str>) -> Option<i64> {
    if body.is_empty() {
        return Some(0);
    }
    let payload = serde_json::from_slice::<Value>(body).ok()?;
    let text = if path.starts_with("/v1/responses") {
        collect_responses_input_text(&payload)
    } else if path.starts_with("/v1/chat/completions") {
        collect_chat_completions_input_text(&payload)
    } else if path.starts_with("/v1/messages") {
        collect_anthropic_input_text(&payload)
    } else {
        String::new()
    };
    estimate_text_tokens_with_model(text.as_str(), model)
}

pub(super) fn estimate_output_tokens(text: &str, model: Option<&str>) -> Option<i64> {
    estimate_text_tokens_with_model(text, model)
}

