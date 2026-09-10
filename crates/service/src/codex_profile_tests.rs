use super::*;
use codexmanager_core::storage::{Account, Storage};
use rusqlite::Connection;

fn temp_profile(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-{name}-{unique}"))
}

fn cleanup_profile(dir: &Path) {
    if let Ok(root) = managed_profile_root(dir) {
        let _ = fs::remove_dir_all(root);
    }
    let _ = fs::remove_dir_all(dir);
}

fn provider_http_header<'a>(provider: &'a Table, name: &str) -> Option<&'a str> {
    let headers = provider.get("http_headers")?;
    if let Some(table) = headers.as_table() {
        return table.get(name).and_then(Item::as_str);
    }
    headers
        .as_value()
        .and_then(Value::as_inline_table)
        .and_then(|inline| inline.get(name))
        .and_then(Value::as_str)
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn set_test_db(dir: &Path) -> EnvGuard {
    fs::create_dir_all(dir).expect("create test db dir");
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open test storage");
    storage.init().expect("init test storage");
    drop(storage);
    EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref())
}

fn test_account(id: &str, status: &str) -> Account {
    Account {
        id: id.to_string(),
        label: format!("Label {id}"),
        issuer: format!("issuer-{id}"),
        chatgpt_account_id: Some(format!("cgpt-{id}")),
        workspace_id: Some(format!("ws-{id}")),
        group_name: Some("test-group".to_string()),
        sort: 0,
        status: status.to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

fn test_token(account_id: &str, access_token: &str, refresh_token: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        api_key_access_token: None,
        last_refresh: 123,
    }
}

fn write_test_rollout(dir: &Path, thread_id: &str, provider: &str) -> (PathBuf, String) {
    let rollout_dir = dir.join("sessions").join("2026").join("06").join("06");
    fs::create_dir_all(&rollout_dir).expect("mkdir rollout");
    let path = rollout_dir.join(format!("rollout-2026-06-06T00-00-00-{thread_id}.jsonl"));
    let event_line = r#"{"timestamp":"2026-06-06T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"keep me"}}"#.to_string();
    let content = format!(
        "{{\"timestamp\":\"2026-06-06T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"model_provider\":\"{provider}\",\"cwd\":\"/tmp\"}}}}\n{event_line}\n"
    );
    fs::write(&path, content).expect("write rollout");
    (path, event_line)
}

fn create_state_db(dir: &Path, thread_id: &str, provider: &str) {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at INTEGER,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000, 1770000000000)",
        params![thread_id, provider],
    )
    .expect("insert thread");
}

fn sqlite_provider(dir: &Path, thread_id: &str) -> String {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.query_row(
        "SELECT model_provider FROM threads WHERE id = ?1",
        params![thread_id],
        |row| row.get::<_, String>(0),
    )
    .expect("read provider")
}

#[test]
fn direct_config_uses_openai_and_keeps_secret_free_legacy_provider() {
    let input = r#"
model_provider = "cm"
model = "gpt-5.4"

[model_providers.cm]
name = "CodexManager"
base_url = "http://localhost:48760/v1"
wire_api = "responses"
experimental_bearer_token = "must-be-removed"
custom_header = "must-also-be-removed"

[model_providers.other]
name = "Other"
base_url = "https://example.test/v1"
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_direct(Some(input.to_string()), &managed_catalog, None)
        .expect("patch direct");

    assert!(output.contains("model_provider = \"openai\""));
    assert!(output.contains("[model_providers.cm]"));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(output.contains("requires_openai_auth = true"));
    assert!(!output.contains("base_url = \"http://localhost:48760/v1\""));
    assert!(!output.contains("experimental_bearer_token"));
    assert!(!output.contains("custom_header"));
    assert!(output.contains("[model_providers.other]"));
    assert!(output.contains("model = \"gpt-5.4\""));
}

