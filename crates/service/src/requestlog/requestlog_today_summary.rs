use chrono::{Duration, Local, LocalResult, TimeZone};
use codexmanager_core::rpc::types::RequestLogTodaySummaryResult;

use crate::storage_helpers::open_storage;

fn local_day_bounds_ts() -> Result<(i64, i64), String> {
    let now = Local::now();
    let today = now.date_naive();
    let start_naive = today
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "build local start-of-day failed".to_string())?;
    let tomorrow_naive = (today + Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "build local end-of-day failed".to_string())?;

    let start = match Local.from_local_datetime(&start_naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(a, b) => a.timestamp().min(b.timestamp()),
        LocalResult::None => now.timestamp(),
    };
    let end = match Local.from_local_datetime(&tomorrow_naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(a, b) => a.timestamp().max(b.timestamp()),
        LocalResult::None => start + 24 * 60 * 60,
    };
    Ok((start, end.max(start)))
}

pub(crate) fn read_requestlog_today_summary() -> Result<RequestLogTodaySummaryResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let (start_ts, end_ts) = local_day_bounds_ts()?;
    let summary = storage
        .summarize_request_logs_between(start_ts, end_ts)
        .map_err(|err| format!("summarize request logs failed: {err}"))?;
    let input_tokens = summary.input_tokens.max(0);
    let cached_input_tokens = summary.cached_input_tokens.max(0);
    let output_tokens = summary.output_tokens.max(0);
    let reasoning_output_tokens = summary.reasoning_output_tokens.max(0);
    let non_cached_input_tokens = input_tokens.saturating_sub(cached_input_tokens);
    Ok(RequestLogTodaySummaryResult {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        today_tokens: non_cached_input_tokens.saturating_add(output_tokens),
        estimated_cost: summary.estimated_cost_usd.max(0.0),
    })
}
