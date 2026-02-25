use codexmanager_core::storage::{
    now_ts, Account, ApiKey, RequestLog, RequestTokenStat, Storage, Token, UsageSnapshotRecord,
};

#[test]
fn storage_can_insert_account_and_token() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "healthy".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).expect("insert token");

    assert_eq!(storage.account_count().expect("count accounts"), 1);
    assert_eq!(storage.token_count().expect("count tokens"), 1);
}

#[test]
fn storage_can_find_token_and_account_by_account_id() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-find-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_find".to_string()),
        workspace_id: Some("org_find".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let token = Token {
        account_id: "acc-find-1".to_string(),
        id_token: "id-find".to_string(),
        access_token: "access-find".to_string(),
        refresh_token: "refresh-find".to_string(),
        api_key_access_token: Some("api-key-find".to_string()),
        last_refresh: now_ts(),
    };
    storage.insert_token(&token).expect("insert token");

    let found_account = storage
        .find_account_by_id("acc-find-1")
        .expect("find account")
        .expect("account exists");
    assert_eq!(found_account.id, "acc-find-1");

    let found_token = storage
        .find_token_by_account_id("acc-find-1")
        .expect("find token")
        .expect("token exists");
    assert_eq!(found_token.account_id, "acc-find-1");
    assert_eq!(found_token.api_key_access_token.as_deref(), Some("api-key-find"));

    assert!(storage
        .find_account_by_id("missing-account")
        .expect("find missing account")
        .is_none());
    assert!(storage
        .find_token_by_account_id("missing-account")
        .expect("find missing token")
        .is_none());
}

#[test]
fn storage_login_session_roundtrip() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let session = codexmanager_core::storage::LoginSession {
        login_id: "login-1".to_string(),
        code_verifier: "verifier".to_string(),
        state: "state".to_string(),
        status: "pending".to_string(),
        error: None,
        note: None,
        tags: None,
        group_name: None,
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage
        .insert_login_session(&session)
        .expect("insert session");
    let loaded = storage
        .get_login_session("login-1")
        .expect("load session")
        .expect("session exists");
    assert_eq!(loaded.status, "pending");
}

#[test]
fn storage_can_update_account_status() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    storage
        .update_account_status("acc-1", "inactive")
        .expect("update status");

    let loaded = storage
        .list_accounts()
        .expect("list accounts")
        .into_iter()
        .find(|acc| acc.id == "acc-1")
        .expect("account exists");

    assert_eq!(loaded.status, "inactive");
}

#[test]
fn storage_updates_account_status_only_when_changed() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let account = Account {
        id: "acc-conditional-1".to_string(),
        label: "main".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("acct_123".to_string()),
        workspace_id: Some("org_123".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert account");

    let unchanged = storage
        .update_account_status_if_changed("acc-conditional-1", "active")
        .expect("conditional update unchanged");
    assert!(!unchanged);

    let changed = storage
        .update_account_status_if_changed("acc-conditional-1", "inactive")
        .expect("conditional update changed");
    assert!(changed);

    let loaded = storage
        .find_account_by_id("acc-conditional-1")
        .expect("find account")
        .expect("account exists");
    assert_eq!(loaded.status, "inactive");
}

#[test]
fn latest_usage_snapshots_break_ties_by_latest_id() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let tie_ts = now_ts();

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts,
        })
        .expect("insert first snapshot");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: Some(30.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts,
        })
        .expect("insert second snapshot with same timestamp");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-2".to_string(),
            used_percent: Some(50.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: tie_ts - 10,
        })
        .expect("insert snapshot for acc-2");

    let latest = storage
        .latest_usage_snapshots_by_account()
        .expect("read latest snapshots");

    assert_eq!(latest.len(), 2);
    assert_eq!(latest[0].account_id, "acc-1");

    let acc1 = latest
        .iter()
        .find(|item| item.account_id == "acc-1")
        .expect("acc-1 exists");
    assert_eq!(acc1.used_percent, Some(30.0));
}