#[test]
fn gateway_config_sets_managed_provider_and_preserves_other_values() {
    let input = r#"
model = "gpt-5.4"

[model_providers.other]
name = "Other"
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        true,
        "cm-managed-key",
        true,
    )
    .expect("patch gateway");

    assert!(output.contains("model_provider = \"cm\""));
    assert!(output.contains("[model_providers.cm]"));
    assert!(output.contains("name = \"CodexManager\""));
    assert!(output.contains("base_url = \"http://127.0.0.1:48770/v1\""));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(!output.contains("requires_openai_auth"));
    assert!(output.contains("supports_websockets = true"));
    assert!(output.contains("experimental_bearer_token = \"cm-managed-key\""));
    assert!(!output.contains("[model_providers.cm.auth]"));
    assert!(output.contains("model_catalog_json = \"/tmp/codexmanager/gateway-models.json\""));
    assert!(output.contains("[model_providers.other]"));
    let doc = parse_config(&output).expect("parse gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");
    assert_eq!(
        provider_http_header(
            provider,
            crate::gateway::X_OPENAI_ACTOR_AUTHORIZATION_HEADER
        ),
        Some(crate::gateway::CODEXMANAGER_IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("cm-managed-key")
    );

    let without_websocket = patch_config_for_gateway(
        Some(output),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        false,
        "cm-managed-key",
        true,
    )
    .expect("disable gateway websocket");
    assert!(without_websocket.contains("supports_websockets = false"));
}

#[test]
fn gateway_config_preserves_requires_openai_auth_when_removal_is_disabled() {
    let input = r#"
model_provider = "cm"

[model_providers.cm]
name = "Custom Gateway"
requires_openai_auth = true
"#;

    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        Path::new("/tmp/gateway-models.json"),
        false,
        "cm-managed-key",
        false,
    )
    .expect("patch gateway without removing auth requirement");
    let doc = parse_config(&output).expect("parse patched gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");

    assert_eq!(
        provider.get("requires_openai_auth").and_then(Item::as_bool),
        Some(true)
    );
}

#[test]
fn gateway_config_preserves_custom_managed_provider_values() {
    let input = r#"
model_provider = "cm"

[model_providers.cm]
name = "Custom Gateway"
base_url = "https://stale.example.test/v1"
wire_api = "chat"
requires_openai_auth = false
supports_websockets = false
experimental_bearer_token = "custom-key"
custom_header = "custom-value"
http_headers = { "x-existing-header" = "keep", "x-openai-actor-authorization" = "stale" }
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        true,
        "cm-managed-key",
        true,
    )
    .expect("patch gateway");
    let doc = parse_config(&output).expect("parse patched gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");

    assert_eq!(
        provider.get("name").and_then(Item::as_str),
        Some("Custom Gateway")
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("cm-managed-key")
    );
    assert_eq!(
        provider.get("custom_header").and_then(Item::as_str),
        Some("custom-value")
    );
    assert_eq!(
        provider.get("base_url").and_then(Item::as_str),
        Some("http://127.0.0.1:48770/v1")
    );
    assert_eq!(
        provider.get("wire_api").and_then(Item::as_str),
        Some("responses")
    );
    assert!(provider.get("requires_openai_auth").is_none());
    assert!(provider.get("auth").is_none());
    assert_eq!(
        provider.get("supports_websockets").and_then(Item::as_bool),
        Some(true)
    );
    assert_eq!(
        provider_http_header(provider, "x-existing-header"),
        Some("keep")
    );
    assert_eq!(
        provider_http_header(
            provider,
            crate::gateway::X_OPENAI_ACTOR_AUTHORIZATION_HEADER
        ),
        Some(crate::gateway::CODEXMANAGER_IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
    );
}

