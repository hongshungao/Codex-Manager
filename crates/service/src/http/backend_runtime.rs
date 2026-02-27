use std::io;
use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use tiny_http::Request;
use tiny_http::Server;

const HTTP_WORKER_FACTOR: usize = 4;
const HTTP_WORKER_MIN: usize = 8;
const HTTP_STREAM_WORKER_FACTOR: usize = 1;
const HTTP_STREAM_WORKER_MIN: usize = 2;
const HTTP_QUEUE_FACTOR: usize = 4;
const HTTP_QUEUE_MIN: usize = 32;
const HTTP_STREAM_QUEUE_FACTOR: usize = 2;
const HTTP_STREAM_QUEUE_MIN: usize = 16;
const ENV_HTTP_WORKER_FACTOR: &str = "CODEXMANAGER_HTTP_WORKER_FACTOR";
const ENV_HTTP_WORKER_MIN: &str = "CODEXMANAGER_HTTP_WORKER_MIN";
const ENV_HTTP_STREAM_WORKER_FACTOR: &str = "CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR";
const ENV_HTTP_STREAM_WORKER_MIN: &str = "CODEXMANAGER_HTTP_STREAM_WORKER_MIN";
const ENV_HTTP_QUEUE_FACTOR: &str = "CODEXMANAGER_HTTP_QUEUE_FACTOR";
const ENV_HTTP_QUEUE_MIN: &str = "CODEXMANAGER_HTTP_QUEUE_MIN";
const ENV_HTTP_STREAM_QUEUE_FACTOR: &str = "CODEXMANAGER_HTTP_STREAM_QUEUE_FACTOR";
const ENV_HTTP_STREAM_QUEUE_MIN: &str = "CODEXMANAGER_HTTP_STREAM_QUEUE_MIN";

pub(crate) struct BackendServer {
    pub(crate) addr: String,
    pub(crate) join: thread::JoinHandle<()>,
}

fn http_worker_count() -> usize {
    // 中文注释：长流请求会占用处理线程；这里固定 worker 上限，避免并发时无限 spawn 拖垮进程。
    let cpus = thread::available_parallelism().map(|value| value.get()).unwrap_or(4);
    let factor = env_usize_or(ENV_HTTP_WORKER_FACTOR, HTTP_WORKER_FACTOR).max(1);
    let min = env_usize_or(ENV_HTTP_WORKER_MIN, HTTP_WORKER_MIN).max(1);
    (cpus.saturating_mul(factor)).max(min)
}

fn http_stream_worker_count() -> usize {
    let cpus = thread::available_parallelism().map(|value| value.get()).unwrap_or(4);
    let factor = env_usize_or(ENV_HTTP_STREAM_WORKER_FACTOR, HTTP_STREAM_WORKER_FACTOR).max(1);
    let min = env_usize_or(ENV_HTTP_STREAM_WORKER_MIN, HTTP_STREAM_WORKER_MIN).max(1);
    (cpus.saturating_mul(factor)).max(min)
}

fn http_queue_size(worker_count: usize) -> usize {
    // 中文注释：使用有界队列给入口施加背压；不这样做会在峰值流量下无限堆积请求并放大内存抖动。
    let factor = env_usize_or(ENV_HTTP_QUEUE_FACTOR, HTTP_QUEUE_FACTOR).max(1);
    let min = env_usize_or(ENV_HTTP_QUEUE_MIN, HTTP_QUEUE_MIN).max(1);
    worker_count.saturating_mul(factor).max(min)
}

