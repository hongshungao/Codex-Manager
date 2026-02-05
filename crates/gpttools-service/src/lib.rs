use gpttools_core::rpc::types::{
    AccountListResult, ApiKeyListResult, InitializeResult, JsonRpcRequest, JsonRpcResponse,
    UsageListResult, UsageReadResult,
};
use gpttools_core::storage::{now_ts, Event, Storage};
use serde_json::Value;
use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

mod http;
mod storage_helpers;
mod account_availability;
mod account_status;
mod account_list;
mod account_delete;
mod account_update;
mod apikey_list;
mod apikey_create;
mod apikey_delete;
mod apikey_disable;
mod auth_login;
mod auth_callback;
mod auth_tokens;
mod usage_read;
mod usage_list;
mod usage_refresh;
mod gateway;

pub const DEFAULT_ADDR: &str = "localhost:48760";

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub struct ServerHandle {
    pub addr: String,
    join: thread::JoinHandle<()>,
}

impl ServerHandle {
    pub fn join(self) {
        let _ = self.join.join();
    }
}

pub fn start_one_shot_server() -> std::io::Result<ServerHandle> {
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    let addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "server addr missing"))?;
    let join = thread::spawn(move || {
        if let Some(request) = server.incoming_requests().next() {
            crate::http::server::route_request(request);
        }
    });
    Ok(ServerHandle { addr, join })
}

pub fn start_server(addr: &str) -> std::io::Result<()> {
    usage_refresh::ensure_usage_polling();
    http::server::start_http(addr)
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

pub fn clear_shutdown_flag() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn request_shutdown(addr: &str) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    // Best-effort wakeups for both IPv4 and IPv6 loopback so whichever listener is active exits.
    let _ = send_shutdown_request(addr);
    if let Some(port) = addr.trim().strip_prefix("localhost:") {
        let _ = send_shutdown_request(&format!("127.0.0.1:{port}"));
        let _ = send_shutdown_request(&format!("[::1]:{port}"));
    }
}

fn send_shutdown_request(addr: &str) -> std::io::Result<()> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Ok(());
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);
    let mut stream = TcpStream::connect(addr)?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let request = format!(
        "GET /__shutdown HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    Ok(())
}

pub(crate) fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => {
            if let Ok(path) = std::env::var("GPTTOOLS_DB_PATH") {
                if let Ok(storage) = Storage::open(path) {
                    let _ = storage.init();
                    let _ = storage.insert_event(&Event {
                        account_id: None,
                        event_type: "initialize".to_string(),
                        message: "service initialized".to_string(),
                        created_at: now_ts(),
                    });
                }
            }
            let result = InitializeResult {
                server_name: "gpttools-service".to_string(),
                version: gpttools_core::core_version().to_string(),
            };
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "account/list" => {
            let items = account_list::read_accounts();
            let result = AccountListResult { items };
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "account/delete" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = match account_delete::delete_account(account_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        "account/update" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sort = req
                .params
                .as_ref()
                .and_then(|v| v.get("sort"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let result = match account_update::update_account_sort(account_id, sort) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        "account/login/start" => {
            let login_type = req
                .params
                .as_ref()
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("chatgpt");
            let open_browser = req
                .params
                .as_ref()
                .and_then(|v| v.get("openBrowser"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let note = req
                .params
                .as_ref()
                .and_then(|v| v.get("note"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let tags = req
                .params
                .as_ref()
                .and_then(|v| v.get("tags"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let group_name = req
                .params
                .as_ref()
                .and_then(|v| v.get("groupName"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let workspace_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("workspaceId"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .and_then(|v| if v.trim().is_empty() { None } else { Some(v) });
            let result = match auth_login::login_start(
                login_type,
                open_browser,
                note,
                tags,
                group_name,
                workspace_id,
            ) {
                Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
                Err(err) => serde_json::json!({ "error": err }),
            };
            JsonRpcResponse {
                id: req.id,
                result,
            }
        }
        "account/login/status" => {
            let login_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("loginId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = auth_login::login_status(login_id);
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "account/login/complete" => {
            let state = req
                .params
                .as_ref()
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = req
                .params
                .as_ref()
                .and_then(|v| v.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let redirect_uri = req
                .params
                .as_ref()
                .and_then(|v| v.get("redirectUri"))
                .and_then(|v| v.as_str());
            if state.is_empty() || code.is_empty() {
                JsonRpcResponse {
                    id: req.id,
                    result: serde_json::json!({"ok": false, "error": "missing code/state"}),
                }
            } else {
                let result = match auth_tokens::complete_login_with_redirect(
                    state,
                    code,
                    redirect_uri,
                ) {
                    Ok(_) => serde_json::json!({ "ok": true }),
                    Err(err) => serde_json::json!({ "ok": false, "error": err }),
                };
                JsonRpcResponse { id: req.id, result }
            }
        }
        "apikey/list" => {
            let result = ApiKeyListResult {
                items: apikey_list::read_api_keys(),
            };
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "apikey/create" => {
            let name = req
                .params
                .as_ref()
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let result = match apikey_create::create_api_key(name) {
                Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
                Err(err) => serde_json::json!({ "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        "apikey/delete" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = match apikey_delete::delete_api_key(key_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        "apikey/disable" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = match apikey_disable::disable_api_key(key_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        "account/usage/read" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str());
            let result = UsageReadResult {
                snapshot: usage_read::read_usage_snapshot(account_id),
            };
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "account/usage/list" => {
            let result = UsageListResult {
                items: usage_list::read_usage_snapshots(),
            };
            JsonRpcResponse {
                id: req.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            }
        }
        "account/usage/refresh" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str());
            let result = match account_id {
                Some(account_id) => usage_refresh::refresh_usage_for_account(account_id),
                None => usage_refresh::refresh_usage_for_all_accounts(),
            };
            let result = match result {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            };
            JsonRpcResponse { id: req.id, result }
        }
        _ => JsonRpcResponse {
            id: req.id,
            result: serde_json::json!({"error": "unknown_method"}),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_complete_requires_params() {
        let req = JsonRpcRequest {
            id: 1,
            method: "account/login/complete".to_string(),
            params: None,
        };
        let resp = handle_request(req);
        let err = resp
            .result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(err.contains("missing"));

        let req = JsonRpcRequest {
            id: 2,
            method: "account/login/complete".to_string(),
            params: Some(serde_json::json!({ "code": "x" })),
        };
        let resp = handle_request(req);
        let err = resp
            .result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(err.contains("missing"));

        let req = JsonRpcRequest {
            id: 3,
            method: "account/login/complete".to_string(),
            params: Some(serde_json::json!({ "state": "y" })),
        };
        let resp = handle_request(req);
        let err = resp
            .result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(err.contains("missing"));
    }
}

