use serde_json::Value;

mod request_rewrite_chat_completions;
mod request_rewrite_responses;
mod request_rewrite_shared;

use request_rewrite_chat_completions as chat_completions;
use request_rewrite_responses as responses;

type RetainFn = fn(&str, &mut serde_json::Map<String, Value>) -> Vec<String>;
const RETAIN_FN_PROBE_KEY: &str = "__codexmanager_allowlist_probe__";

pub(super) fn compute_upstream_url(base: &str, path: &str) -> (String, Option<String>) {
    let base = base.trim_end_matches('/');
    let url = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        // 中文注释：兼容 ChatGPT backend-api/codex 的路径约定；不做映射会导致 /v1/* 请求 404。
        format!("{}{}", base, path.trim_start_matches("/v1"))
    } else if base.ends_with("/v1") && path.starts_with("/v1") {
        format!("{}{}", base.trim_end_matches("/v1"), path)
    } else {
        format!("{}{}", base, path)
    };
    let url_alt = if base.contains("/backend-api/codex") && path.starts_with("/v1/") {
        Some(format!("{}{}", base, path))
    } else {
        None
    };
    (url, url_alt)
}

fn is_codex_backend_base(base: &str) -> bool {
    base.to_ascii_lowercase().contains("/backend-api/codex")
}

fn should_apply_codex_responses_compat(path: &str, explicit_upstream_base: Option<&str>) -> bool {
    if !responses::is_responses_path(path) {
        return false;
    }
    let resolved_base = explicit_upstream_base
        .map(str::to_string)
        .unwrap_or_else(super::upstream::config::resolve_upstream_base_url);
    let normalized_base = super::upstream::config::normalize_upstream_base_url(&resolved_base);
    is_codex_backend_base(&normalized_base)
}

fn path_matches_retain_fn(path: &str, retain_fn: RetainFn) -> bool {
    let mut probe = serde_json::Map::new();
    probe.insert(RETAIN_FN_PROBE_KEY.to_string(), Value::Null);
    retain_fn(path, &mut probe);
    !probe.contains_key(RETAIN_FN_PROBE_KEY)
}

fn resolve_retain_fn(path: &str, use_codex_responses_compat: bool) -> Option<RetainFn> {
    if path_matches_retain_fn(path, chat_completions::retain_official_fields) {
        return Some(chat_completions::retain_official_fields);
    }
    if use_codex_responses_compat {
        if path_matches_retain_fn(path, responses::retain_codex_fields) {
            return Some(responses::retain_codex_fields);
        }
    } else if path_matches_retain_fn(path, responses::retain_official_fields) {
        return Some(responses::retain_official_fields);
    }
    None
}

fn is_allowed_field(path: &str, key: &str, retain_fn: RetainFn) -> bool {
    let mut one = serde_json::Map::new();
    one.insert(key.to_string(), Value::Null);
    retain_fn(path, &mut one);
    one.contains_key(key)
}

fn find_subsequence(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() || haystack.len() < needle.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|idx| idx + start)
}

fn extract_multipart_part_name(headers: &[u8]) -> Option<String> {
    let headers_str = std::str::from_utf8(headers).ok()?;
    for line in headers_str.split("\r\n") {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("content-disposition") {
            continue;
        }
        for token in value.split(';') {
            let token = token.trim();
            if token
                .get(..5)
                .map(|prefix| prefix.eq_ignore_ascii_case("name="))
                .unwrap_or(false)
            {
                let mut field_name = token[5..].trim().to_string();
                if field_name.starts_with('"') && field_name.ends_with('"') && field_name.len() >= 2
                {
                    field_name = field_name[1..field_name.len() - 1].to_string();
                }
                if !field_name.is_empty() {
                    return Some(field_name);
                }
            }
        }
    }
    None
}

fn filter_form_urlencoded_body(
    path: &str,
    body: &[u8],
    retain_fn: RetainFn,
) -> Option<(Vec<u8>, Vec<String>)> {
    if !body.contains(&b'=') {
        return None;
    }
    let pairs = url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    let mut dropped_keys = Vec::new();
    for (key, value) in pairs {
        if is_allowed_field(path, &key, retain_fn) {
            serializer.append_pair(&key, &value);
        } else {
            dropped_keys.push(key);
        }
    }
    if dropped_keys.is_empty() {
        return None;
    }
    Some((serializer.finish().into_bytes(), dropped_keys))
}

