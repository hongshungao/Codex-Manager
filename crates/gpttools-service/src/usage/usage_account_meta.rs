use gpttools_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, extract_workspace_name, parse_id_token_claims,
};
use gpttools_core::storage::{now_ts, Account, Storage, Token};
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
    resolve_workspace_header(
        account.workspace_id.clone(),
        account.chatgpt_account_id.clone(),
    )
}

pub(crate) fn build_workspace_map(storage: &Storage) -> HashMap<String, Option<String>> {
    let mut workspace_map = HashMap::new();
    if let Ok(accounts) = storage.list_accounts() {
        for account in accounts {
            let workspace_id = workspace_header_for_account(&account);
            workspace_map.insert(account.id, workspace_id);
        }
    }
    workspace_map
}

pub(crate) fn resolve_workspace_id_for_account(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .list_accounts()
        .ok()
        .and_then(|accounts| {
            accounts
                .into_iter()
                .find(|account| account.id == account_id)
                .and_then(|account| workspace_header_for_account(&account))
        })
}

pub(crate) fn derive_account_meta(
    token: &Token,
) -> (Option<String>, Option<String>, Option<String>) {
    let mut chatgpt_account_id = None;
    let mut workspace_id = None;
    let mut workspace_name = None;

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
            extract_workspace_id(&token.id_token)
                .or_else(|| extract_workspace_id(&token.access_token)),
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
    if workspace_name.is_none() {
        workspace_name = clean_header_value(
            extract_workspace_name(&token.id_token)
                .or_else(|| extract_workspace_name(&token.access_token)),
        );
    }

    (chatgpt_account_id, workspace_id, workspace_name)
}

pub(crate) fn patch_account_meta(
    storage: &Storage,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
) {
    let Ok(accounts) = storage.list_accounts() else {
        return;
    };
    let Some(mut account) = accounts.into_iter().find(|acc| acc.id == account_id) else {
        return;
    };

    let mut changed = false;
    if account.chatgpt_account_id.as_deref().unwrap_or("").trim().is_empty()
        && chatgpt_account_id.is_some()
    {
        account.chatgpt_account_id = chatgpt_account_id;
        changed = true;
    }
    if account.workspace_id.as_deref().unwrap_or("").trim().is_empty() && workspace_id.is_some() {
        account.workspace_id = workspace_id;
        changed = true;
    }
    if account.workspace_name.as_deref().unwrap_or("").trim().is_empty()
        && workspace_name.is_some()
    {
        account.workspace_name = workspace_name;
        changed = true;
    }

    if changed {
        account.updated_at = now_ts();
        let _ = storage.insert_account(&account);
    }
}

#[cfg(test)]
mod tests {
    use super::{build_workspace_map, clean_header_value, resolve_workspace_id_for_account};
    use gpttools_core::storage::{now_ts, Account, Storage};

    fn build_account(id: &str, workspace_id: Option<&str>, chatgpt_account_id: Option<&str>) -> Account {
        Account {
            id: id.to_string(),
            label: format!("label-{id}"),
            issuer: "issuer".to_string(),
            chatgpt_account_id: chatgpt_account_id.map(|value| value.to_string()),
            workspace_id: workspace_id.map(|value| value.to_string()),
            workspace_name: None,
            note: None,
            tags: None,
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
}
