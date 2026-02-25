use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use rand::RngCore;
use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

mod http;
#[path = "storage/storage_helpers.rs"]
mod storage_helpers;
#[path = "account/account_availability.rs"]
mod account_availability;
#[path = "account/account_status.rs"]
mod account_status;
#[path = "account/account_list.rs"]
mod account_list;
#[path = "account/account_delete.rs"]
mod account_delete;
#[path = "account/account_update.rs"]
mod account_update;
#[path = "apikey/apikey_list.rs"]
mod apikey_list;
#[path = "apikey/apikey_create.rs"]
mod apikey_create;
#[path = "apikey/apikey_delete.rs"]
mod apikey_delete;
#[path = "apikey/apikey_disable.rs"]
mod apikey_disable;
#[path = "apikey/apikey_enable.rs"]
mod apikey_enable;
#[path = "apikey/apikey_models.rs"]
mod apikey_models;
#[path = "apikey/apikey_profile.rs"]
mod apikey_profile;
#[path = "apikey/apikey_update_model.rs"]
mod apikey_update_model;
#[path = "apikey/apikey_read_secret.rs"]
mod apikey_read_secret;
#[path = "auth/auth_login.rs"]
mod auth_login;
#[path = "auth/auth_callback.rs"]
mod auth_callback;
#[path = "auth/auth_tokens.rs"]
mod auth_tokens;
#[path = "usage/usage_read.rs"]
mod usage_read;
#[path = "usage/usage_list.rs"]
mod usage_list;
#[path = "usage/usage_scheduler.rs"]
mod usage_scheduler;
#[path = "usage/usage_http.rs"]
mod usage_http;
#[path = "usage/usage_account_meta.rs"]
mod usage_account_meta;
#[path = "usage/usage_keepalive.rs"]
mod usage_keepalive;
#[path = "usage/usage_snapshot_store.rs"]
mod usage_snapshot_store;
#[path = "usage/usage_token_refresh.rs"]
mod usage_token_refresh;
#[path = "usage/usage_refresh.rs"]
mod usage_refresh;
mod gateway;
#[path = "requestlog/requestlog_list.rs"]
mod requestlog_list;
#[path = "requestlog/requestlog_clear.rs"]
mod requestlog_clear;
#[path = "requestlog/requestlog_today_summary.rs"]
mod requestlog_today_summary;
mod reasoning_effort;
mod rpc_dispatch;

pub const DEFAULT_ADDR: &str = "localhost:48760";

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static RPC_AUTH_TOKEN: OnceLock<String> = OnceLock::new();

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
    gateway::reload_runtime_config_from_env();
    // 中文注释：one-shot 入口也先尝试建表，避免未初始化数据库在首个 RPC 就触发读写失败。
    if let Err(err) = storage_helpers::initialize_storage() {
        log::warn!("storage startup init skipped: {}", err);
    }
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    let addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "server addr missing"))?;
    let join = thread::spawn(move || {
        if let Some(request) = server.incoming_requests().next() {
            crate::http::backend_router::handle_backend_request(request);
        }
    });
    Ok(ServerHandle { addr, join })
}

pub fn start_server(addr: &str) -> std::io::Result<()> {
    gateway::reload_runtime_config_from_env();
    // 中文注释：启动阶段先做一次显式初始化；不放在每次 open_storage 里是为避免高频 RPC 重复执行迁移检查。
    if let Err(err) = storage_helpers::initialize_storage() {
        log::warn!("storage startup init skipped: {}", err);
    }
    usage_refresh::ensure_usage_polling();
    usage_refresh::ensure_gateway_keepalive();
    http::server::start_http(addr)
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

pub fn clear_shutdown_flag() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
}

fn build_rpc_auth_token() -> String {
    if let Ok(raw) = std::env::var("CODEXMANAGER_RPC_TOKEN") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    std::env::set_var("CODEXMANAGER_RPC_TOKEN", &token);
    token
}

pub fn rpc_auth_token() -> &'static str {
    RPC_AUTH_TOKEN.get_or_init(build_rpc_auth_token).as_str()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub fn rpc_auth_token_matches(candidate: &str) -> bool {
    let expected = rpc_auth_token();
    constant_time_eq(expected.as_bytes(), candidate.trim().as_bytes())
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
    rpc_dispatch::handle_request(req)
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









