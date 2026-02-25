use codexmanager_core::rpc::types::RequestLogSummary;

use crate::storage_helpers::open_storage;

pub(crate) fn read_request_logs(
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<RequestLogSummary>, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let logs = storage
        .list_request_logs(query.as_deref(), limit.unwrap_or(200))
        .map_err(|err| format!("list request logs failed: {err}"))?;
    Ok(logs
        .into_iter()
        .map(|item| RequestLogSummary {
            key_id: item.key_id,
            account_id: item.account_id,
            request_path: item.request_path,
            method: item.method,
            model: item.model,
            reasoning_effort: item.reasoning_effort,
            upstream_url: item.upstream_url,
            status_code: item.status_code,
            input_tokens: item.input_tokens,
            cached_input_tokens: item.cached_input_tokens,
            output_tokens: item.output_tokens,
            reasoning_output_tokens: item.reasoning_output_tokens,
            estimated_cost_usd: item.estimated_cost_usd,
            error: item.error,
            created_at: item.created_at,
        })
        .collect())
}
