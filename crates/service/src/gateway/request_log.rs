use codexmanager_core::storage::{now_ts, RequestLog, RequestTokenStat, Storage};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RequestLogUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
}

const MODEL_PRICE_PER_1K_TOKENS: &[(&str, f64, f64, f64)] = &[
    // OpenAI 官方价格（单位：USD / 1K tokens）。按模型前缀匹配，越具体越靠前。
    // gpt-5.3-codex 暂未公开价格，临时按 gpt-5.2-codex 计费。
    ("gpt-5.3-codex", 0.00175, 0.000175, 0.014),
    ("gpt-5.2-codex", 0.00175, 0.000175, 0.014),
    ("gpt-5.2", 0.00175, 0.000175, 0.014),
    ("gpt-5.1-codex-mini", 0.00025, 0.000025, 0.002),
    ("gpt-5.1-codex-max", 0.00125, 0.000125, 0.01),
    ("gpt-5.1-codex", 0.00125, 0.000125, 0.01),
    ("gpt-5.1", 0.00125, 0.000125, 0.01),
    ("gpt-5-codex", 0.00125, 0.000125, 0.01),
    ("gpt-5", 0.00125, 0.000125, 0.01),
    // 兼容旧模型：缓存输入按输入同价处理，保持历史口径稳定。
    ("gpt-4.1", 0.002, 0.002, 0.008),
    ("gpt-4o", 0.0025, 0.0025, 0.01),
    ("gpt-4", 0.03, 0.03, 0.06),
    ("claude-3-7", 0.003, 0.003, 0.015),
    ("claude-3-5", 0.003, 0.003, 0.015),
    ("claude-3", 0.003, 0.003, 0.015),
];

fn estimate_cost_usd(
    model: Option<&str>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
) -> f64 {
    let normalized = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let Some(normalized) = normalized else {
        return 0.0;
    };
    let Some((_, in_per_1k, cached_in_per_1k, out_per_1k)) = MODEL_PRICE_PER_1K_TOKENS
        .iter()
        .find(|(prefix, _, _, _)| normalized.starts_with(prefix))
    else {
        return 0.0;
    };
    let in_tokens_total = input_tokens.unwrap_or(0).max(0) as f64;
    let cached_in_tokens = (cached_input_tokens.unwrap_or(0).max(0) as f64).min(in_tokens_total);
    let billable_in_tokens = (in_tokens_total - cached_in_tokens).max(0.0);
    let out_tokens = output_tokens.unwrap_or(0).max(0) as f64;
    (billable_in_tokens / 1000.0) * in_per_1k
        + (cached_in_tokens / 1000.0) * cached_in_per_1k
        + (out_tokens / 1000.0) * out_per_1k
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
    let total_tokens = normalize_token(usage.total_tokens);
    let reasoning_output_tokens = normalize_token(usage.reasoning_output_tokens);
    let created_at = now_ts();
    let estimated_cost_usd =
        estimate_cost_usd(model, input_tokens, cached_input_tokens, output_tokens);
    let success = status_code
        .map(|status| (200..300).contains(&status))
        .unwrap_or(false);
    let input_zero_or_missing = input_tokens.unwrap_or(0) == 0;
    let cached_zero_or_missing = cached_input_tokens.unwrap_or(0) == 0;
    let output_zero_or_missing = output_tokens.unwrap_or(0) == 0;
    let total_zero_or_missing = total_tokens.unwrap_or(0) == 0;
    let reasoning_zero_or_missing = reasoning_output_tokens.unwrap_or(0) == 0;
    if success
        && is_inference_path(request_path)
        && input_zero_or_missing
        && cached_zero_or_missing
        && output_zero_or_missing
        && total_zero_or_missing
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
        total_tokens: None,
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
        total_tokens,
        reasoning_output_tokens,
        estimated_cost_usd: Some(estimated_cost_usd),
        created_at,
    }) {
        eprintln!("insert request_token_stats failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::estimate_cost_usd;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn estimate_cost_matches_openai_gpt5_family_prices() {
        // 基准样本：输入 1000，缓存 200，输出 500
        // gpt-5 系列：输入 1.25/M，缓存 0.125/M，输出 10/M
        // => 非缓存输入 800*0.00125/1000 + 缓存 200*0.000125/1000 + 输出 500*0.01/1000
        // => 0.006025
        let expected = 0.006025_f64;
        let models = [
            "gpt-5",
            "gpt-5-codex",
            "gpt-5.1",
            "gpt-5.1-codex",
            "gpt-5.1-codex-max",
        ];
        for model in models {
            let actual =
                estimate_cost_usd(Some(model), Some(1000), Some(200), Some(500));
            assert_close(actual, expected);
        }
    }

    #[test]
    fn estimate_cost_matches_openai_gpt5_mini_and_52_prices() {
        // mini：输入 0.25/M，缓存 0.025/M，输出 2/M
        // 样本同上 => 0.001205
        let mini_cost = estimate_cost_usd(
            Some("gpt-5.1-codex-mini"),
            Some(1000),
            Some(200),
            Some(500),
        );
        assert_close(mini_cost, 0.001205);

        // 5.2：输入 1.75/M，缓存 0.175/M，输出 14/M
        // 样本同上 => 0.008435
        let v52_models = ["gpt-5.2", "gpt-5.2-codex"];
        for model in v52_models {
            let actual =
                estimate_cost_usd(Some(model), Some(1000), Some(200), Some(500));
            assert_close(actual, 0.008435);
        }
    }

    #[test]
    fn estimate_cost_uses_cached_input_rate_for_gpt_5_1_codex() {
        // 非缓存输入 800k * 1.25 + 缓存输入 200k * 0.125 + 输出 500k * 10
        // 期望：1 + 0.025 + 5 = 6.025 USD
        let actual = estimate_cost_usd(
            Some("gpt-5.1-codex"),
            Some(1_000_000),
            Some(200_000),
            Some(500_000),
        );
        assert_close(actual, 6.025);
    }

    #[test]
    fn estimate_cost_falls_back_gpt_5_3_codex_to_gpt_5_2_codex_price() {
        // gpt-5.3-codex 暂按 gpt-5.2-codex：输入 1.75 + 输出 14.00
        let actual = estimate_cost_usd(
            Some("gpt-5.3-codex"),
            Some(1_000_000),
            Some(0),
            Some(1_000_000),
        );
        assert_close(actual, 15.75);
    }
}
