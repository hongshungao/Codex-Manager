use codexmanager_core::rpc::types::ModelOption;
use codexmanager_core::storage::{now_ts, Account, ApiKey, Storage, Token};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::collections::HashMap;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

static ENV_LOCK: Mutex<()> = Mutex::new(());
static TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    // 中文注释：若某个测试 panic 导致锁被 poison，不应让后续测试直接二次失败。
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn new_test_dir(prefix: &str) -> PathBuf {
    // 中文注释：Windows 进程 ID 可能被复用；增加递增序号避免复用旧目录/旧 db 文件导致用例不稳定。
    let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(val) = &self.original {
            std::env::set_var(self.key, val);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn decode_chunked_body_if_needed(body: &str) -> String {
    let normalized = body.replace("\r\n", "\n");
    let bytes = normalized.as_bytes();
    let mut idx = 0usize;
    let mut out = Vec::new();
    let mut saw_chunk = false;

    while idx < bytes.len() {
        let size_end = match bytes[idx..].iter().position(|b| *b == b'\n') {
            Some(rel) => idx + rel,
            None => bytes.len(),
        };
        let size_text = std::str::from_utf8(&bytes[idx..size_end]).ok().map(str::trim);
        let Some(size_text) = size_text else {
            return normalized;
        };
        let Ok(size) = usize::from_str_radix(size_text, 16) else {
            return normalized;
        };
        saw_chunk = true;
        idx = if size_end < bytes.len() { size_end + 1 } else { size_end };
        if size == 0 {
            break;
        }
        if idx + size > bytes.len() {
            return normalized;
        }
        out.extend_from_slice(&bytes[idx..idx + size]);
        idx += size;
        if idx >= bytes.len() || bytes[idx] != b'\n' {
            return normalized;
        }
        idx += 1;
    }

    if !saw_chunk {
        return normalized;
    }
    String::from_utf8(out).unwrap_or(normalized)
}

fn post_http_raw(
    addr: &str,
    path: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> (u16, String) {
    let mut last_raw = String::new();
    for _ in 0..20 {
        let mut stream = TcpStream::connect(addr).expect("connect server");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut request =
            format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
        stream.write_all(request.as_bytes()).expect("write");

        let mut buf = String::new();
        stream.read_to_string(&mut buf).expect("read");
        if let Some(status) = buf
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok())
        {
            let body_raw = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let body = decode_chunked_body_if_needed(&body_raw);
            return (status, body);
        }
        last_raw = buf;
        thread::sleep(Duration::from_millis(50));
    }
    panic!("status parse failed, raw response: {last_raw:?}");
}

fn get_http_raw(addr: &str, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
    let mut last_raw = String::new();
    for _ in 0..20 {
        let mut stream = TcpStream::connect(addr).expect("connect server");
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut request =
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).expect("write");

        let mut buf = String::new();
        stream.read_to_string(&mut buf).expect("read");
        if let Some(status) = buf
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok())
        {
            let body_raw = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            let body = decode_chunked_body_if_needed(&body_raw);
            return (status, body);
        }
        last_raw = buf;
        thread::sleep(Duration::from_millis(50));
    }
    panic!("status parse failed, raw response: {last_raw:?}");
}

fn hash_platform_key_for_test(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[derive(Debug)]
struct CapturedUpstreamRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_http_request_once(stream: &mut TcpStream) -> CapturedUpstreamRequest {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));

    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let mut header_end = None;
    while header_end.is_none() {
        let read = stream.read(&mut buf).expect("read upstream headers");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..read]);
        header_end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|idx| idx + 4);
    }
    let header_end = header_end.expect("upstream header terminator");
    let header_text = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = header_text.split("\r\n").filter(|line| !line.is_empty());
    let request_line = lines.next().expect("upstream request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse::<usize>().unwrap_or(0);
            }
            headers.insert(name, value);
        }
    }

    while raw.len() < header_end + content_length {
        let read = stream.read(&mut buf).expect("read upstream body");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..read]);
    }
    let body_end = (header_end + content_length).min(raw.len());
    let body = raw[header_end..body_end].to_vec();

    CapturedUpstreamRequest {
        path,
        headers,
        body,
    }
}

