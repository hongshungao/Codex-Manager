use std::io;
use std::time::Duration;

use axum::Router;

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

pub(crate) async fn run_proxy_server(addr: &str, app: Router) -> io::Result<()> {
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