#[test]
fn request_logs_support_prefixed_query_filters() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-alpha-extra".to_string()),
            account_id: Some("acc-1".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5.1".to_string()),
            reasoning_effort: Some("low".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/v1/responses".to_string()),
            status_code: Some(201),
            input_tokens: Some(11),
            cached_input_tokens: Some(3),
            output_tokens: Some(7),
            total_tokens: Some(18),
            reasoning_output_tokens: Some(2),
            estimated_cost_usd: Some(0.0),
            error: None,
            created_at: now_ts() - 2,
        })
        .expect("insert request log 0");

    storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-alpha".to_string()),
            account_id: Some("acc-1".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5.1".to_string()),
            reasoning_effort: Some("low".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/v1/responses".to_string()),
            status_code: Some(200),
            input_tokens: Some(9),
            cached_input_tokens: Some(1),
            output_tokens: Some(5),
            total_tokens: Some(14),
            reasoning_output_tokens: Some(1),
            estimated_cost_usd: Some(0.0),
            error: None,
            created_at: now_ts() - 1,
        })
        .expect("insert request log 1");

    storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-beta".to_string()),
            account_id: Some("acc-2".to_string()),
            request_path: "/v1/models".to_string(),
            method: "GET".to_string(),
            model: Some("gpt-4.1".to_string()),
            reasoning_effort: Some("xhigh".to_string()),
            upstream_url: Some("https://api.openai.com/v1/models".to_string()),
            status_code: Some(503),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: Some(0.0),
            error: Some("upstream timeout".to_string()),
            created_at: now_ts(),
        })
        .expect("insert request log 2");

    let method_filtered = storage
        .list_request_logs(Some("method:GET"), 100)
        .expect("filter by method");
    assert_eq!(method_filtered.len(), 1);
    assert_eq!(method_filtered[0].method, "GET");

    let status_filtered = storage
        .list_request_logs(Some("status:5xx"), 100)
        .expect("filter by status range");
    assert_eq!(status_filtered.len(), 1);
    assert_eq!(status_filtered[0].status_code, Some(503));

    let key_filtered = storage
        .list_request_logs(Some("key:key-alpha"), 100)
        .expect("filter by key id");
    assert_eq!(key_filtered.len(), 2);

    let key_exact_filtered = storage
        .list_request_logs(Some("key:=key-alpha"), 100)
        .expect("filter by exact key id");
    assert_eq!(key_exact_filtered.len(), 1);
    assert_eq!(key_exact_filtered[0].key_id.as_deref(), Some("key-alpha"));

    let fallback_filtered = storage
        .list_request_logs(Some("timeout"), 100)
        .expect("fallback fuzzy query");
    assert_eq!(fallback_filtered.len(), 1);
    assert_eq!(fallback_filtered[0].error.as_deref(), Some("upstream timeout"));
}

#[test]
fn request_log_today_summary_reads_from_token_stats_table() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();
    let request_log_id = storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-summary".to_string()),
            account_id: Some("acc-summary".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
            status_code: Some(200),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
        })
        .expect("insert request log");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some("key-summary".to_string()),
            account_id: Some("acc-summary".to_string()),
            model: Some("gpt-5.3-codex".to_string()),
            input_tokens: Some(120),
            cached_input_tokens: Some(80),
            output_tokens: Some(22),
            total_tokens: Some(142),
            reasoning_output_tokens: Some(9),
            estimated_cost_usd: Some(0.33),
            created_at,
        })
        .expect("insert token stat");

    let summary = storage
        .summarize_request_logs_between(created_at - 1, created_at + 1)
        .expect("summarize");
    assert_eq!(summary.input_tokens, 120);
    assert_eq!(summary.cached_input_tokens, 80);
    assert_eq!(summary.output_tokens, 22);
    assert_eq!(summary.reasoning_output_tokens, 9);
    assert!(summary.estimated_cost_usd > 0.32);
}

