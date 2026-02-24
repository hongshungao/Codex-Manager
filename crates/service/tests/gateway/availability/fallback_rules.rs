use super::*;

#[test]
fn html_content_type_detection() {
    assert!(is_html_content_type("text/html; charset=utf-8"));
    assert!(is_html_content_type("TEXT/HTML"));
    assert!(!is_html_content_type("application/json"));
}

#[test]
fn apply_request_overrides_accepts_xhigh() {
    let body = br#"{"model":"gpt-5.3-codex","reasoning":{"effort":"medium"}}"#.to_vec();
    let updated = apply_request_overrides("/v1/responses", body, None, Some("xhigh"));
    let value: serde_json::Value = serde_json::from_slice(&updated).expect("json");
    assert_eq!(value["reasoning"]["effort"], "xhigh");
}

#[test]
fn apply_request_overrides_maps_extra_high_to_xhigh() {
    let body = br#"{"model":"gpt-5.3-codex"}"#.to_vec();
    let updated = apply_request_overrides("/v1/responses", body, None, Some("extra_high"));
    let value: serde_json::Value = serde_json::from_slice(&updated).expect("json");
    assert_eq!(value["reasoning"]["effort"], "xhigh");
}

#[test]
fn cooldown_reason_maps_status() {
    assert_eq!(cooldown_reason_for_status(429), CooldownReason::RateLimited);
    assert_eq!(cooldown_reason_for_status(503), CooldownReason::Upstream5xx);
    assert_eq!(cooldown_reason_for_status(403), CooldownReason::Challenge);
    assert_eq!(cooldown_reason_for_status(400), CooldownReason::Upstream4xx);
    assert_eq!(cooldown_reason_for_status(200), CooldownReason::Default);
}

#[test]
fn challenge_detection_requires_html_or_429() {
    let html = HeaderValue::from_str("text/html; charset=utf-8").ok();
    let json = HeaderValue::from_str("application/json").ok();
    assert!(is_upstream_challenge_response(403, html.as_ref()));
    assert!(!is_upstream_challenge_response(403, json.as_ref()));
    assert!(is_upstream_challenge_response(429, json.as_ref()));
}