fn start_mock_upstream_once(response_json: &str) -> (String, Receiver<CapturedUpstreamRequest>, thread::JoinHandle<()>) {
    start_mock_upstream_once_with_content_type(response_json, "application/json")
}

fn start_mock_upstream_once_with_content_type(
    response_body: &str,
    content_type: &str,
) -> (String, Receiver<CapturedUpstreamRequest>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let response = response_body.as_bytes().to_vec();
    let content_type = content_type.to_string();
    let (tx, rx) = mpsc::channel();

    let join = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept upstream");
        let captured = read_http_request_once(&mut stream);
        let _ = tx.send(captured);

        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream.write_all(header.as_bytes()).expect("write upstream status");
        stream.write_all(&response).expect("write upstream body");
        let _ = stream.flush();
    });

    (addr.to_string(), rx, join)
}

fn start_mock_upstream_sequence(
    responses: Vec<(u16, String)>,
) -> (String, Receiver<CapturedUpstreamRequest>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (tx, rx) = mpsc::channel();

    let join = thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept upstream");
            let captured = read_http_request_once(&mut stream);
            let _ = tx.send(captured);

            let body_bytes = body.as_bytes().to_vec();
            let header = format!(
                "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                status,
                body_bytes.len()
            );
            stream.write_all(header.as_bytes()).expect("write upstream status");
            stream
                .write_all(&body_bytes)
                .expect("write upstream response body");
            let _ = stream.flush();
        }
    });

    (addr.to_string(), rx, join)
}

