use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Response, StatusCode};

use crate::http::header_filter::should_skip_response_header;

pub(crate) fn text_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    let mut response = Response::new(Body::from(body.into()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

pub(crate) fn merge_upstream_headers(
    mut builder: axum::http::response::Builder,
    headers: &reqwest::header::HeaderMap,
) -> axum::http::response::Builder {
    for (name, value) in headers.iter() {
        if should_skip_response_header(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::{merge_upstream_headers, text_response};
    use axum::body::Body;
    use axum::http::header::CONTENT_TYPE;
    use axum::http::StatusCode;

    #[test]
    fn text_response_sets_status_and_plain_text_header() {
        let response = text_response(StatusCode::BAD_GATEWAY, "proxy failed");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
    }

    #[test]
    fn merge_upstream_headers_filters_hop_by_hop_and_content_length() {
        let mut upstream_headers = reqwest::header::HeaderMap::new();
        upstream_headers.insert(
            "content-type",
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        upstream_headers.insert(
            "content-length",
            reqwest::header::HeaderValue::from_static("64"),
        );
        upstream_headers.insert(
            "connection",
            reqwest::header::HeaderValue::from_static("close"),
        );

        let response = merge_upstream_headers(
            axum::http::Response::builder().status(StatusCode::OK),
            &upstream_headers,
        )
        .body(Body::empty())
        .expect("response should build");

        assert!(response.headers().contains_key("content-type"));
        assert!(!response.headers().contains_key("content-length"));
        assert!(!response.headers().contains_key("connection"));
    }
}
