use super::*;

#[test]
fn failover_on_missing_usage() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");
    let record = UsageSnapshotRecord {
        account_id: "acc-1".to_string(),
        used_percent: None,
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(10.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };
    storage.insert_usage_snapshot(&record).expect("insert usage");

    let should_failover = should_failover_after_refresh(&storage, "acc-1", Ok(()));
    assert!(should_failover);
}

#[test]
fn compute_url_keeps_v1_for_models_on_codex_backend() {
    let (url, alt) = compute_upstream_url("https://chatgpt.com/backend-api/codex", "/v1/models");
    assert_eq!(url, "https://chatgpt.com/backend-api/codex/models");
    assert_eq!(
        alt.as_deref(),
        Some("https://chatgpt.com/backend-api/codex/v1/models")
    );
    let (url, alt) = compute_upstream_url("https://api.openai.com/v1", "/v1/models");
    assert_eq!(url, "https://api.openai.com/v1/models");
    assert!(alt.is_none());
}

#[test]
fn normalize_upstream_base_url_for_chatgpt_host() {
    assert_eq!(
        normalize_upstream_base_url("https://chatgpt.com"),
        "https://chatgpt.com/backend-api/codex"
    );
    assert_eq!(
        normalize_upstream_base_url("https://chat.openai.com/"),
        "https://chat.openai.com/backend-api/codex"
    );
}

#[test]
fn normalize_upstream_base_url_keeps_existing_backend_path() {
    assert_eq!(
        normalize_upstream_base_url("https://chatgpt.com/backend-api/codex/"),
        "https://chatgpt.com/backend-api/codex"
    );
    assert_eq!(
        normalize_upstream_base_url("https://api.openai.com/v1/"),
        "https://api.openai.com/v1"
    );
}

#[test]
fn normalize_models_path_appends_client_version_when_missing() {
    assert_eq!(
        normalize_models_path("/v1/models"),
        "/v1/models?client_version=0.98.0"
    );
    assert_eq!(
        normalize_models_path("/v1/models?foo=1"),
        "/v1/models?foo=1&client_version=0.98.0"
    );
}

#[test]
fn normalize_models_path_keeps_existing_client_version() {
    assert_eq!(
        normalize_models_path("/v1/models?client_version=1.2.3"),
        "/v1/models?client_version=1.2.3"
    );
    assert_eq!(normalize_models_path("/v1/responses"), "/v1/responses");
}

#[test]
fn models_path_does_not_try_openai_fallback() {
    let content_type = HeaderValue::from_str("text/html; charset=utf-8").ok();
    assert!(!should_try_openai_fallback(
        "https://chatgpt.com/backend-api/codex",
        "/v1/models?client_version=0.98.0",
        content_type.as_ref()
    ));
    assert!(should_try_openai_fallback(
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        content_type.as_ref()
    ));
}

#[test]
fn status_fallback_triggers_on_429_and_responses_403() {
    assert!(should_try_openai_fallback_by_status(
        "https://chatgpt.com/backend-api/codex",
        "/v1/chat/completions",
        429
    ));
    assert!(should_try_openai_fallback_by_status(
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        403
    ));
    assert!(!should_try_openai_fallback_by_status(
        "https://chatgpt.com/backend-api/codex",
        "/v1/chat/completions",
        403
    ));
}

