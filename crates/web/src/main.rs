#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_WEB_ADDR: &str = "localhost:48761";

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    service_rpc_url: String,
    rpc_token: String,
}

fn read_env_trim(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn normalize_addr(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        value = rest;
    }
    value = value.split('/').next().unwrap_or(value);
    if value.is_empty() {
        return None;
    }
    if value.contains(':') {
        return Some(value.to_string());
    }
    Some(format!("localhost:{value}"))
}

fn resolve_service_addr() -> String {
    read_env_trim("CODEXMANAGER_SERVICE_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| codexmanager_service::DEFAULT_ADDR.to_string())
}

fn resolve_web_addr() -> String {
    read_env_trim("CODEXMANAGER_WEB_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| DEFAULT_WEB_ADDR.to_string())
}

fn resolve_web_root() -> PathBuf {
    if let Some(v) = read_env_trim("CODEXMANAGER_WEB_ROOT") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return p;
        }
        return exe_dir().join(p);
    }
    exe_dir().join("web")
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

async fn rpc_proxy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_json_content_type(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response();
    }
    let resp = state
        .client
        .post(&state.service_rpc_url)
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .body(body)
        .send()
        .await;
    let resp = match resp {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let bytes = match resp.bytes().await {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream read error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };
    let mut out = Response::new(axum::body::Body::from(bytes));
    *out.status_mut() = status;
    out.headers_mut()
        .insert("content-type", axum::http::HeaderValue::from_static("application/json"));
    out
}

async fn serve_on_listener(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // 保持简单：当前版本无 UI 关闭入口，进程退出即可
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
}

async fn run_web_server(addr: &str, app: Router) -> std::io::Result<()> {
    // 中文注释：localhost 在 Windows/macOS 上可能只解析到单栈；双栈监听可减少连接差异。
    let trimmed = addr.trim();
    if trimmed.len() > "localhost:".len() && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:") {
        let port = &trimmed["localhost:".len()..];
        let v4 = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await;
        let v6 = tokio::net::TcpListener::bind(format!("[::1]:{port}")).await;
        return match (v4, v6) {
            (Ok(v4_listener), Ok(v6_listener)) => {
                let v4_task = serve_on_listener(v4_listener, app.clone());
                let v6_task = serve_on_listener(v6_listener, app);
                let (v4_result, v6_result) = tokio::join!(v4_task, v6_task);
                v4_result.and(v6_result)
            }
            (Ok(listener), Err(_)) | (Err(_), Ok(listener)) => serve_on_listener(listener, app).await,
            (Err(err), Err(_)) => Err(err),
        };
    }

    let listener = tokio::net::TcpListener::bind(trimmed).await?;
    serve_on_listener(listener, app).await
}

fn ensure_index_file(index: &Path) -> bool {
    index.is_file()
}

#[tokio::main]
async fn main() {
    // 先加载同目录 env / 默认 DB / RPC token 文件，做到“解压即用”。
    codexmanager_service::portable::bootstrap_current_process();

    let service_addr = resolve_service_addr();
    let web_addr = resolve_web_addr();
    let web_root = resolve_web_root();
    let index = web_root.join("index.html");

    let rpc_url = format!("http://{service_addr}/rpc");
    let rpc_token = codexmanager_service::rpc_auth_token().to_string();

    if !ensure_index_file(&index) {
        eprintln!(
            "web root invalid: {} (index.html missing). Expect files under: <exe_dir>/web",
            web_root.display()
        );
    }

    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        service_rpc_url: rpc_url,
        rpc_token,
    });

    let static_service = ServeDir::new(&web_root).not_found_service(ServeFile::new(index));
    let app = Router::new()
        .route("/api/rpc", post(rpc_proxy))
        .fallback_service(static_service)
        .with_state(state);

    println!("codexmanager-web listening on {web_addr} (service={service_addr})");

    let open_url = format!("http://{}", web_addr.trim());
    if read_env_trim("CODEXMANAGER_WEB_NO_OPEN").is_none() {
        let _ = webbrowser::open(&open_url);
    }

    if let Err(err) = run_web_server(&web_addr, app).await {
        eprintln!("web stopped: {err}");
        std::process::exit(1);
    }
}
