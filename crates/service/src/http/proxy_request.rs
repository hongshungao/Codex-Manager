use axum::http::{HeaderMap, Uri};

use crate::http::header_filter::should_skip_request_header;

pub(crate) fn build_target_url(backend_base_url: &str, uri: &Uri) -> String {
    // 中文注释：部分 tiny_http 请求在重写后可能丢失 query；统一在这里拼接可避免多处实现不一致。
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
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
#[path = "tests/proxy_request_tests.rs"]
mod tests;
