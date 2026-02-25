use codexmanager_core::rpc::types::{InitializeResult, JsonRpcRequest, JsonRpcResponse};
use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;
use serde_json::Value;

use crate::storage_helpers;

mod account;
mod apikey;
mod gateway;
mod requestlog;
mod usage;

pub(super) fn response(req: &JsonRpcRequest, result: Value) -> JsonRpcResponse {
    JsonRpcResponse { id: req.id, result }
}

pub(super) fn as_json<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub(super) fn str_param<'a>(req: &'a JsonRpcRequest, key: &str) -> Option<&'a str> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
}

pub(super) fn string_param(req: &JsonRpcRequest, key: &str) -> Option<String> {
    str_param(req, key).map(|v| v.to_string())
}

pub(super) fn i64_param(req: &JsonRpcRequest, key: &str) -> Option<i64> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_i64())
}

pub(super) fn bool_param(req: &JsonRpcRequest, key: &str) -> Option<bool> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_bool())
}

pub(super) fn ok_result() -> Value {
    serde_json::json!({ "ok": true })
}

pub(super) fn ok_or_error(result: Result<(), String>) -> Value {
    match result {
        Ok(_) => ok_result(),
        Err(err) => serde_json::json!({ "ok": false, "error": err }),
    }
}

pub(super) fn value_or_error<T: Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => as_json(value),
        Err(err) => serde_json::json!({ "error": err }),
    }
}

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
            server_name: "codexmanager-service".to_string(),
            version: codexmanager_core::core_version().to_string(),
        };
        return response(&req, as_json(result));
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
    if let Some(resp) = gateway::try_handle(&req) {
        return resp;
    }
    if let Some(resp) = requestlog::try_handle(&req) {
        return resp;
    }

    response(&req, serde_json::json!({"error": "unknown_method"}))
}
