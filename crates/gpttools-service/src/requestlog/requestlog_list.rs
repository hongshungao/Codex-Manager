use gpttools_core::rpc::types::RequestLogSummary;

use crate::storage_helpers::open_storage;

pub(crate) fn read_request_logs(query: Option<String>, limit: Option<i64>) -> Vec<RequestLogSummary> {
    let storage = match open_storage() {
        Some(storage) => storage,
        None => return Vec::new(),
    };
    let logs = match storage.list_request_logs(query.as_deref(), limit.unwrap_or(200)) {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };
    logs.into_iter()
        .map(|item| RequestLogSummary {
            key_id: item.key_id,
            request_path: item.request_path,
            method: item.method,
            model: item.model,
            reasoning_effort: item.reasoning_effort,
            upstream_url: item.upstream_url,
            status_code: item.status_code,
            error: item.error,
            created_at: item.created_at,
        })
        .collect()
}
