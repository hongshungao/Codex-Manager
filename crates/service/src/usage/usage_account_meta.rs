use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, parse_id_token_claims,
};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use std::collections::HashMap;

pub(crate) fn clean_header_value(value: Option<String>) -> Option<String> {
    match value {
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => None,
    }
}

fn resolve_workspace_header(
    workspace_id: Option<String>,
    chatgpt_account_id: Option<String>,
) -> Option<String> {
    clean_header_value(workspace_id).or_else(|| clean_header_value(chatgpt_account_id))
}

pub(crate) fn workspace_header_for_account(account: &Account) -> Option<String> {
    resolve_workspace_header(account.workspace_id.clone(), account.chatgpt_account_id.clone())
}

pub(crate) fn build_workspace_map_from_accounts(
    accounts: &[Account],
) -> HashMap<String, Option<String>> {
    let mut workspace_map = HashMap::with_capacity(accounts.len());
    for account in accounts {
        let workspace_id = workspace_header_for_account(account);
        workspace_map.insert(account.id.clone(), workspace_id);
    }
    workspace_map
}

#[allow(dead_code)]
pub(crate) fn build_workspace_map(storage: &Storage) -> HashMap<String, Option<String>> {
    storage
        .list_accounts()
        .map(|accounts| build_workspace_map_from_accounts(&accounts))
        .unwrap_or_default()
}

#[allow(dead_code)]
pub(crate) fn resolve_workspace_id_for_account(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .find_account_by_id(account_id)
        .ok()
        .flatten()
        .and_then(|account| workspace_header_for_account(&account))
}

pub(crate) fn derive_account_meta(token: &Token) -> (Option<String>, Option<String>) {
    let mut chatgpt_account_id = None;
    let mut workspace_id = None;

    if let Ok(claims) = parse_id_token_claims(&token.id_token) {
        if let Some(auth) = claims.auth {
            if chatgpt_account_id.is_none() {
                chatgpt_account_id = clean_header_value(auth.chatgpt_account_id);
            }
        }
        if workspace_id.is_none() {
            workspace_id = clean_header_value(claims.workspace_id);
        }
    }

    if workspace_id.is_none() {
        workspace_id = clean_header_value(
            extract_workspace_id(&token.id_token).or_else(|| extract_workspace_id(&token.access_token)),
        );
    }
    if chatgpt_account_id.is_none() {
        chatgpt_account_id = clean_header_value(
            extract_chatgpt_account_id(&token.id_token)
                .or_else(|| extract_chatgpt_account_id(&token.access_token)),
        );
    }
    if workspace_id.is_none() {
        workspace_id = chatgpt_account_id.clone();
    }

    (chatgpt_account_id, workspace_id)
}

pub(crate) fn patch_account_meta(
    storage: &Storage,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) {
    let Ok(account) = storage.find_account_by_id(account_id) else {
        return;
    };
    let Some(mut account) = account else {
        return;
    };

    if apply_account_meta_patch(&mut account, chatgpt_account_id, workspace_id) {
        account.updated_at = now_ts();
        let _ = storage.insert_account(&account);
    }
}

pub(crate) fn patch_account_meta_cached(
    storage: &Storage,
    accounts: &mut HashMap<String, Account>,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) {
    if let Some(account) = accounts.get_mut(account_id) {
        if apply_account_meta_patch(account, chatgpt_account_id, workspace_id) {
            account.updated_at = now_ts();
            let _ = storage.insert_account(account);
        }
        return;
    }

    patch_account_meta(storage, account_id, chatgpt_account_id, workspace_id);
}

pub(crate) fn patch_account_meta_in_place(
    account: &mut Account,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) -> bool {
    apply_account_meta_patch(account, chatgpt_account_id, workspace_id)
}

fn is_invalid_upstream_scope_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    // `auth0|...` / `google-oauth2|...` 等 subject 不能作为 ChatGPT workspace/account header。
    trimmed.contains('|') || trimmed.starts_with("import-sub-")
}

fn apply_account_meta_patch(
    account: &mut Account,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) -> bool {
    let mut changed = false;
    let next_chatgpt_account_id = clean_header_value(chatgpt_account_id);
    let next_workspace_id = clean_header_value(workspace_id);

    if let Some(next) = next_chatgpt_account_id.clone() {
        let current = account.chatgpt_account_id.as_deref().unwrap_or("").trim();
        if current.is_empty() || is_invalid_upstream_scope_value(current) {
            if current != next {
                account.chatgpt_account_id = Some(next);
                changed = true;
            }
        }
    }

    let desired_workspace = next_workspace_id.or_else(|| next_chatgpt_account_id.clone());
    if let Some(next) = desired_workspace {
        let current = account.workspace_id.as_deref().unwrap_or("").trim();
        if current.is_empty() || is_invalid_upstream_scope_value(current) {
            if current != next {
                account.workspace_id = Some(next);
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::{
        build_workspace_map, build_workspace_map_from_accounts, clean_header_value,
        patch_account_meta_cached, resolve_workspace_id_for_account,
    };
    use codexmanager_core::storage::{now_ts, Account, Storage};
    use std::collections::HashMap;

    fn build_account(id: &str, workspace_id: Option<&str>, chatgpt_account_id: Option<&str>) -> Account {
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
        assert_eq!(clean_header_value(Some(" abc ".to_string())), Some("abc".to_string()));
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
        assert_eq!(workspace_map.get("acc-2").cloned(), Some(Some("chatgpt-2".to_string())));
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
}

