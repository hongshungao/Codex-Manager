use super::{mark_usage_unreachable_if_needed, should_retry_with_refresh};
use crate::account_availability::Availability;
use crate::usage_snapshot_store::apply_status_from_snapshot;
use gpttools_core::storage::{now_ts, Account, Storage, UsageSnapshotRecord};

#[test]
fn apply_status_marks_inactive_on_missing() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        workspace_name: None,
        note: None,
        tags: None,
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

    let availability = apply_status_from_snapshot(&storage, &record);
    assert!(matches!(availability, Availability::Unavailable(_)));
    let loaded = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-1")
        .expect("exists");
    assert_eq!(loaded.status, "inactive");
}

#[test]
fn mark_usage_unreachable_only_for_usage_status_error() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-2".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        workspace_name: None,
        note: None,
        tags: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    mark_usage_unreachable_if_needed(&storage, "acc-2", "network timeout");
    let still_active = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(still_active.status, "active");

    mark_usage_unreachable_if_needed(
        &storage,
        "acc-2",
        "usage endpoint status 500 Internal Server Error",
    );
    let inactive = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(inactive.status, "inactive");
}

#[test]
fn refresh_retry_filter_matches_auth_failures() {
    assert!(should_retry_with_refresh("usage endpoint status 401"));
    assert!(should_retry_with_refresh("usage endpoint status 403"));
    assert!(!should_retry_with_refresh("usage endpoint status 429"));
}
