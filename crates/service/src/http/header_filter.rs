use axum::http::{HeaderName, HeaderValue};

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

pub(crate) fn should_skip_request_header(name: &HeaderName, value: &HeaderValue) -> bool {
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

pub(crate) fn should_skip_response_header(name: &HeaderName) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower) || lower.eq_ignore_ascii_case("content-length")
}

#[cfg(test)]
mod tests {
    use super::{should_skip_request_header, should_skip_response_header};
    use axum::http::{HeaderName, HeaderValue};

    #[test]
    fn request_header_filters_hop_by_hop_and_non_ascii() {
        let connection = HeaderName::from_static("connection");
        let keep = HeaderValue::from_static("keep-alive");
        assert!(should_skip_request_header(&connection, &keep));

        let metadata = HeaderName::from_static("x-codex-turn-metadata");
        let bad_value = HeaderValue::from_bytes(&[0xE4, 0xB8, 0xAD]).expect("non-ascii bytes");
        assert!(should_skip_request_header(&metadata, &bad_value));
    }

    #[test]
    fn request_header_keeps_normal_content_type() {
        let content_type = HeaderName::from_static("content-type");
        let json = HeaderValue::from_static("application/json");
        assert!(!should_skip_request_header(&content_type, &json));
    }

    #[test]
    fn response_header_filters_content_length_and_connection() {
        let content_length = HeaderName::from_static("content-length");
        assert!(should_skip_response_header(&content_length));

        let connection = HeaderName::from_static("connection");
        assert!(should_skip_response_header(&connection));

        let content_type = HeaderName::from_static("content-type");
        assert!(!should_skip_response_header(&content_type));
    }
}
