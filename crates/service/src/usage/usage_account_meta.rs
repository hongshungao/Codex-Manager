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
    resolve_workspace_header(
        account.workspace_id.clone(),
        account.chatgpt_account_id.clone(),
    )
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
pub(crate) fn resolve_workspace_id_for_account(
    storage: &Storage,
    account_id: &str,
) -> Option<String> {
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
#[path = "tests/usage_account_meta_tests.rs"]
mod tests;
