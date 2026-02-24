use crate::http::backend_runtime::{start_backend_server, wake_backend_shutdown};
use crate::http::proxy_runtime::run_front_proxy;

pub fn start_http(addr: &str) -> std::io::Result<()> {
    let backend = start_backend_server()?;
    let result = run_front_proxy(addr, &backend.addr);
    wake_backend_shutdown(&backend.addr);
    let _ = backend.join.join();
    result
}