#[test]
fn direct_config_only_removes_manager_owned_catalog() {
    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let managed = format!(
        "model_catalog_json = {:?}\n",
        managed_catalog.to_string_lossy()
    );

    let removed = patch_config_for_direct(Some(managed), &managed_catalog, None)
        .expect("remove managed catalog");
    assert!(!removed.contains("model_catalog_json"));

    let restored = patch_config_for_direct(
        Some(format!(
            "model_catalog_json = {:?}\n",
            managed_catalog.to_string_lossy()
        )),
        &managed_catalog,
        Some("/home/test/custom-models.json"),
    )
    .expect("restore prior catalog");
    assert!(restored.contains("model_catalog_json = \"/home/test/custom-models.json\""));

    let custom = "model_catalog_json = \"/home/test/owned-by-user.json\"\n";
    let preserved = patch_config_for_direct(Some(custom.to_string()), &managed_catalog, None)
        .expect("preserve user catalog");
    assert!(preserved.contains("/home/test/owned-by-user.json"));
}

#[test]
fn invalid_toml_is_rejected() {
    assert!(patch_config_for_gateway(
        Some("bad = [".to_string()),
        "http://x/v1",
        Path::new("/tmp/gateway-models.json"),
        false,
        "cm-managed-key",
        false,
    )
    .is_err());
}

#[test]
fn rotation_strategy_selects_catalog_ownership() {
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_ACCOUNT
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::OfficialAccountPool
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_AGGREGATE_API
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_HYBRID
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
}

#[test]
fn api_key_candidates_expose_route_and_catalog_ownership() {
    for (rotation_strategy, catalog_source) in [
        (crate::apikey_profile::ROTATION_ACCOUNT, "official"),
        (crate::apikey_profile::ROTATION_AGGREGATE_API, "managed"),
        (crate::apikey_profile::ROTATION_HYBRID, "managed"),
        (
            crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST,
            "managed",
        ),
    ] {
        let candidate = api_key_candidate(ApiKeyCodexProfileCandidate {
            id: format!("key-{rotation_strategy}"),
            name: Some("Platform key".to_string()),
            model_slug: None,
            reasoning_effort: None,
            rotation_strategy: rotation_strategy.to_string(),
            status: "active".to_string(),
        })
        .expect("active candidate");

        assert_eq!(candidate.rotation_strategy, rotation_strategy);
        assert_eq!(candidate.catalog_source, catalog_source);
    }
}

#[test]
fn usable_account_token_candidates_by_account_indexes_candidates() {
    let candidates = usable_account_token_candidates_by_account(vec![
        AccountTokenCandidate {
            account_id: "acc-ready".to_string(),
            has_access_token: true,
            has_refresh_token: true,
            last_refresh: 10,
        },
        AccountTokenCandidate {
            account_id: "acc-no-access".to_string(),
            has_access_token: false,
            has_refresh_token: true,
            last_refresh: 11,
        },
        AccountTokenCandidate {
            account_id: "acc-no-refresh".to_string(),
            has_access_token: true,
            has_refresh_token: false,
            last_refresh: 12,
        },
    ]);

    assert_eq!(candidates.len(), 3);
    assert_eq!(
        candidates
            .get("acc-ready")
            .map(|candidate| candidate.last_refresh),
        Some(10)
    );
    assert_eq!(
        candidates
            .get("acc-no-access")
            .map(|candidate| candidate.last_refresh),
        Some(11)
    );
    assert_eq!(
        candidates
            .get("acc-no-refresh")
            .map(|candidate| candidate.last_refresh),
        Some(12)
    );
}

