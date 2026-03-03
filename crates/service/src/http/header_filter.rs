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
#[path = "tests/header_filter_tests.rs"]
mod tests;