fn filter_multipart_form_data_body(
    path: &str,
    body: &[u8],
    retain_fn: RetainFn,
) -> Option<(Vec<u8>, Vec<String>)> {
    if !body.starts_with(b"--") {
        return None;
    }
    let boundary_line_end = find_subsequence(body, b"\r\n", 0)?;
    if boundary_line_end <= 2 {
        return None;
    }
    let boundary = &body[2..boundary_line_end];
    if boundary.is_empty() {
        return None;
    }
    let mut boundary_marker = Vec::with_capacity(boundary.len() + 2);
    boundary_marker.extend_from_slice(b"--");
    boundary_marker.extend_from_slice(boundary);
    if !body.starts_with(&boundary_marker) {
        return None;
    }

    let mut delimiter_with_crlf = Vec::with_capacity(boundary_marker.len() + 2);
    delimiter_with_crlf.extend_from_slice(b"\r\n");
    delimiter_with_crlf.extend_from_slice(&boundary_marker);

    let mut cursor = boundary_marker.len();
    if body.get(cursor..cursor + 2) == Some(b"--") {
        return None;
    }
    if body.get(cursor..cursor + 2) != Some(b"\r\n") {
        return None;
    }
    cursor += 2;

    let mut kept_parts: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut dropped_keys = Vec::new();

    loop {
        let headers_end = find_subsequence(body, b"\r\n\r\n", cursor)?;
        let headers = &body[cursor..headers_end];
        let part_body_start = headers_end + 4;
        let next_boundary = find_subsequence(body, &delimiter_with_crlf, part_body_start)?;
        let part_body = &body[part_body_start..next_boundary];

        let keep = match extract_multipart_part_name(headers) {
            Some(name) => {
                if is_allowed_field(path, &name, retain_fn) {
                    true
                } else {
                    dropped_keys.push(name);
                    false
                }
            }
            None => true,
        };
        if keep {
            kept_parts.push((headers.to_vec(), part_body.to_vec()));
        }

        cursor = next_boundary + delimiter_with_crlf.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        if body.get(cursor..cursor + 2) != Some(b"\r\n") {
            return None;
        }
        cursor += 2;
    }

    if dropped_keys.is_empty() {
        return None;
    }

    let mut rebuilt = Vec::new();
    for (idx, (headers, part_body)) in kept_parts.iter().enumerate() {
        if idx > 0 {
            rebuilt.extend_from_slice(b"\r\n");
        }
        rebuilt.extend_from_slice(&boundary_marker);
        rebuilt.extend_from_slice(b"\r\n");
        rebuilt.extend_from_slice(headers);
        rebuilt.extend_from_slice(b"\r\n\r\n");
        rebuilt.extend_from_slice(part_body);
    }
    if !kept_parts.is_empty() {
        rebuilt.extend_from_slice(b"\r\n");
    }
    rebuilt.extend_from_slice(&boundary_marker);
    rebuilt.extend_from_slice(b"--\r\n");

    Some((rebuilt, dropped_keys))
}

pub(super) fn apply_request_overrides(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
) -> Vec<u8> {
    let use_codex_responses_compat = should_apply_codex_responses_compat(path, upstream_base_url);
    let normalized_model = model_slug.map(str::trim).filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .map(str::to_string);
    if body.is_empty() {
        return body;
    }
    if let Ok(mut payload) = serde_json::from_slice::<Value>(&body) {
        if let Some(obj) = payload.as_object_mut() {
            let mut changed = false;
            let mut dropped_keys = Vec::new();

            if let Some(model) = normalized_model {
                obj.insert("model".to_string(), Value::String(model.to_string()));
                changed = true;
            }

            if chat_completions::normalize_responses_payload(path, obj) {
                changed = true;
            }

            if let Some(level) = normalized_reasoning.as_deref() {
                if responses::apply_reasoning_override(path, obj, Some(level)) {
                    changed = true;
                }
                if chat_completions::apply_reasoning_override(path, obj, Some(level)) {
                    changed = true;
                }
            }

            if chat_completions::ensure_reasoning_effort(path, obj) {
                changed = true;
            }
            if chat_completions::ensure_stream_usage_override(path, obj) {
                changed = true;
            }

            if super::strict_request_param_allowlist_enabled() {
                dropped_keys.extend(chat_completions::retain_official_fields(path, obj));
                if !use_codex_responses_compat {
                    dropped_keys.extend(responses::retain_official_fields(path, obj));
                }
            }

            if use_codex_responses_compat {
                let had_stream_passthrough = obj.contains_key("stream_passthrough");
                let stream_passthrough = responses::take_stream_passthrough_flag(path, obj);
                if had_stream_passthrough {
                    changed = true;
                }
                if responses::ensure_input_list(path, obj) {
                    changed = true;
                }
                if !stream_passthrough && responses::ensure_stream_true(path, obj) {
                    changed = true;
                }
                if responses::ensure_store_false(path, obj) {
                    changed = true;
                }
                if responses::ensure_instructions(path, obj) {
                    changed = true;
                }
                dropped_keys.extend(responses::retain_codex_fields(path, obj));
            }

            if !dropped_keys.is_empty() {
                dropped_keys.sort_unstable();
                dropped_keys.dedup();
                changed = true;
                log::debug!(
                    "event=gateway_request_param_filtered path={} dropped_keys={}",
                    path,
                    dropped_keys.join(",")
                );
            }

            if !changed {
                return body;
            }
            return serde_json::to_vec(&payload).unwrap_or(body);
        }
    }

    if !super::strict_request_param_allowlist_enabled() {
        return body;
    }
    let Some(retain_fn) = resolve_retain_fn(path, use_codex_responses_compat) else {
        return body;
    };

    let filtered = filter_multipart_form_data_body(path, &body, retain_fn)
        .or_else(|| filter_form_urlencoded_body(path, &body, retain_fn));
    let Some((filtered_body, mut dropped_keys)) = filtered else {
        return body;
    };

    dropped_keys.sort_unstable();
    dropped_keys.dedup();
    log::debug!(
        "event=gateway_request_param_filtered path={} dropped_keys={}",
        path,
        dropped_keys.join(",")
    );
    filtered_body
}

#[cfg(test)]
#[path = "tests/request_rewrite_tests.rs"]
mod tests;
