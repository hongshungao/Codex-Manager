use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, Request as HttpRequest, Response, StatusCode};
use axum::routing::any;
use axum::Router;
use reqwest::Client;
use std::io;

use crate::http::proxy_bridge::run_proxy_server;
use crate::http::proxy_request::{build_target_url, filter_request_headers};
use crate::http::proxy_response::{merge_upstream_headers, text_response};

#[derive(Clone)]
struct ProxyState {
    backend_base_url: String,
    client: Client,
}

fn build_backend_base_url(backend_addr: &str) -> String {
    format!("http://{backend_addr}")
}

async fn proxy_handler(State(state): State<ProxyState>, request: HttpRequest<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let target_url = build_target_url(&state.backend_base_url, &parts.uri);
    let max_body_bytes = crate::gateway::front_proxy_max_body_bytes();

    if let Some(content_length) = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        if content_length > max_body_bytes as u64 {
            return text_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body too large: content-length={content_length}"),
            );
        }
    }

    let outbound_headers = filter_request_headers(&parts.headers);
    let body_bytes = match to_bytes(body, max_body_bytes).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return text_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body too large: content-length>{max_body_bytes}"),
            );
        }
    };

    let mut builder = state.client.request(parts.method, target_url);
    builder = builder.headers(outbound_headers);
    builder = builder.body(body_bytes);

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            return text_response(StatusCode::BAD_GATEWAY, format!("backend proxy error: {err}"));
        }
    };

    let response_builder = merge_upstream_headers(
        Response::builder().status(upstream.status()),
        upstream.headers(),
    );

    match response_builder.body(Body::from_stream(upstream.bytes_stream())) {
        Ok(response) => response,
        Err(err) => text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("build response failed: {err}"),
        ),
    }
}

pub(crate) fn run_front_proxy(addr: &str, backend_addr: &str) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    runtime.block_on(async move {
        let client = Client::builder()
            .build()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let state = ProxyState {
            backend_base_url: build_backend_base_url(backend_addr),
            client,
        };
        let app = Router::new().fallback(any(proxy_handler)).with_state(state);
        run_proxy_server(addr, app).await
    })
}

#[cfg(test)]
mod tests {
    use super::{build_backend_base_url, proxy_handler, ProxyState};
    use axum::body::{to_bytes, Body};
    use axum::extract::State;
    use axum::http::{Request as HttpRequest, StatusCode};
    use reqwest::Client;

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
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn backend_base_url_uses_http_scheme() {
        assert_eq!(
            build_backend_base_url("127.0.0.1:18080"),
            "http://127.0.0.1:18080"
        );
    }

    #[test]
    fn request_without_content_length_over_limit_returns_413() {
        let _guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "8");
        crate::gateway::reload_runtime_config_from_env();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let state = ProxyState {
            backend_base_url: "http://127.0.0.1:1".to_string(),
            client: Client::new(),
        };
        let request = HttpRequest::builder()
            .method("POST")
            .uri("/rpc")
            .body(Body::from(vec![b'x'; 9]))
            .expect("request");

        let response = runtime.block_on(proxy_handler(State(state), request));
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = runtime
            .block_on(to_bytes(response.into_body(), usize::MAX))
            .expect("read body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("request body too large: content-length>8"));
    }
}
