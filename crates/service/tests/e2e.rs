use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::Storage;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
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

fn post_rpc(addr: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect server");
    let token = codexmanager_service::rpc_auth_token().to_string();
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nX-CodexManager-Rpc-Token: {token}\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).expect("write");
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");
    buf
}

#[test]
fn e2e_initialize_writes_event() {
    let mut dir = std::env::temp_dir();
    dir.push(format!("codexmanager-e2e-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req = JsonRpcRequest {
        id: 1,
        method: "initialize".to_string(),
        params: None,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let buf = post_rpc(&server.addr, &json);
    assert!(!buf.trim().is_empty());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init schema");
    let count = storage.event_count().expect("count events");
    assert!(count >= 1);
}
