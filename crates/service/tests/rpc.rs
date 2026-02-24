use codexmanager_core::rpc::types::JsonRpcRequest;
use std::io::{Read, Write};
use std::net::TcpStream;

fn post_rpc_raw(addr: &str, body: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect server");
    let mut request = format!("POST /rpc HTTP/1.1\r\nHost: {addr}\r\n");
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
    stream.write_all(request.as_bytes()).expect("write");
    stream.shutdown(std::net::Shutdown::Write).ok();

    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");
    let status = buf
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("status");
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn post_rpc(addr: &str, body: &str) -> serde_json::Value {
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, body) = post_rpc_raw(
        addr,
        body,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
        ],
    );
    assert_eq!(status, 200, "unexpected status {status}: {body}");
    serde_json::from_str(&body).expect("parse response")
}

#[test]
fn rpc_initialize_roundtrip() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 1,
        method: "initialize".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert_eq!(result.get("server_name").unwrap(), "codexmanager-service");
}

#[test]
fn rpc_account_list_empty() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 2,
        method: "account/list".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    if let Some(items) = result.get("items").and_then(|value| value.as_array()) {
        assert!(items.is_empty());
        return;
    }
    let err = result.get("error").and_then(|value| value.as_str()).unwrap_or("");
    assert!(!err.is_empty(), "expected items or explicit error, got: {result}");
}

#[test]
fn rpc_login_start_returns_url() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 3,
        method: "account/login/start".to_string(),
        params: Some(serde_json::json!({"type": "chatgpt", "openBrowser": false})),
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    let auth_url = result.get("authUrl").and_then(|v| v.as_str()).unwrap();
    let login_id = result.get("loginId").and_then(|v| v.as_str()).unwrap();
    assert!(auth_url.contains("oauth/authorize"));
    assert!(!login_id.is_empty());
}

#[test]
fn rpc_usage_read_empty() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 4,
        method: "account/usage/read".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert!(result.get("snapshot").is_some());
}

#[test]
fn rpc_login_status_pending() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 5,
        method: "account/login/status".to_string(),
        params: Some(serde_json::json!({"loginId": "login-1"})),
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    assert!(result.get("status").is_some());
}

#[test]
fn rpc_usage_list_empty() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 6,
        method: "account/usage/list".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let v = post_rpc(&server.addr, &json);
    let result = v.get("result").expect("result");
    if let Some(items) = result.get("items").and_then(|value| value.as_array()) {
        assert!(items.is_empty());
        return;
    }
    let err = result.get("error").and_then(|value| value.as_str()).unwrap_or("");
    assert!(!err.is_empty(), "expected items or explicit error, got: {result}");
}

#[test]
fn rpc_rejects_missing_token() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 7,
        method: "initialize".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let (status, _) = post_rpc_raw(&server.addr, &json, &[("Content-Type", "application/json")]);
    assert_eq!(status, 401);
}

#[test]
fn rpc_rejects_cross_site_origin() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 8,
        method: "initialize".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, _) = post_rpc_raw(
        &server.addr,
        &json,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "https://evil.example"),
            ("Sec-Fetch-Site", "cross-site"),
        ],
    );
    assert_eq!(status, 403);
}

#[test]
fn rpc_accepts_loopback_origin() {
    let server = codexmanager_service::start_one_shot_server().expect("start server");

    let req = JsonRpcRequest {
        id: 9,
        method: "initialize".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let (status, body) = post_rpc_raw(
        &server.addr,
        &json,
        &[
            ("Content-Type", "application/json"),
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "http://localhost:5173"),
            ("Sec-Fetch-Site", "same-site"),
        ],
    );
    assert_eq!(status, 200, "unexpected status {status}: {body}");
}
