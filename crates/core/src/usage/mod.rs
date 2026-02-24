use serde_json::Value;

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
}

pub fn normalize_base_url(base_url: &str) -> String {
    let mut base = base_url.trim_end_matches('/').to_string();
    let is_chatgpt_host = base.starts_with("https://chatgpt.com")
        || base.starts_with("https://chat.openai.com");
    if is_chatgpt_host && !base.contains("/backend-api") {
        base.push_str("/backend-api");
    }
    base
}

pub fn usage_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.contains("/backend-api") {
        format!("{base}/wham/usage")
    } else {
        format!("{base}/api/codex/usage")
    }
}

pub fn parse_usage_snapshot(value: &Value) -> UsageSnapshot {
    let used_percent = value
        .pointer("/rate_limit/primary_window/used_percent")
        .and_then(Value::as_f64);
    let window_minutes = value
        .pointer("/rate_limit/primary_window/limit_window_seconds")
        .and_then(Value::as_i64)
        .map(|s| (s + 59) / 60);
    let resets_at = value
        .pointer("/rate_limit/primary_window/reset_at")
        .and_then(Value::as_i64);
    let secondary_used_percent = value
        .pointer("/rate_limit/secondary_window/used_percent")
        .and_then(Value::as_f64);
    let secondary_window_minutes = value
        .pointer("/rate_limit/secondary_window/limit_window_seconds")
        .and_then(Value::as_i64)
        .map(|s| (s + 59) / 60);
    let secondary_resets_at = value
        .pointer("/rate_limit/secondary_window/reset_at")
        .and_then(Value::as_i64);
    let credits_json = value
        .get("credits")
        .and_then(|v| if v.is_null() { None } else { Some(v.to_string()) });

    UsageSnapshot {
        used_percent,
        window_minutes,
        resets_at,
        secondary_used_percent,
        secondary_window_minutes,
        secondary_resets_at,
        credits_json,
    }
}