fn http_stream_queue_size(worker_count: usize) -> usize {
    let factor = env_usize_or(ENV_HTTP_STREAM_QUEUE_FACTOR, HTTP_STREAM_QUEUE_FACTOR).max(1);
    let min = env_usize_or(ENV_HTTP_STREAM_QUEUE_MIN, HTTP_STREAM_QUEUE_MIN).max(1);
    worker_count.saturating_mul(factor).max(min)
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn spawn_request_workers(worker_count: usize, rx: Receiver<Request>, is_stream_queue: bool) {
    for _ in 0..worker_count {
        let worker_rx = rx.clone();
        let _ = thread::spawn(move || {
            while let Ok(request) = worker_rx.recv() {
                crate::gateway::record_http_queue_dequeue(is_stream_queue);
                crate::http::backend_router::handle_backend_request(request);
            }
        });
    }
}

fn request_accept_header(request: &Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Accept"))
        .map(|header| header.value.as_str().to_ascii_lowercase())
}

fn request_is_stream_like(request: &Request) -> bool {
    request_accept_header(request)
        .is_some_and(|value| value.contains("text/event-stream"))
}

fn enqueue_request(
    request: Request,
    normal_tx: &Sender<Request>,
    stream_tx: &Sender<Request>,
) -> Result<(), ()> {
    let prefer_stream = request_is_stream_like(&request);
    if prefer_stream {
        match stream_tx.send(request) {
            Ok(()) => {
                crate::gateway::record_http_queue_enqueue(true);
                Ok(())
            }
            Err(err) => {
                normal_tx.send(err.into_inner()).map_err(|_| ())?;
                crate::gateway::record_http_queue_enqueue(false);
                Ok(())
            }
        }
    } else {
        match normal_tx.send(request) {
            Ok(()) => {
                crate::gateway::record_http_queue_enqueue(false);
                Ok(())
            }
            Err(err) => {
                stream_tx.send(err.into_inner()).map_err(|_| ())?;
                crate::gateway::record_http_queue_enqueue(true);
                Ok(())
            }
        }
    }
}

fn run_backend_server(server: Server) {
    let worker_count = http_worker_count();
    let stream_worker_count = http_stream_worker_count();
    let queue_size = http_queue_size(worker_count);
    let stream_queue_size = http_stream_queue_size(stream_worker_count);
    let (normal_tx, normal_rx) = bounded::<Request>(queue_size);
    let (stream_tx, stream_rx) = bounded::<Request>(stream_queue_size);
    crate::gateway::record_http_queue_capacity(queue_size, stream_queue_size);
    spawn_request_workers(worker_count, normal_rx, false);
    spawn_request_workers(stream_worker_count, stream_rx, true);

    for request in server.incoming_requests() {
        if crate::shutdown_requested() || request.url() == "/__shutdown" {
            let _ = request.respond(tiny_http::Response::from_string("shutdown"));
            break;
        }
        if enqueue_request(request, &normal_tx, &stream_tx).is_err() {
            crate::gateway::record_http_queue_enqueue_failure();
            break;
        }
    }
}

pub(crate) fn start_backend_server() -> io::Result<BackendServer> {
    let server = Server::http("127.0.0.1:0").map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    let addr = server
        .server_addr()
        .to_ip()
        .map(|address| address.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "backend addr missing"))?;
    let join = thread::spawn(move || run_backend_server(server));
    Ok(BackendServer { addr, join })
}

pub(crate) fn wake_backend_shutdown(addr: &str) {
    let Ok(mut stream) = TcpStream::connect(addr) else {
        return;
    };

    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));

    let request = format!("GET /__shutdown HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(request.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::{
        http_queue_size, http_stream_queue_size, http_stream_worker_count, http_worker_count,
        HTTP_QUEUE_MIN, HTTP_STREAM_QUEUE_MIN, HTTP_STREAM_WORKER_MIN, HTTP_WORKER_MIN,
    };

    #[test]
    fn worker_count_has_minimum_guard() {
        assert!(http_worker_count() >= HTTP_WORKER_MIN);
        assert!(http_stream_worker_count() >= HTTP_STREAM_WORKER_MIN);
    }

    #[test]
    fn queue_size_has_minimum_guard() {
        assert!(http_queue_size(0) >= HTTP_QUEUE_MIN);
        assert!(http_stream_queue_size(0) >= HTTP_STREAM_QUEUE_MIN);
    }
}

