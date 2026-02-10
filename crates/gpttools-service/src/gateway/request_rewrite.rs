use serde_json::Value;

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

fn path_supports_reasoning_override(path: &str) -> bool {
    path.starts_with("/v1/responses") || path.starts_with("/v1/chat/completions")
}

pub(super) fn apply_request_overrides(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Vec<u8> {
    let normalized_model = model_slug.map(str::trim).filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .map(str::to_string);
    if normalized_model.is_none() && normalized_reasoning.is_none() {
        return body;
    }
    if path == "/v1/models" || path.starts_with("/v1/models?") {
        return body;
    }
    if body.is_empty() {
        return body;
    }
    let Ok(mut payload) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    if let Some(model) = normalized_model {
        obj.insert("model".to_string(), Value::String(model.to_string()));
    }
    if let Some(level) = normalized_reasoning {
        if path_supports_reasoning_override(path) {
            let reasoning = obj
                .entry("reasoning".to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if !reasoning.is_object() {
                // 中文注释：某些客户端会把 reasoning 误传成字符串；不矫正为对象会导致 effort 覆盖失效。
                *reasoning = Value::Object(serde_json::Map::new());
            }
            if let Some(reasoning_obj) = reasoning.as_object_mut() {
                reasoning_obj.insert("effort".to_string(), Value::String(level));
            }
        }
    }
    serde_json::to_vec(&payload).unwrap_or(body)
}