#[test]
fn clear_request_logs_keeps_token_stats_for_usage_summary() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let created_at = now_ts();
    let request_log_id = storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-clear".to_string()),
            account_id: Some("acc-clear".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            upstream_url: Some("https://chatgpt.com/backend-api/codex/responses".to_string()),
            status_code: Some(200),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at,
        })
        .expect("insert request log");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some("key-clear".to_string()),
            account_id: Some("acc-clear".to_string()),
            model: Some("gpt-5.3-codex".to_string()),
            input_tokens: Some(100),
            cached_input_tokens: Some(30),
            output_tokens: Some(20),
            total_tokens: Some(120),
            reasoning_output_tokens: Some(5),
            estimated_cost_usd: Some(0.12),
            created_at,
        })
        .expect("insert token stat");

    storage.clear_request_logs().expect("clear request logs");

    let logs = storage.list_request_logs(None, 100).expect("list logs");
    assert!(logs.is_empty(), "request logs should be cleared");

    let summary = storage
        .summarize_request_logs_between(created_at - 1, created_at + 1)
        .expect("summarize");
    assert_eq!(summary.input_tokens, 100);
    assert_eq!(summary.cached_input_tokens, 30);
    assert_eq!(summary.output_tokens, 20);
    assert_eq!(summary.reasoning_output_tokens, 5);
    assert!(summary.estimated_cost_usd > 0.11);
}

#[test]
fn usage_snapshots_can_prune_history_per_account() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let now = now_ts();

    for offset in 0..5 {
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-prune-1".to_string(),
                used_percent: Some(10.0 + offset as f64),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now + offset,
            })
            .expect("insert acc-prune-1 snapshot");
    }

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-prune-2".to_string(),
            used_percent: Some(30.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert acc-prune-2 snapshot");

    let deleted = storage
        .prune_usage_snapshots_for_account("acc-prune-1", 2)
        .expect("prune snapshots");
    assert_eq!(deleted, 3);

    let kept = storage
        .usage_snapshot_count_for_account("acc-prune-1")
        .expect("count kept");
    assert_eq!(kept, 2);

    let untouched = storage
        .usage_snapshot_count_for_account("acc-prune-2")
        .expect("count untouched");
    assert_eq!(untouched, 1);
}

#[test]
fn storage_api_keys_include_profile_fields() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_api_key(&ApiKey {
            id: "key-1".to_string(),
            name: Some("main".to_string()),
            model_slug: Some("claude-sonnet-4".to_string()),
            reasoning_effort: Some("medium".to_string()),
            client_type: "claude_code".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: Some("https://api.anthropic.com".to_string()),
            static_headers_json: Some("{\"anthropic-version\":\"2023-06-01\"}".to_string()),
            key_hash: "hash-1".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert key");

    let key = storage
        .list_api_keys()
        .expect("list keys")
        .into_iter()
        .find(|item| item.id == "key-1")
        .expect("key exists");
    assert_eq!(key.client_type, "claude_code");
    assert_eq!(key.protocol_type, "anthropic_native");
    assert_eq!(key.auth_scheme, "x_api_key");
    assert_eq!(key.model_slug.as_deref(), Some("claude-sonnet-4"));
}

#[test]
fn storage_can_roundtrip_api_key_secret() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    storage
        .insert_api_key(&ApiKey {
            id: "key-secret-1".to_string(),
            name: Some("secret".to_string()),
            model_slug: None,
            reasoning_effort: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "hash-secret-1".to_string(),
            status: "active".to_string(),
            created_at: now_ts(),
            last_used_at: None,
        })
        .expect("insert key");

    storage
        .upsert_api_key_secret("key-secret-1", "sk-secret-value")
        .expect("upsert secret");

    let loaded = storage
        .find_api_key_secret_by_id("key-secret-1")
        .expect("load secret");
    assert_eq!(loaded.as_deref(), Some("sk-secret-value"));

    storage.delete_api_key("key-secret-1").expect("delete key");
    let removed = storage
        .find_api_key_secret_by_id("key-secret-1")
        .expect("load removed secret");
    assert!(removed.is_none());
}