fn start_mock_upstream_sequence_lenient(
    responses: Vec<(u16, String)>,
    idle_timeout: Duration,
) -> (String, Receiver<CapturedUpstreamRequest>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking listener");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (tx, rx) = mpsc::channel();

    let join = thread::spawn(move || {
        let mut idx = 0usize;
        let mut last_accept_at = Instant::now();
        while idx < responses.len() {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    last_accept_at = Instant::now();
                    let _ = stream.set_nonblocking(false);
                    let captured = read_http_request_once(&mut stream);
                    let _ = tx.send(captured);

                    let (status, body) = &responses[idx];
                    let body_bytes = body.as_bytes().to_vec();
                    let header = format!(
                        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status,
                        body_bytes.len()
                    );
                    stream.write_all(header.as_bytes()).expect("write upstream status");
                    stream
                        .write_all(&body_bytes)
                        .expect("write upstream response body");
                    let _ = stream.flush();
                    idx += 1;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_accept_at.elapsed() >= idle_timeout {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    (addr.to_string(), rx, join)
}

struct TestServer {
    addr: String,
    join: Option<thread::JoinHandle<()>>,
}

fn check_health(addr: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    if stream.read_to_string(&mut buf).is_err() {
        return false;
    }
    buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200")
}

impl TestServer {
    fn start() -> Self {
        codexmanager_service::clear_shutdown_flag();
        for _ in 0..10 {
            let probe = TcpListener::bind("127.0.0.1:0").expect("bind probe port");
            let port = probe.local_addr().expect("probe addr").port();
            drop(probe);

            let addr = format!("localhost:{port}");
            let addr_for_thread = addr.clone();
            let join = thread::spawn(move || {
                let _ = codexmanager_service::start_server(&addr_for_thread);
            });

            // 中文注释：前置代理与后端会串行启动；必须等 /health 成功，才能保证连到的是本测试服务而不是端口竞争者。
            for _ in 0..120 {
                if check_health(&addr) {
                    return Self {
                        addr,
                        join: Some(join),
                    };
                }
                if join.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = join.join();
        }
        panic!("server start timeout");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        codexmanager_service::request_shutdown(&self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        codexmanager_service::clear_shutdown_flag();
    }
}

#[test]
fn gateway_logs_invalid_api_key_error() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-logs");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let server = TestServer::start();
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello"}"#;
    let (status, _) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer invalid-platform-key"),
        ],
    );
    assert_eq!(status, 403);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init schema");
    let mut logs = Vec::new();
    for _ in 0..40 {
        logs = storage.list_request_logs(None, 100).expect("list request logs");
        if !logs.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let found = logs.iter().any(|item| {
        item.request_path == "/v1/responses"
            && item.status_code == Some(403)
            && item.input_tokens.is_none()
            && item.cached_input_tokens.is_none()
            && item.output_tokens.is_none()
            && item.total_tokens.is_none()
            && item.reasoning_output_tokens.is_none()
            && item.error.as_deref() == Some("invalid api key")
    });
    assert!(
        found,
        "expected invalid api key request to be logged, got {:?}",
        logs.iter()
            .map(|v| (&v.request_path, v.status_code, v.error.as_deref()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn gateway_tolerates_non_ascii_turn_metadata_header() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-logs-nonascii");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let server = TestServer::start();
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello"}"#;
    let metadata = r#"{"workspaces":{"D:\\MyComputer\\own\\GPTTeam相关\\CodexManager\\CodexManager":{"latest_git_commit_hash":"abc123"}}}"#;
    let (status, body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer invalid-platform-key"),
            ("x-codex-turn-metadata", metadata),
        ],
    );
    assert_eq!(status, 403, "response body: {body}");
}

#[test]
fn gateway_claude_protocol_end_to_end_uses_codex_headers() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-claude-e2e");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_response = serde_json::json!({
        "id": "resp_test_1",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "pong" }]
        }],
        "usage": { "input_tokens": 12, "output_tokens": 6, "total_tokens": 18 }
    });
    let upstream_response =
        serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&upstream_response);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_claude_e2e".to_string(),
            label: "claude-e2e".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_test".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_claude_e2e".to_string(),
            id_token: String::new(),
            access_token: "access_token_fallback".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_test".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_claude_e2e";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_claude_e2e".to_string(),
            name: Some("claude-e2e".to_string()),
            model_slug: None,
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            { "role": "user", "content": "你好" }
        ],
        "max_tokens": 64,
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");

    let value: serde_json::Value =
        serde_json::from_str(&gateway_body).expect("parse anthropic response");
    assert_eq!(value["type"], "message");
    assert_eq!(value["role"], "assistant");
    assert_eq!(value["content"][0]["type"], "text");
    assert_eq!(value["content"][0]["text"], "pong");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");

    assert_eq!(captured.path, "/backend-api/codex/responses");
    let authorization = captured
        .headers
        .get("authorization")
        .expect("authorization header");
    assert!(authorization.starts_with("Bearer "));
    assert!(!authorization.contains(platform_key));
    assert_eq!(
        captured.headers.get("accept").map(String::as_str),
        Some("text/event-stream")
    );
    assert_eq!(
        captured.headers.get("version").map(String::as_str),
        Some("0.101.0")
    );
    assert_eq!(
        captured.headers.get("openai-beta").map(String::as_str),
        Some("responses=experimental")
    );
    assert_eq!(
        captured.headers.get("originator").map(String::as_str),
        Some("codex_cli_rs")
    );
    assert_eq!(
        captured
            .headers
            .get("chatgpt-account-id")
            .map(String::as_str),
        Some("chatgpt_acc_test")
    );
    assert!(!captured.headers.contains_key("anthropic-version"));
    assert!(!captured.headers.contains_key("x-stainless-lang"));

    let upstream_payload: serde_json::Value =
        serde_json::from_slice(&captured.body).expect("parse upstream payload");
    assert_eq!(upstream_payload["model"], "gpt-5.3-codex");
    assert_eq!(upstream_payload["reasoning"]["effort"], "high");
    assert_eq!(upstream_payload["stream"], true);
    assert_eq!(upstream_payload["input"][0]["role"], "user");
    assert_eq!(upstream_payload["input"][0]["content"][0]["text"], "你好");

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_claude_e2e"), 20)
            .expect("list request logs");
        matched = logs
            .into_iter()
            .find(|item| item.request_path == "/v1/responses" && item.status_code == Some(200));
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("claude e2e request log");
    assert_eq!(log.input_tokens, Some(12));
    assert_eq!(log.cached_input_tokens, None);
    assert_eq!(log.output_tokens, Some(6));
    assert_eq!(log.total_tokens, Some(18));
    assert_eq!(log.reasoning_output_tokens, None);
}

