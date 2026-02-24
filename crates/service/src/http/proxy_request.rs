use axum::http::{HeaderMap, Uri};

use crate::http::header_filter::should_skip_request_header;

pub(crate) fn build_target_url(backend_base_url: &str, uri: &Uri) -> String {
    // 中文注释：部分 tiny_http 请求在重写后可能丢失 query；统一在这里拼接可避免多处实现不一致。
    let path_and_query = uri.path_and_query().map(|value| value.as_str()).unwrap_or("/");
    format!("{backend_base_url}{path_and_query}")
}

pub(crate) fn filter_request_headers(headers: &HeaderMap) -> HeaderMap {
    let mut outbound_headers = HeaderMap::new();
    for (name, value) in headers.iter() {
        if should_skip_request_header(name, value) {
            continue;
        }
        let _ = outbound_headers.insert(name.clone(), value.clone());
    }
    outbound_headers
}

#[cfg(test)]
mod tests {
    use super::{build_target_url, filter_request_headers};
    use axum::http::{HeaderMap, HeaderName, HeaderValue, Uri};

    #[test]
    fn build_target_url_keeps_path_and_query() {
        let uri: Uri = "/v1/models?limit=20".parse().expect("valid uri");
        assert_eq!(
            build_target_url("http://127.0.0.1:1234", &uri),
            "http://127.0.0.1:1234/v1/models?limit=20"
        );
    }

    #[test]
    fn filter_request_headers_drops_forbidden_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_static("localhost:8080"),
        );
        headers.insert(
            HeaderName::from_static("connection"),
            HeaderValue::from_static("keep-alive"),
        );

        let filtered = filter_request_headers(&headers);
        assert!(filtered.contains_key("content-type"));
        assert!(!filtered.contains_key("host"));
        assert!(!filtered.contains_key("connection"));
    }
}
