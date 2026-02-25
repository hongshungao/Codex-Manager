use codexmanager_core::storage::{now_ts, RequestLog, RequestTokenStat, Storage};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RequestLogUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
}

const MODEL_PRICE_PER_1K_TOKENS: &[(&str, f64, f64)] = &[
    ("gpt-5", 0.00125, 0.01),
    ("gpt-4.1", 0.002, 0.008),
    ("gpt-4o", 0.0025, 0.01),
    ("gpt-4", 0.03, 0.06),
    ("claude-3-7", 0.003, 0.015),
    ("claude-3-5", 0.003, 0.015),
    ("claude-3", 0.003, 0.015),
];

fn estimate_cost_usd(model: Option<&str>, input_tokens: Option<i64>, output_tokens: Option<i64>) -> f64 {
    let normalized = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let Some(normalized) = normalized else {
        return 0.0;
    };
    let Some((_, in_per_1k, out_per_1k)) = MODEL_PRICE_PER_1K_TOKENS
        .iter()
        .find(|(prefix, _, _)| normalized.starts_with(prefix))
    else {
        return 0.0;
    };
    let in_tokens = input_tokens.unwrap_or(0).max(0) as f64;
    let out_tokens = output_tokens.unwrap_or(0).max(0) as f64;
    (in_tokens / 1000.0) * in_per_1k + (out_tokens / 1000.0) * out_per_1k
}

fn normalize_token(value: Option<i64>) -> Option<i64> {
    value.map(|v| v.max(0))
}

fn is_inference_path(path: &str) -> bool {
    path.starts_with("/v1/responses")
        || path.starts_with("/v1/chat/completions")
        || path.starts_with("/v1/messages")
}

pub(super) fn write_request_log(
    storage: &Storage,
    key_id: Option<&str>,
    account_id: Option<&str>,
    request_path: &str,
    method: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_url: Option<&str>,
    status_code: Option<u16>,
    usage: RequestLogUsage,
    error: Option<&str>,
) {
    let input_tokens = normalize_token(usage.input_tokens);
    let cached_input_tokens = normalize_token(usage.cached_input_tokens);
    let output_tokens = normalize_token(usage.output_tokens);
    let reasoning_output_tokens = normalize_token(usage.reasoning_output_tokens);
    let created_at = now_ts();
    let estimated_cost_usd = estimate_cost_usd(model, input_tokens, output_tokens);
    let success = status_code
        .map(|status| (200..300).contains(&status))
        .unwrap_or(false);
    let input_zero_or_missing = input_tokens.unwrap_or(0) == 0;
    let cached_zero_or_missing = cached_input_tokens.unwrap_or(0) == 0;
    let output_zero_or_missing = output_tokens.unwrap_or(0) == 0;
    let reasoning_zero_or_missing = reasoning_output_tokens.unwrap_or(0) == 0;
    if success
        && is_inference_path(request_path)
        && input_zero_or_missing
        && cached_zero_or_missing
        && output_zero_or_missing
        && reasoning_zero_or_missing
    {
        eprintln!(
            "gateway token usage missing: path={request_path}, key_id={}, account_id={}, model={}",
            key_id.unwrap_or("-"),
            account_id.unwrap_or("-"),
            model.unwrap_or("-"),
        );
    }
    // 记录请求最终结果（而非内部重试明细），保证 UI 一次请求只展示一条记录。
    let request_log_id = storage.insert_request_log(&RequestLog {
        key_id: key_id.map(|v| v.to_string()),
        account_id: account_id.map(|v| v.to_string()),
        request_path: request_path.to_string(),
        method: method.to_string(),
        model: model.map(|v| v.to_string()),
        reasoning_effort: reasoning_effort.map(|v| v.to_string()),
        upstream_url: upstream_url.map(|v| v.to_string()),
        status_code: status_code.map(|v| i64::from(v)),
        input_tokens: None,
        cached_input_tokens: None,
        output_tokens: None,
        reasoning_output_tokens: None,
        estimated_cost_usd: None,
        error: error.map(|v| v.to_string()),
        created_at,
    });
    let Ok(request_log_id) = request_log_id else {
        return;
    };

    if let Err(err) = storage.insert_request_token_stat(&RequestTokenStat {
        request_log_id,
        key_id: key_id.map(|v| v.to_string()),
        account_id: account_id.map(|v| v.to_string()),
        model: model.map(|v| v.to_string()),
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        estimated_cost_usd: Some(estimated_cost_usd),
        created_at,
    }) {
        eprintln!("insert request_token_stats failed: {err}");
    }
}
