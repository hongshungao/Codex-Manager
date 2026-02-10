use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Request as HttpRequest, Response, StatusCode};
use axum::routing::any;
use axum::Router;
use reqwest::Client;
use std::io;
use std::io::Write;
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use tiny_http::Request;
use tiny_http::Server;

const HTTP_WORKER_FACTOR: usize = 4;
const HTTP_WORKER_MIN: usize = 8;
const HTTP_QUEUE_FACTOR: usize = 4;
const HTTP_QUEUE_MIN: usize = 32;
const FRONT_PROXY_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
struct ProxyState {
    backend_base_url: String,
    client: Client,
}

struct BackendServer {
    addr: String,
    join: thread::JoinHandle<()>,
}

fn http_worker_count() -> usize {
    // 中文注释：长流请求会占用处理线程；这里固定 worker 上限，避免并发时无限 spawn 拖垮进程。
    let cpus = thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(4);
    (cpus * HTTP_WORKER_FACTOR).max(HTTP_WORKER_MIN)
}

fn http_queue_size(worker_count: usize) -> usize {
    // 中文注释：使用有界队列给入口施加背压；不这样做会在峰值流量下无限堆积请求并放大内存抖动。
    worker_count.saturating_mul(HTTP_QUEUE_FACTOR).max(HTTP_QUEUE_MIN)
}

fn spawn_request_workers(worker_count: usize, rx: mpsc::Receiver<Request>) {
    let shared_rx = Arc::new(Mutex::new(rx));
    for _ in 0..worker_count {
        let worker_rx = Arc::clone(&shared_rx);
        let _ = thread::spawn(move || loop {
            let request = {
                let Ok(guard) = worker_rx.lock() else {
                    break;
                };
                match guard.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                }
            };
            route_request(request);
        });
    }
}

fn run_backend_server(server: Server) {
    let worker_count = http_worker_count();
    let queue_size = http_queue_size(worker_count);
    let (tx, rx) = mpsc::sync_channel::<Request>(queue_size);
    spawn_request_workers(worker_count, rx);

    for request in server.incoming_requests() {
        if crate::shutdown_requested() || request.url() == "/__shutdown" {
            let _ = request.respond(tiny_http::Response::from_string("shutdown"));
            break;
        }
        if tx.send(request).is_err() {
            break;
        }
    }
}

fn start_backend_server() -> io::Result<BackendServer> {
    let server = Server::http("127.0.0.1:0")
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    let addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "backend addr missing"))?;
    let join = thread::spawn(move || run_backend_server(server));
    Ok(BackendServer { addr, join })
}

fn is_hop_by_hop_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("keep-alive")
        || name.eq_ignore_ascii_case("proxy-authenticate")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("te")
        || name.eq_ignore_ascii_case("trailer")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("upgrade")
}

fn should_skip_request_header(name: &HeaderName, value: &HeaderValue) -> bool {
    let lower = name.as_str();
    if is_hop_by_hop_header(lower)
        || lower.eq_ignore_ascii_case("host")
        || lower.eq_ignore_ascii_case("content-length")
        // 中文注释：该头由 Codex 自动注入，值里可能包含中文路径；若直传给 tiny_http 会在解析阶段断流。
        // 在前置代理层剔除该头，可避免“请求没进业务层就断开”。
        || lower.eq_ignore_ascii_case("x-codex-turn-metadata")
    {
        return true;
    }
    // 中文注释：tiny_http 仅支持 ASCII 头值；非 ASCII 统一在入口层过滤，避免污染后端业务处理。
    value.to_str().is_err()
}

fn should_skip_response_header(name: &HeaderName) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower) || lower.eq_ignore_ascii_case("content-length")
}

fn text_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    let mut response = Response::new(Body::from(body.into()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    request: HttpRequest<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/");
    let target_url = format!("{}{}", state.backend_base_url, path_and_query);

    let body_bytes = match to_bytes(body, FRONT_PROXY_MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return text_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body too large: {err}"),
            );
        }
    };

    let mut outbound_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if should_skip_request_header(name, value) {
            continue;
        }
        let _ = outbound_headers.insert(name.clone(), value.clone());
    }

    let mut builder = state.client.request(parts.method, target_url);
    builder = builder.headers(outbound_headers);
    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes.to_vec());
    }

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            return text_response(StatusCode::BAD_GATEWAY, format!("backend proxy error: {err}"));
        }
    };

    let mut response_builder = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers().iter() {
        if should_skip_response_header(name) {
            continue;
        }
        response_builder = response_builder.header(name, value);
    }

    match response_builder.body(Body::from_stream(upstream.bytes_stream())) {
        Ok(response) => response,
        Err(err) => text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("build response failed: {err}"),
        ),
    }
}

async fn wait_for_shutdown_signal() {
    while !crate::shutdown_requested() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn serve_proxy_on_listener(
    listener: tokio::net::TcpListener,
    app: Router,
) -> io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(wait_for_shutdown_signal())
        .await
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

async fn run_proxy_server(addr: &str, app: Router) -> io::Result<()> {
    // 中文注释：localhost 在 Windows 上可能只解析到 IPv6；双栈监听可避免客户端栈选择差异导致的连接失败。
    if let Some(port) = addr.strip_prefix("localhost:") {
        let v4 = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await;
        let v6 = tokio::net::TcpListener::bind(format!("[::1]:{port}")).await;
        return match (v4, v6) {
            (Ok(v4_listener), Ok(v6_listener)) => {
                let v4_task = serve_proxy_on_listener(v4_listener, app.clone());
                let v6_task = serve_proxy_on_listener(v6_listener, app);
                let (v4_result, v6_result) = tokio::join!(v4_task, v6_task);
                v4_result.and(v6_result)
            }
            (Ok(listener), Err(_)) | (Err(_), Ok(listener)) => {
                serve_proxy_on_listener(listener, app).await
            }
            (Err(err), Err(_)) => Err(err),
        };
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_proxy_on_listener(listener, app).await
}

fn run_front_proxy(addr: &str, backend_addr: &str) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    runtime.block_on(async move {
        let client = Client::builder()
            .build()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let state = ProxyState {
            backend_base_url: format!("http://{backend_addr}"),
            client,
        };
        let app = Router::new().fallback(any(proxy_handler)).with_state(state);
        run_proxy_server(addr, app).await
    })
}

fn wake_backend_shutdown(addr: &str) {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let req = format!(
        "GET /__shutdown HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.write_all(req.as_bytes());
}

pub fn start_http(addr: &str) -> std::io::Result<()> {
    let backend = start_backend_server()?;
    let result = run_front_proxy(addr, &backend.addr);
    wake_backend_shutdown(&backend.addr);
    let _ = backend.join.join();
    result
}

pub fn route_request(request: Request) {
    let path = request.url().to_string();
    if request.method().as_str() == "POST" && path == "/rpc" {
        crate::http::rpc_endpoint::handle_rpc(request);
        return;
    }
    if request.method().as_str() == "GET" && path.starts_with("/auth/callback") {
        crate::http::callback_endpoint::handle_callback(request);
        return;
    }
    if request.method().as_str() == "GET" && path == "/metrics" {
        crate::http::gateway_endpoint::handle_metrics(request);
        return;
    }
    crate::http::gateway_endpoint::handle_gateway(request);
}
