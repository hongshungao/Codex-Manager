use gpttools_core::storage::Storage;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

static ENV_LOCK: Mutex<()> = Mutex::new(());

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
            let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            return (status, body);
        }
        last_raw = buf;
        thread::sleep(Duration::from_millis(50));
    }
    panic!("status parse failed, raw response: {last_raw:?}");
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
        gpttools_service::clear_shutdown_flag();
        for _ in 0..10 {
            let probe = TcpListener::bind("127.0.0.1:0").expect("bind probe port");
            let port = probe.local_addr().expect("probe addr").port();
            drop(probe);

            let addr = format!("localhost:{port}");
            let addr_for_thread = addr.clone();
            let join = thread::spawn(move || {
                let _ = gpttools_service::start_server(&addr_for_thread);
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
        gpttools_service::request_shutdown(&self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        gpttools_service::clear_shutdown_flag();
    }
}

#[test]
fn gateway_logs_invalid_api_key_error() {
    let _lock = ENV_LOCK.lock().expect("lock env");
    let mut dir = std::env::temp_dir();
    dir.push(format!("gpttools-gateway-logs-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let db_path: PathBuf = dir.join("gpttools.db");

    let _guard = EnvGuard::set("GPTTOOLS_DB_PATH", db_path.to_string_lossy().as_ref());

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
    let _lock = ENV_LOCK.lock().expect("lock env");
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "gpttools-gateway-logs-nonascii-{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    let db_path: PathBuf = dir.join("gpttools.db");

    let _guard = EnvGuard::set("GPTTOOLS_DB_PATH", db_path.to_string_lossy().as_ref());

    let server = TestServer::start();
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello"}"#;
    let metadata = r#"{"workspaces":{"D:\\MyComputer\\own\\GPTTeam相关\\CodexManager\\GPTTools":{"latest_git_commit_hash":"abc123"}}}"#;
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
