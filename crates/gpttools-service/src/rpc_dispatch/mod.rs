use gpttools_core::rpc::types::{InitializeResult, JsonRpcRequest, JsonRpcResponse};
use gpttools_core::storage::{now_ts, Event};

use crate::storage_helpers;

mod account;
mod apikey;
mod requestlog;
mod usage;

pub(crate) fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    if req.method == "initialize" {
        let _ = storage_helpers::initialize_storage();
        if let Some(storage) = storage_helpers::open_storage() {
            let _ = storage.insert_event(&Event {
                account_id: None,
                event_type: "initialize".to_string(),
                message: "service initialized".to_string(),
                created_at: now_ts(),
            });
        }
        let result = InitializeResult {
            server_name: "gpttools-service".to_string(),
            version: gpttools_core::core_version().to_string(),
        };
        return JsonRpcResponse {
            id: req.id,
            result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
        };
    }

    if let Some(resp) = account::try_handle(&req) {
        return resp;
    }
    if let Some(resp) = apikey::try_handle(&req) {
        return resp;
    }
    if let Some(resp) = usage::try_handle(&req) {
        return resp;
    }
    if let Some(resp) = requestlog::try_handle(&req) {
        return resp;
    }

    JsonRpcResponse {
        id: req.id,
        result: serde_json::json!({"error": "unknown_method"}),
    }
}
