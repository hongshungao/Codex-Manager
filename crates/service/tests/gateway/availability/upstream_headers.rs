use super::*;

fn find_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

#[test]
fn codex_header_profile_sets_required_headers_for_stream() {
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-123",
        account_id: Some("acc-1"),
        upstream_cookie: Some("cf_clearance=test"),
        incoming_session_id: None,
        fallback_session_id: None,
        incoming_turn_state: Some("turn-state"),
        incoming_conversation_id: Some("conversation"),
        fallback_conversation_id: None,
        strip_session_affinity: false,
        is_stream: true,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Authorization").as_deref(),
        Some("Bearer token-123")
    );
    assert_eq!(
        find_header(&headers, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        find_header(&headers, "Accept").as_deref(),
        Some("text/event-stream")
    );
    assert_eq!(
        find_header(&headers, "Version").as_deref(),
        Some("0.101.0")
    );
    assert_eq!(
        find_header(&headers, "Openai-Beta").as_deref(),
        Some("responses=experimental")
    );
    assert_eq!(
        find_header(&headers, "Originator").as_deref(),
        Some("codex_cli_rs")
    );
    assert_eq!(
        find_header(&headers, "Chatgpt-Account-Id").as_deref(),
        Some("acc-1")
    );
    assert_eq!(
        find_header(&headers, "Cookie").as_deref(),
        Some("cf_clearance=test")
    );
    assert_eq!(
        find_header(&headers, "x-codex-turn-state").as_deref(),
        Some("turn-state")
    );
    assert_eq!(
        find_header(&headers, "Conversation_id").as_deref(),
        Some("conversation")
    );
    assert!(find_header(&headers, "Session_id").is_some());
}

#[test]
fn codex_header_profile_uses_json_accept_for_non_stream() {
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-456",
        account_id: None,
        upstream_cookie: None,
        incoming_session_id: None,
        fallback_session_id: None,
        incoming_turn_state: None,
        incoming_conversation_id: None,
        fallback_conversation_id: None,
        strip_session_affinity: false,
        is_stream: false,
        has_body: false,
    });

    assert_eq!(
        find_header(&headers, "Accept").as_deref(),
        Some("application/json")
    );
    assert!(find_header(&headers, "Content-Type").is_none());
}

#[test]
fn codex_header_profile_regenerates_session_on_failover() {
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-789",
        account_id: None,
        upstream_cookie: None,
        incoming_session_id: Some("sticky-session"),
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: Some("sticky-turn"),
        incoming_conversation_id: Some("sticky-conversation"),
        fallback_conversation_id: Some("fallback-conversation"),
        strip_session_affinity: true,
        is_stream: true,
        has_body: true,
    });

    assert_ne!(
        find_header(&headers, "Session_id").as_deref(),
        Some("sticky-session")
    );
    assert!(find_header(&headers, "x-codex-turn-state").is_none());
    assert!(find_header(&headers, "Conversation_id").is_none());
}

#[test]
fn codex_header_profile_uses_fallback_session_when_incoming_missing() {
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-fallback",
        account_id: None,
        upstream_cookie: None,
        incoming_session_id: None,
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: None,
        incoming_conversation_id: None,
        fallback_conversation_id: None,
        strip_session_affinity: false,
        is_stream: true,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Session_id").as_deref(),
        Some("fallback-session")
    );
}

#[test]
fn codex_header_profile_uses_fallback_conversation_when_incoming_missing() {
    let headers = build_codex_upstream_headers(CodexUpstreamHeaderInput {
        auth_token: "token-fallback-conv",
        account_id: None,
        upstream_cookie: None,
        incoming_session_id: None,
        fallback_session_id: Some("fallback-session"),
        incoming_turn_state: None,
        incoming_conversation_id: None,
        fallback_conversation_id: Some("fallback-conversation"),
        strip_session_affinity: false,
        is_stream: true,
        has_body: true,
    });

    assert_eq!(
        find_header(&headers, "Conversation_id").as_deref(),
        Some("fallback-conversation")
    );
}