#[test]
fn gateway_openai_stream_logs_cached_and_reasoning_tokens() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-openai-stream-usage");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_stream_usage_1\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":120,\"input_tokens_details\":{\"cached_tokens\":90},\"output_tokens\":18,\"total_tokens\":138,\"output_tokens_details\":{\"reasoning_tokens\":7}}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_once_with_content_type(upstream_sse, "text/event-stream");
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_openai_stream_usage".to_string(),
            label: "openai-stream-usage".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_openai_stream_usage".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_openai_stream_usage".to_string(),
            id_token: String::new(),
            access_token: "access_token_openai_stream_usage".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_openai_stream_usage".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_openai_stream_usage";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_openai_stream_usage".to_string(),
            name: Some("openai-stream-usage".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request_body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": true
    });
    let request_body = serde_json::to_string(&request_body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_openai_stream_usage"), 20)
            .expect("list request logs");
        matched = logs.into_iter().find(|item| item.request_path == "/v1/responses");
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("openai stream request log");
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.input_tokens, Some(120));
    assert_eq!(log.cached_input_tokens, Some(90));
    assert_eq!(log.output_tokens, Some(18));
    assert_eq!(log.total_tokens, Some(138));
    assert_eq!(log.reasoning_output_tokens, Some(7));
}

#[test]
fn gateway_openai_stream_usage_with_plain_content_type() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-openai-stream-plain-ct");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_stream_usage_plain_1\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":91,\"input_tokens_details\":{\"cached_tokens\":56},\"output_tokens\":14,\"total_tokens\":105,\"output_tokens_details\":{\"reasoning_tokens\":5}}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_once_with_content_type(upstream_sse, "text/plain; charset=utf-8");
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_openai_stream_plain_ct".to_string(),
            label: "openai-stream-plain-ct".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_openai_stream_plain_ct".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_openai_stream_plain_ct".to_string(),
            id_token: String::new(),
            access_token: "access_token_openai_stream_plain_ct".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_openai_stream_plain_ct".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_openai_stream_plain_ct";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_openai_stream_plain_ct".to_string(),
            name: Some("openai-stream-plain-ct".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request_body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": true
    });
    let request_body = serde_json::to_string(&request_body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_openai_stream_plain_ct"), 20)
            .expect("list request logs");
        matched = logs.into_iter().find(|item| item.request_path == "/v1/responses");
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("openai stream plain content-type request log");
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.input_tokens, Some(91));
    assert_eq!(log.cached_input_tokens, Some(56));
    assert_eq!(log.output_tokens, Some(14));
    assert_eq!(log.total_tokens, Some(105));
    assert_eq!(log.reasoning_output_tokens, Some(5));
}

#[test]
fn gateway_openai_non_stream_without_usage_keeps_tokens_null() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-openai-no-usage");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_response = serde_json::json!({
        "id": "resp_no_usage_1",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "pong" }]
        }]
    });
    let upstream_response =
        serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&upstream_response);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_openai_no_usage".to_string(),
            label: "openai-no-usage".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_openai_no_usage".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_openai_no_usage".to_string(),
            id_token: String::new(),
            access_token: "access_token_openai_no_usage".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_openai_no_usage".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_openai_no_usage";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_openai_no_usage".to_string(),
            name: Some("openai-no-usage".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request_body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": false
    });
    let request_body = serde_json::to_string(&request_body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");
    let value: serde_json::Value = serde_json::from_str(&gateway_body)
        .unwrap_or_else(|err| panic!("parse response failed: {err}; body={gateway_body}"));
    assert_eq!(value["output"][0]["content"][0]["text"], "pong");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_openai_no_usage"), 20)
            .expect("list request logs");
        matched = logs.into_iter().find(|item| item.request_path == "/v1/responses");
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("openai no usage request log");
    assert_eq!(log.status_code, Some(200), "log error: {:?}", log.error);
    assert_eq!(log.input_tokens, None);
    assert_eq!(log.cached_input_tokens, None);
    assert_eq!(log.output_tokens, None);
    assert_eq!(log.total_tokens, None);
    assert_eq!(log.reasoning_output_tokens, None);
}