#[test]
fn list_candidates_uses_active_account_projection_and_usable_tokens() {
    let _lock = crate::test_env_guard();
    let dir = temp_profile("codex-profile-candidates");
    fs::create_dir_all(&dir).expect("mkdir temp dir");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    let mut active = test_account("acc-active-candidate", "active");
    active.label = "Active Candidate".to_string();
    active.group_name = Some("candidate-group".to_string());
    let mut disabled = test_account("acc-disabled-candidate", "disabled");
    disabled.label = "Disabled Candidate".to_string();
    let mut force_enabled = test_account("acc-force-candidate", "force_enabled");
    force_enabled.label = "Force Candidate".to_string();
    storage
        .insert_account(&active)
        .expect("insert active account");
    storage
        .insert_account(&disabled)
        .expect("insert disabled account");
    storage
        .insert_account(&force_enabled)
        .expect("insert force-enabled account");
    storage
        .insert_token(&test_token("acc-active-candidate", "access", "refresh"))
        .expect("insert active token");
    storage
        .insert_token(&test_token("acc-disabled-candidate", "access", "refresh"))
        .expect("insert disabled token");
    storage
        .insert_token(&test_token("acc-force-candidate", "access", "refresh"))
        .expect("insert force-enabled token");
    storage
        .insert_account(&test_account("acc-missing-refresh", "active"))
        .expect("insert missing refresh account");
    storage
        .insert_token(&test_token("acc-missing-refresh", "access", ""))
        .expect("insert missing refresh token");
    drop(storage);

    let result = list_candidates().expect("list candidates");

    assert_eq!(result.accounts.len(), 2);
    let account = &result.accounts[0];
    assert_eq!(account.id, "acc-active-candidate");
    assert_eq!(account.label, "Active Candidate");
    assert_eq!(account.group_name.as_deref(), Some("candidate-group"));
    assert_eq!(account.status, "active");
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("cgpt-acc-active-candidate")
    );
    assert_eq!(
        account.workspace_id.as_deref(),
        Some("ws-acc-active-candidate")
    );
    assert_eq!(account.issuer, "issuer-acc-active-candidate");
    assert_eq!(account.last_refresh, 123);
    let force_account = &result.accounts[1];
    assert_eq!(force_account.id, "acc-force-candidate");
    assert_eq!(force_account.status, "force_enabled");
    cleanup_profile(&dir);
}

#[test]
fn restore_optional_file_removes_files_that_were_missing() {
    let dir = temp_profile("restore-missing");
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("auth.json");
    fs::write(&path, "{}").expect("write");

    restore_optional_file(&path, None).expect("restore missing");

    assert!(!path.exists());
    cleanup_profile(&dir);
}

#[test]
fn auth_json_shapes_match_codex_modes() {
    let now = now_ts();
    let account = AccountDirectAuthProfile {
        id: "acc-1".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("chatgpt-1".to_string()),
        status: "active".to_string(),
    };
    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    let direct = build_direct_auth_json(&account, &token).expect("direct auth");
    let gateway = build_gateway_auth_json("cm-key").expect("gateway auth");

    assert!(auth_json_has_tokens(&direct));
    assert!(!auth_json_is_gateway(&direct));
    assert!(auth_json_is_gateway(&gateway));
}

fn detect_login_mode_for_config(name: &str, config: &str) -> CodexProfileMode {
    let _env_lock = crate::test_env_guard();
    let _service_addr = EnvGuard::set("CODEXMANAGER_SERVICE_ADDR", "127.0.0.1:48760");
    let dir = temp_profile(name);
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(dir.join(CONFIG_FILE), config).expect("write config");

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), None);
    cleanup_profile(&dir);
    mode
}

#[test]
fn experimental_gateway_base_without_token_keeps_login_mode_direct() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-without-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_blank_token_keeps_login_mode_direct() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-blank-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "   "

[model_providers.default]
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "\t"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_ignores_token_from_other_provider() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-other-provider-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"

[model_providers.other]
base_url = "https://example.test/v1"
experimental_bearer_token = "other-provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_accepts_top_level_token() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-top-level-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "http://localhost:48760/v1"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_accepts_current_provider_token() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-current-provider-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_prefers_current_provider_external_base_url_over_stale_local_root() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-provider-external-root-local",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "https://example.test/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_prefers_current_provider_local_base_url_over_stale_external_root() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-provider-local-root-external",
        r#"
