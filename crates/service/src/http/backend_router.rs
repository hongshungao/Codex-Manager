use tiny_http::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendRoute {
    Rpc,
    AuthCallback,
    Metrics,
    Gateway,
}

pub(crate) fn resolve_backend_route(method: &str, path: &str) -> BackendRoute {
    if method == "POST" && path == "/rpc" {
        return BackendRoute::Rpc;
    }
    if method == "GET" && path.starts_with("/auth/callback") {
        return BackendRoute::AuthCallback;
    }
    if method == "GET" && path == "/metrics" {
        return BackendRoute::Metrics;
    }
    BackendRoute::Gateway
}

pub(crate) fn handle_backend_request(request: Request) {
    let route = resolve_backend_route(request.method().as_str(), request.url());
    match route {
        BackendRoute::Rpc => crate::http::rpc_endpoint::handle_rpc(request),
        BackendRoute::AuthCallback => crate::http::callback_endpoint::handle_callback(request),
        BackendRoute::Metrics => crate::http::gateway_endpoint::handle_metrics(request),
        BackendRoute::Gateway => crate::http::gateway_endpoint::handle_gateway(request),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_backend_route, BackendRoute};

    #[test]
    fn resolves_rpc_route() {
        assert_eq!(resolve_backend_route("POST", "/rpc"), BackendRoute::Rpc);
    }

    #[test]
    fn resolves_auth_callback_route() {
        assert_eq!(
            resolve_backend_route("GET", "/auth/callback?code=123"),
            BackendRoute::AuthCallback
        );
    }

    #[test]
    fn resolves_metrics_route() {
        assert_eq!(
            resolve_backend_route("GET", "/metrics"),
            BackendRoute::Metrics
        );
    }

    #[test]
    fn falls_back_to_gateway_route() {
        assert_eq!(
            resolve_backend_route("POST", "/v1/responses"),
            BackendRoute::Gateway
        );
    }
}
