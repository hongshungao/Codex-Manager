use super::{has_api_key_header, parse_static_headers_json};

#[test]
fn static_headers_parse_ok_and_detect_api_key() {
    let headers = parse_static_headers_json(Some(
        r#"{"api-key":"k123","x-extra":"v","Content-Type":"application/json"}"#,
    ))
    .expect("parse headers");
    assert!(has_api_key_header(&headers));
}

#[test]
fn static_headers_without_api_key_returns_false() {
    let headers =
        parse_static_headers_json(Some(r#"{"authorization":"Bearer x"}"#)).expect("parse headers");
    assert!(!has_api_key_header(&headers));
}

#[test]
fn static_headers_invalid_value_rejected() {
    let err = parse_static_headers_json(Some(r#"{"api-key":123}"#))
        .expect_err("should reject non-string header value");
    assert!(err.contains("value must be string"));
}
