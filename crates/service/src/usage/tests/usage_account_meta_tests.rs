use super::{
    build_workspace_map, build_workspace_map_from_accounts, clean_header_value,
    patch_account_meta_cached, resolve_workspace_id_for_account,
};
use codexmanager_core::storage::{now_ts, Account, Storage};
use std::collections::HashMap;

fn build_account(
    id: &str,
    workspace_id: Option<&str>,
    chatgpt_account_id: Option<&str>,
) -> Account {
    Account {
        id: id.to_string(),
        label: format!("label-{id}"),
        issuer: "issuer".to_string(),
        chatgpt_account_id: chatgpt_account_id.map(|value| value.to_string()),
        workspace_id: workspace_id.map(|value| value.to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

#[test]
fn clean_header_value_trims_and_drops_empty() {
    assert_eq!(
        clean_header_value(Some(" abc ".to_string())),
        Some("abc".to_string())
    );
    assert_eq!(clean_header_value(Some("   ".to_string())), None);
    assert_eq!(clean_header_value(None), None);
}

#[test]
fn resolve_workspace_prefers_workspace_then_chatgpt() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-1", Some(" ws-primary "), Some("chatgpt-fallback"));
    storage.insert_account(&account).expect("insert");

    let resolved = resolve_workspace_id_for_account(&storage, "acc-1");
    assert_eq!(resolved, Some("ws-primary".to_string()));
}

#[test]
fn build_workspace_map_falls_back_to_chatgpt_account_id() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_account(&build_account("acc-2", Some("  "), Some(" chatgpt-2 ")))
        .expect("insert");

    let workspace_map = build_workspace_map(&storage);
    assert_eq!(
        workspace_map.get("acc-2").cloned(),
        Some(Some("chatgpt-2".to_string()))
    );
}

#[test]
fn build_workspace_map_from_accounts_uses_preloaded_snapshot() {
    let accounts = vec![
        build_account("acc-3", Some(" ws-3 "), None),
        build_account("acc-4", None, Some(" chatgpt-4 ")),
    ];
    let workspace_map = build_workspace_map_from_accounts(&accounts);
    assert_eq!(workspace_map.get("acc-3"), Some(&Some("ws-3".to_string())));
    assert_eq!(
        workspace_map.get("acc-4"),
        Some(&Some("chatgpt-4".to_string()))
    );
}

#[test]
fn patch_account_meta_cached_updates_preloaded_account_without_lookup() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-5", None, None);
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-5",
        Some("chatgpt-5".to_string()),
        Some("workspace-5".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-5")
        .expect("find")
        .expect("account");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("chatgpt-5"));
    assert_eq!(updated.workspace_id.as_deref(), Some("workspace-5"));
}

#[test]
fn patch_account_meta_cached_replaces_subject_style_scope_values() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = build_account("acc-6", Some("auth0|legacy"), Some("auth0|legacy"));
    storage.insert_account(&account).expect("insert");
    let mut account_map = HashMap::new();
    account_map.insert(account.id.clone(), account);

    patch_account_meta_cached(
        &storage,
        &mut account_map,
        "acc-6",
        Some("org-correct".to_string()),
        Some("ws-correct".to_string()),
    );

    let updated = storage
        .find_account_by_id("acc-6")
        .expect("find")
        .expect("account");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("org-correct"));
    assert_eq!(updated.workspace_id.as_deref(), Some("ws-correct"));
}