model_provider = "default"
base_url = "https://example.test/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "http://127.0.0.1:48760/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn managed_gateway_provider_does_not_require_experimental_token() {
    let mode = detect_login_mode_for_config(
        "managed-gateway-without-experimental-token",
        r#"
model_provider = "cm"

[model_providers.cm]
base_url = "http://localhost:48760/v1"
wire_api = "responses"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_config_overrides_login_tokens_and_stale_direct_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("experimental-gateway-detection");
    let _db_guard = set_test_db(&dir);
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(
        dir.join(CONFIG_FILE),
        r#"
model_provider = "default"
wire_api = "responses"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "cm-key"

[model_providers.default]
name = "OpenAI"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "cm-key"
"#,
    )
    .expect("write config");
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: CodexProfileMode::DirectAccount,
        account_id: Some("acc-1".to_string()),
        api_key_id: None,
        gateway_base_url: None,
        supports_websockets: None,
        provider_id: PROVIDER_ID.to_string(),
        updated_at: now_ts(),
    };

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), Some(&marker));

    assert!(matches!(mode, CodexProfileMode::Gateway));
    fs::write(
        dir.join(MARKER_FILE),
        serde_json::to_string_pretty(&marker).expect("marker json"),
    )
    .expect("write stale marker");
    let status = status_for_profile(&dir).expect("profile status");
    assert!(matches!(status.mode, CodexProfileMode::Gateway));
    assert_eq!(status.provider_id, "default");
    assert_eq!(
        status.gateway_base_url.as_deref(),
        Some("http://localhost:48760/v1")
    );
    assert_eq!(status.selected_account_id, None);
    assert_eq!(status.last_applied_at, None);
    assert_eq!(
        target_history_provider_for_profile(&dir).expect("history provider"),
        "default"
    );
    cleanup_profile(&dir);
}

#[test]
fn experimental_non_gateway_base_url_keeps_login_mode_direct() {
    let dir = temp_profile("experimental-non-gateway-detection");
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(
        dir.join(CONFIG_FILE),
        r#"
model_provider = "default"
base_url = "http://localhost:12345/v1"
experimental_bearer_token = "other-key"

[model_providers.default]
base_url = "http://localhost:12345/v1"
experimental_bearer_token = "other-key"
"#,
    )
    .expect("write config");

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), None);

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
    cleanup_profile(&dir);
}

#[test]
fn write_profile_files_uses_internal_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("internal-marker");
    let _db_guard = set_test_db(&dir);
    let state = ManagedState {
        profile_dir: profile_key(&dir),
        mode: CodexProfileMode::Gateway,
        account_id: None,
        api_key_id: Some("key-1".to_string()),
        gateway_base_url: Some("http://localhost:48760/v1".to_string()),
        supports_websockets: Some(false),
        provider_id: PROVIDER_ID.to_string(),
        previous_model_catalog_json: None,
        updated_at: now_ts(),
    };

    write_profile_files(&dir, "{}", "", state).expect("write profile");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!dir.join(MARKER_FILE).exists());
    let status = status_for_profile(&dir).expect("status");
    assert!(matches!(status.mode, CodexProfileMode::Gateway));
    assert_eq!(
        status.marker_path,
        paths.marker_path.to_string_lossy().to_string()
    );
    cleanup_profile(&dir);
}

#[test]
fn legacy_marker_migrates_to_internal_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("legacy-marker");
    let _db_guard = set_test_db(&dir);
    fs::create_dir_all(&dir).expect("mkdir profile");
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: CodexProfileMode::DirectAccount,
        account_id: Some("acc-1".to_string()),
        api_key_id: None,
        gateway_base_url: None,
        supports_websockets: None,
        provider_id: PROVIDER_ID.to_string(),
        updated_at: now_ts(),
    };
    fs::write(
        dir.join(MARKER_FILE),
        serde_json::to_string_pretty(&marker).expect("marker json"),
    )
    .expect("write legacy marker");

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!paths.legacy_marker_path.exists());
    assert!(matches!(status.mode, CodexProfileMode::DirectAccount));
    cleanup_profile(&dir);
}