#[test]
fn gateway_models_returns_cached_without_upstream() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-models-cache");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _upstream_guard = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        "http://127.0.0.1:1/backend-api/codex",
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    let platform_key = "pk_models_cache";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_models_cache".to_string(),
            name: Some("models-cache".to_string()),
            model_slug: None,
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let cached = vec![ModelOption {
        slug: "gpt-5.3-codex".to_string(),
        display_name: "GPT-5.3 Codex".to_string(),
    }];
    let items_json = serde_json::to_string(&cached).expect("serialize cached model options");
    storage
        .upsert_model_options_cache("default", &items_json, now_ts())
        .expect("upsert model options cache");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let (status, response_body) = get_http_raw(
        &server.addr,
        "/v1/models",
        &[("Authorization", &format!("Bearer {platform_key}"))],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let value: serde_json::Value =
        serde_json::from_str(&response_body).expect("parse models list response");
    let data = value
        .get("data")
        .and_then(|v| v.as_array())
        .expect("models list data array");
    assert!(
        data.iter()
            .any(|item| item.get("id").and_then(|v| v.as_str()) == Some("gpt-5.3-codex")),
        "models response missing cached id: {response_body}"
    );
}

#[test]
fn gateway_openai_fallback_strips_turn_state_headers() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-openai-fallback-strip-turn-state");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "rate limited",
            "type": "rate_limit_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_fallback_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(429, err_body), (200, ok_body)]);

    // Make the primary base look like a ChatGPT backend base so fallback logic is enabled,
    // while still routing to the local mock upstream server.
    let upstream_base = format!("http://{upstream_addr}/chatgpt.com/backend-api/codex");
    let fallback_base = format!("http://{upstream_addr}/v1");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);
    let _fallback_guard =
        EnvGuard::set("CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL", &fallback_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_fallback".to_string(),
            label: "fallback".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws_fallback".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_fallback".to_string(),
            id_token: String::new(),
            access_token: "access_token_fallback".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_fallback".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_openai_fallback_strip_turn_state";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_openai_fallback_strip_turn_state".to_string(),
            name: Some("fallback-strip-turn-state".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello","stream":false}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("x-codex-turn-state", "gAAA_dummy_turn_state_blob"),
            ("Conversation_id", "conv_dummy"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    // Primary attempt + fallback attempt should both be captured.
    let first = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive primary upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive fallback upstream request");
    upstream_join.join().expect("join mock upstream");

    assert!(
        first.headers.contains_key("x-codex-turn-state"),
        "primary attempt should forward turn_state for same-account flow"
    );

    assert_eq!(second.path, "/v1/responses");
    assert!(
        !second.headers.contains_key("x-codex-turn-state"),
        "fallback attempt must strip org-scoped turn_state to avoid invalid_encrypted_content"
    );
    assert!(
        !second.headers.contains_key("conversation_id"),
        "fallback attempt must strip conversation_id when stripping session affinity"
    );
    assert!(
        second.headers.contains_key("session_id"),
        "fallback attempt should still send a session_id"
    );
}

#[test]
fn gateway_cpa_no_cookie_header_mode_suppresses_affinity_headers_on_responses() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-cpa-no-cookie-header-mode");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _mode_guard = EnvGuard::set("CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE", "1");
    let _cookie_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_COOKIE", "cf_clearance=still_present");

    let upstream_response = serde_json::json!({
        "id": "resp_cpa_mode",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let upstream_response =
        serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&upstream_response);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_cpa_no_cookie".to_string(),
            label: "cpa-mode".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_cpa_mode".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_cpa_no_cookie".to_string(),
            id_token: String::new(),
            access_token: "access_token_cpa_mode".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_cpa_mode".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_cpa_no_cookie";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_cpa_no_cookie".to_string(),
            name: Some("cpa-no-cookie".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello","stream":false}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("x-codex-turn-state", "gAAA_dummy_turn_state_blob"),
            ("Conversation_id", "conv_dummy"),
            ("Session_id", "sess_dummy"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");

    assert_eq!(captured.path, "/backend-api/codex/responses");
    assert!(!captured.headers.contains_key("x-codex-turn-state"));
    assert!(!captured.headers.contains_key("conversation_id"));
    assert!(!captured.headers.contains_key("openai-beta"));
    assert!(!captured.headers.contains_key("chatgpt-account-id"));
    assert_eq!(
        captured.headers.get("cookie").map(String::as_str),
        Some("cf_clearance=still_present")
    );
    assert_eq!(
        captured.headers.get("session_id").map(String::as_str),
        Some("sess_dummy")
    );
    let upstream_payload: serde_json::Value =
        serde_json::from_slice(&captured.body).expect("parse upstream payload");
    assert_eq!(upstream_payload["stream"], true);
    assert!(upstream_payload["input"].is_array());
    assert_eq!(upstream_payload["input"][0]["type"], "message");
    assert_eq!(upstream_payload["input"][0]["role"], "user");
    assert_eq!(upstream_payload["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(upstream_payload["input"][0]["content"][0]["text"], "hello");
}

#[test]
fn gateway_cpa_no_cookie_header_mode_binds_prompt_cache_key_to_session_only() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-cpa-no-cookie-prompt-cache");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _mode_guard = EnvGuard::set("CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE", "1");
    let _cookie_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_COOKIE", "");

    let upstream_response = serde_json::json!({
        "id": "resp_cpa_prompt_cache",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let upstream_response =
        serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&upstream_response);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_cpa_prompt_cache".to_string(),
            label: "cpa-prompt-cache".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_cpa_prompt_cache".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_cpa_prompt_cache".to_string(),
            id_token: String::new(),
            access_token: "access_token_cpa_prompt_cache".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_cpa_prompt_cache".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_cpa_prompt_cache";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_cpa_prompt_cache".to_string(),
            name: Some("cpa-prompt-cache".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","prompt_cache_key":"cache_anchor_123","input":"hello","stream":false}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("Session_id", "legacy_session_should_be_overridden"),
            ("Conversation_id", "legacy_conversation_should_be_overridden"),
            ("x-codex-turn-state", "legacy_turn_state_should_be_dropped"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");

    assert_eq!(
        captured.headers.get("session_id").map(String::as_str),
        Some("cache_anchor_123")
    );
    assert!(!captured.headers.contains_key("conversation_id"));
    assert!(!captured.headers.contains_key("x-codex-turn-state"));
    assert!(!captured.headers.contains_key("openai-beta"));
    assert!(!captured.headers.contains_key("chatgpt-account-id"));
}

#[test]
fn gateway_cpa_no_cookie_header_mode_skips_post_retries_on_404() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-cpa-no-cookie-no-post-retry");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _mode_guard = EnvGuard::set("CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE", "1");
    let _cookie_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_COOKIE", "");

    let err_body = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let ok_body = serde_json::json!({
        "id": "resp_should_not_be_hit",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "unexpected" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence_lenient(
        vec![
            (
                404,
                serde_json::to_string(&err_body).expect("serialize 404 body"),
            ),
            (
                200,
                serde_json::to_string(&ok_body).expect("serialize 200 body"),
            ),
        ],
        Duration::from_millis(350),
    );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_cpa_no_retry".to_string(),
            label: "cpa-no-retry".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_cpa_no_retry".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_cpa_no_retry".to_string(),
            id_token: String::new(),
            access_token: "access_token_cpa_no_retry".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_cpa_no_retry".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_cpa_no_retry";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_cpa_no_retry".to_string(),
            name: Some("cpa-no-retry".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello","stream":false}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();

    assert_eq!(
        status, 404,
        "cpa no-cookie mode should keep first 404 response without post retries, body: {response_body}"
    );

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    assert_eq!(first.path, "/backend-api/codex/responses");

    let second = upstream_rx.recv_timeout(Duration::from_millis(300));
    assert!(
        second.is_err(),
        "unexpected second upstream request in cpa no-cookie mode: {:?}",
        second.ok().map(|item| item.path)
    );

    upstream_join.join().expect("join upstream");
}

#[test]
fn gateway_stateless_retry_strips_encrypted_content_on_invalid_encrypted_content() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-strip-encrypted-content-on-400");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "The encrypted content gAAA_test could not be verified. Reason: Encrypted content organization_id did not match the target organization.",
            "type": "invalid_request_error",
            "code": "invalid_encrypted_content"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_retry_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(400, err_body), (200, ok_body)]);

    let upstream_base = format!("http://{upstream_addr}/v1");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_retry_encrypted_content".to_string(),
            label: "retry".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws_retry".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_retry_encrypted_content".to_string(),
            id_token: String::new(),
            access_token: "access_token_retry".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_retry".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_retry_strip_encrypted_content";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_retry_strip_encrypted_content".to_string(),
            name: Some("retry-strip-encrypted-content".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello","stream":false,"encrypted_content":"gAAA_test_payload"}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("x-codex-turn-state", "gAAA_dummy_turn_state_blob"),
            ("Conversation_id", "conv_dummy"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second upstream request");
    upstream_join.join().expect("join upstream");

    assert!(first.headers.contains_key("x-codex-turn-state"));
    assert!(first.headers.contains_key("conversation_id"));
    let first_body: serde_json::Value =
        serde_json::from_slice(&first.body).expect("parse first request body");
    assert_eq!(
        first_body
            .get("encrypted_content")
            .and_then(|v| v.as_str()),
        Some("gAAA_test_payload")
    );

    assert!(!second.headers.contains_key("x-codex-turn-state"));
    assert!(!second.headers.contains_key("conversation_id"));
    let second_body: serde_json::Value =
        serde_json::from_slice(&second.body).expect("parse second request body");
    assert!(
        second_body.get("encrypted_content").is_none(),
        "stateless retry must drop org-scoped encrypted_content field"
    );
}

#[test]
fn gateway_claude_failover_cross_workspace_strips_session_affinity_headers() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-claude-strip-cross-workspace");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_strip_cross_workspace_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    // A 404 can trigger alternate-path + stateless retries before failover. Force those retries to
    // also 404 so the gateway actually fails over to wsB.
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body),
        (200, ok_body),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    storage
        .insert_account(&Account {
            id: "acc_ws_a".to_string(),
            label: "ws-a".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("wsA".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account wsA");
    storage
        .insert_token(&Token {
            account_id: "acc_ws_a".to_string(),
            id_token: String::new(),
            access_token: "access_token_ws_a".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_ws_a".to_string()),
            last_refresh: now,
        })
        .expect("insert token wsA");

    storage
        .insert_account(&Account {
            id: "acc_ws_b".to_string(),
            label: "ws-b".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("wsB".to_string()),
            group_name: None,
            sort: 2,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account wsB");
    storage
        .insert_token(&Token {
            account_id: "acc_ws_b".to_string(),
            id_token: String::new(),
            access_token: "access_token_ws_b".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_ws_b".to_string()),
            last_refresh: now,
        })
        .expect("insert token wsB");

    let platform_key = "pk_strip_cross_workspace";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_strip_cross_workspace".to_string(),
            name: Some("strip-cross-workspace".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hello" }],
        "metadata": { "user_id": "user_strip_cross_workspace" },
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
            ("x-codex-turn-state", "turn_state_cross_ws"),
            ("conversation_id", "conv_cross_ws"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let mut captured = Vec::new();
    for idx in 0..5 {
        captured.push(
            upstream_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|_| panic!("receive upstream request {idx}")),
        );
    }
    upstream_join.join().expect("join upstream");

    let ws_a_stateful = captured
        .iter()
        .find(|req| {
            req.headers
                .get("chatgpt-account-id")
                .map(String::as_str)
                == Some("wsA")
                && req.headers.contains_key("x-codex-turn-state")
        })
        .expect("expected wsA stateful upstream request");
    let ws_b = captured
        .iter()
        .find(|req| {
            req.headers
                .get("chatgpt-account-id")
                .map(String::as_str)
                == Some("wsB")
        })
        .expect("expected wsB upstream request");

    assert_eq!(
        ws_a_stateful
            .headers
            .get("x-codex-turn-state")
            .map(String::as_str),
        Some("turn_state_cross_ws")
    );
    assert_eq!(
        ws_a_stateful
            .headers
            .get("conversation_id")
            .map(String::as_str),
        Some("conv_cross_ws")
    );
    assert!(
        ws_a_stateful
            .headers
            .get("authorization")
            .map(|v| v.contains("access_token_ws_a"))
            .unwrap_or(false),
        "wsA upstream authorization missing expected bearer token"
    );

    assert!(!ws_b.headers.contains_key("x-codex-turn-state"));
    assert!(!ws_b.headers.contains_key("conversation_id"));
    assert!(
        ws_b.headers
            .get("authorization")
            .map(|v| v.contains("access_token_ws_b"))
            .unwrap_or(false),
        "wsB upstream authorization missing expected bearer token"
    );
}

#[test]
fn gateway_claude_failover_same_workspace_preserves_session_affinity_headers() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-claude-strip-same-workspace");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_strip_same_workspace_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    // A 404 can trigger alternate-path + stateless retries before failover. Force those retries to
    // also 404 so the gateway actually fails over to the 2nd account (same workspace scope).
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body),
        (200, ok_body),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    for index in 1..=2 {
        storage
            .insert_account(&Account {
                id: format!("acc_ws_same_{index}"),
                label: format!("ws-same-{index}"),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: Some("wsSame".to_string()),
                group_name: None,
                sort: index,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account wsSame");
        storage
            .insert_token(&Token {
                account_id: format!("acc_ws_same_{index}"),
                id_token: String::new(),
                access_token: format!("access_token_ws_same_{index}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_token_ws_same_{index}")),
                last_refresh: now,
            })
            .expect("insert token wsSame");
    }

    let platform_key = "pk_strip_same_workspace";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_strip_same_workspace".to_string(),
            name: Some("strip-same-workspace".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hello" }],
        "metadata": { "user_id": "user_strip_same_workspace" },
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
            ("x-codex-turn-state", "turn_state_same_ws"),
            ("conversation_id", "conv_same_ws"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let mut captured = Vec::new();
    for idx in 0..5 {
        captured.push(
            upstream_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|_| panic!("receive upstream request {idx}")),
        );
    }
    upstream_join.join().expect("join upstream");

    let account_2 = captured
        .iter()
        .find(|req| {
            req.headers
                .get("authorization")
                .map(|v| v.contains("access_token_ws_same_2"))
                .unwrap_or(false)
        })
        .expect("expected upstream request for account 2");

    assert_eq!(
        account_2
            .headers
            .get("chatgpt-account-id")
            .map(String::as_str),
        Some("wsSame")
    );
    assert_eq!(
        account_2
            .headers
            .get("x-codex-turn-state")
            .map(String::as_str),
        Some("turn_state_same_ws")
    );
    assert_eq!(
        account_2
            .headers
            .get("conversation_id")
            .map(String::as_str),
        Some("conv_same_ws")
    );
}

#[test]
fn gateway_request_log_keeps_only_final_result_for_multi_attempt_flow() {
    let _lock = lock_env();
    let dir = new_test_dir("codexmanager-gateway-final-log");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let trace_log_path: PathBuf = dir.join("gateway-trace.log");
    let _ = fs::remove_file(&trace_log_path);

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_final_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (
            404,
            serde_json::to_string(&first_response).expect("serialize first response"),
        ),
        (
            200,
            serde_json::to_string(&second_response).expect("serialize second response"),
        ),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();

    for index in 1..=2 {
        storage
            .insert_account(&Account {
                id: format!("acc_final_{index}"),
                label: format!("final-{index}"),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(format!("chatgpt_acc_final_{index}")),
                workspace_id: None,
                group_name: None,
                sort: index,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: format!("acc_final_{index}"),
                id_token: String::new(),
                access_token: format!("access_token_{index}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_token_{index}")),
                last_refresh: now,
            })
            .expect("insert token");
    }

    let platform_key = "pk_final_result_only";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_final_result_only".to_string(),
            name: Some("final-result-only".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let _ = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    let _ = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second upstream request");
    upstream_join.join().expect("join upstream");

    let logs = storage.list_request_logs(Some("key:gk_final_result_only"), 20).expect("list logs");
    let final_logs = logs
        .iter()
        .filter(|item| {
            item.request_path == "/v1/responses"
                && item.method == "POST"
                && item.key_id.as_deref() == Some("gk_final_result_only")
        })
        .collect::<Vec<_>>();
    assert_eq!(final_logs.len(), 1, "logs: {final_logs:#?}");
    assert_eq!(final_logs[0].status_code, Some(200));

    let trace_text = fs::read_to_string(&trace_log_path).expect("read trace log");
    assert!(trace_text.contains("event=ATTEMPT_RESULT"));
    assert!(trace_text.contains("event=REQUEST_FINAL"));
}