#[test]
fn legacy_history_backups_migrate_and_are_pruned() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("legacy-history-backups");
    let _db_guard = set_test_db(&dir);
    let legacy_root = dir.join(HISTORY_BACKUP_DIR);
    fs::create_dir_all(&legacy_root).expect("mkdir legacy root");
    for index in 0..5 {
        let backup_dir = legacy_root.join(format!("backup-{index}"));
        fs::create_dir_all(&backup_dir).expect("mkdir legacy backup");
        fs::write(backup_dir.join("file.txt"), format!("backup-{index}"))
            .expect("write legacy backup");
    }

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(!paths.legacy_history_backup_root.exists());
    assert!(paths.history_backup_root.exists());
    assert_eq!(status.history_backup_count, MAX_HISTORY_BACKUPS_PER_PROFILE);
    cleanup_profile(&dir);
}

#[test]
fn history_repair_aligns_direct_and_gateway_providers() {
    let dir = temp_profile("history-provider");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-provider";
    let (rollout_path, event_line) = write_test_rollout(&dir, thread_id, PROVIDER_ID);
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");

    let direct = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(direct.warnings.is_empty(), "{:?}", direct.warnings);
    assert_eq!(direct.changed_rollout_file_count, 1);
    assert_eq!(direct.updated_sqlite_row_count, 1);
    assert_eq!(
        sqlite_provider(&dir, thread_id),
        DEFAULT_HISTORY_PROVIDER_ID
    );
    let direct_rollout = fs::read_to_string(&rollout_path).expect("read direct rollout");
    assert!(direct_rollout.contains("\"model_provider\":\"openai\""));
    assert!(direct_rollout.contains(&event_line));
    assert!(!dir.join(HISTORY_BACKUP_DIR).exists());
    let direct_backup = direct.backup_dir.as_ref().expect("direct backup dir");
    assert!(direct_backup.contains(MANAGED_PROFILE_ROOT_DIR));
    let direct_backup_path = PathBuf::from(direct_backup);
    assert!(direct_backup_path.join(STATE_DB_FILE).exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-wal"))
        .exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-shm"))
        .exists());
    assert!(direct_backup_path
        .join(HISTORY_BACKUP_MANIFEST_FILE)
        .exists());

    let gateway = repair_history_for_provider(&dir, PROVIDER_ID);

    assert!(gateway.warnings.is_empty(), "{:?}", gateway.warnings);
    assert_eq!(gateway.changed_rollout_file_count, 1);
    assert_eq!(gateway.updated_sqlite_row_count, 1);
    assert_eq!(sqlite_provider(&dir, thread_id), PROVIDER_ID);
    let gateway_rollout = fs::read_to_string(&rollout_path).expect("read gateway rollout");
    assert!(gateway_rollout.contains("\"model_provider\":\"cm\""));
    assert!(gateway_rollout.contains(&event_line));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_appends_missing_session_index_once() {
    let dir = temp_profile("history-index");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index";
    create_state_db(&dir, thread_id, DEFAULT_HISTORY_PROVIDER_ID);

    let first = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);
    let second = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(first.added_session_index_entry_count, 1);
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(second.added_session_index_entry_count, 0);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert_eq!(index.lines().count(), 1);
    assert!(index.contains(thread_id));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_handles_sqlite_with_only_updated_at_ms() {
    let dir = temp_profile("history-index-updated-ms-only");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index-ms";
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000000)",
        params![thread_id, DEFAULT_HISTORY_PROVIDER_ID],
    )
    .expect("insert thread");
    drop(conn);

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert_eq!(summary.added_session_index_entry_count, 1);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert!(index.contains(thread_id));
    assert!(index.contains("2026"));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_reports_sqlite_lock_as_warning() {
    let dir = temp_profile("history-locked");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-locked";
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");
    let lock_conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open lock sqlite");
    lock_conn
        .execute("BEGIN IMMEDIATE", [])
        .expect("begin immediate");

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert_eq!(summary.updated_sqlite_row_count, 0);
    assert!(
        summary
            .warnings
            .iter()
            .any(|warning| warning.contains("update Codex history sqlite provider failed")),
        "{:?}",
        summary.warnings
    );
    drop(lock_conn);
    cleanup_profile(&dir);
}
