use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, parse_id_token_claims, DEFAULT_ISSUER,
};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

use crate::storage_helpers::{account_key, open_storage};

const MAX_ERROR_ITEMS: usize = 50;
const DEFAULT_IMPORT_BATCH_SIZE: usize = 200;
const IMPORT_BATCH_SIZE_ENV: &str = "CODEXMANAGER_ACCOUNT_IMPORT_BATCH_SIZE";

#[derive(Debug, Serialize)]
pub(crate) struct AccountImportResult {
    total: usize,
    created: usize,
    updated: usize,
    failed: usize,
    errors: Vec<AccountImportError>,
}

#[derive(Debug, Serialize)]
struct AccountImportError {
    index: usize,
    message: String,
}

#[derive(Debug)]
struct ImportTokenPayload {
    access_token: String,
    id_token: String,
    refresh_token: String,
    account_id_hint: Option<String>,
}

#[derive(Default)]
struct ExistingAccountIndex {
    by_id: HashMap<String, Account>,
    by_chatgpt_account_id: HashMap<String, String>,
    next_sort: i64,
}

impl ExistingAccountIndex {
    fn build(storage: &Storage) -> Result<Self, String> {
        let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
        let mut idx = ExistingAccountIndex::default();
        for account in accounts {
            idx.next_sort = idx.next_sort.max(account.sort + 1);
            if let Some(chatgpt_account_id) = account.chatgpt_account_id.as_ref() {
                let key = chatgpt_account_id.trim();
                if !key.is_empty() {
                    idx.by_chatgpt_account_id
                        .entry(key.to_string())
                        .or_insert_with(|| account.id.clone());
                }
            }
            idx.by_id.insert(account.id.clone(), account);
        }
        Ok(idx)
    }

    fn find_existing_account_id(
        &self,
        logical_account_id: &str,
        subject_account_id: Option<&str>,
        chatgpt_account_id: Option<&str>,
    ) -> Option<String> {
        if self.by_id.contains_key(logical_account_id) {
            return Some(logical_account_id.to_string());
        }
        if let Some(subject) = subject_account_id {
            let normalized = subject.trim();
            if !normalized.is_empty() && self.by_id.contains_key(normalized) {
                return Some(normalized.to_string());
            }
        }
        if let Some(chatgpt_id) = chatgpt_account_id {
            let normalized = chatgpt_id.trim();
            if !normalized.is_empty() {
                if let Some(found) = self.by_chatgpt_account_id.get(normalized) {
                    return Some(found.clone());
                }
            }
        }
        None
    }

    fn upsert_index(&mut self, account: &Account) {
        if let Some(chatgpt_account_id) = account.chatgpt_account_id.as_ref() {
            let key = chatgpt_account_id.trim();
            if !key.is_empty() {
                self.by_chatgpt_account_id
                    .insert(key.to_string(), account.id.clone());
            }
        }
        self.by_id.insert(account.id.clone(), account.clone());
    }
}

pub(crate) fn import_account_auth_json(contents: Vec<String>) -> Result<AccountImportResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut index = ExistingAccountIndex::build(&storage)?;
    let mut result = AccountImportResult {
        total: 0,
        created: 0,
        updated: 0,
        failed: 0,
        errors: Vec::new(),
    };
    let mut progress = AccountImportProgress::new();
    let batch_size = import_batch_size();

    for content in contents {
        let items = parse_items_from_content(&content)?;
        import_items_in_batches(
            &storage,
            &mut index,
            &mut result,
            &mut progress,
            items,
            batch_size,
        );
    }

    progress.finish();
    Ok(result)
}

fn import_batch_size() -> usize {
    std::env::var(IMPORT_BATCH_SIZE_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_IMPORT_BATCH_SIZE)
}

fn import_items_in_batches(
    storage: &Storage,
    index: &mut ExistingAccountIndex,
    result: &mut AccountImportResult,
    progress: &mut AccountImportProgress,
    items: Vec<Value>,
    batch_size: usize,
) {
    if items.is_empty() {
        return;
    }
    let total_batches = items.len().div_ceil(batch_size);
    for (batch_index, batch) in items.chunks(batch_size).enumerate() {
        progress.begin_batch(batch_index + 1, total_batches, batch.len());
        for item in batch {
            result.total += 1;
            let current_index = result.total;
            match import_single_item(storage, index, item, current_index) {
                Ok(created) => {
                    if created {
                        result.created += 1;
                    } else {
                        result.updated += 1;
                    }
                    progress.on_item_success(created);
                }
                Err(err) => {
                    result.failed += 1;
                    progress.on_item_failure();
                    if result.errors.len() < MAX_ERROR_ITEMS {
                        result.errors.push(AccountImportError {
                            index: current_index,
                            message: err,
                        });
                    }
                }
            }
        }
        progress.finish_batch();
    }
}

#[derive(Debug)]
struct AccountImportProgress {
    started_at: Instant,
    processed: usize,
    created: usize,
    updated: usize,
    failed: usize,
    active_batch: Option<AccountImportBatchProgress>,
}

#[derive(Debug)]
struct AccountImportBatchProgress {
    index: usize,
    total: usize,
    size: usize,
    processed: usize,
    created: usize,
    updated: usize,
    failed: usize,
}

impl AccountImportProgress {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            processed: 0,
            created: 0,
            updated: 0,
            failed: 0,
            active_batch: None,
        }
    }

    fn begin_batch(&mut self, index: usize, total: usize, size: usize) {
        self.active_batch = Some(AccountImportBatchProgress {
            index,
            total,
            size,
            processed: 0,
            created: 0,
            updated: 0,
            failed: 0,
        });
    }

    fn on_item_success(&mut self, created: bool) {
        self.processed += 1;
        if created {
            self.created += 1;
        } else {
            self.updated += 1;
        }
        if let Some(batch) = self.active_batch.as_mut() {
            batch.processed += 1;
            if created {
                batch.created += 1;
            } else {
                batch.updated += 1;
            }
        }
    }

    fn on_item_failure(&mut self) {
        self.processed += 1;
        self.failed += 1;
        if let Some(batch) = self.active_batch.as_mut() {
            batch.processed += 1;
            batch.failed += 1;
        }
    }

    fn finish_batch(&mut self) {
        if let Some(batch) = self.active_batch.take() {
            log::info!(
                "account import batch finished: {}/{} size={} processed={} created={} updated={} failed={} total_processed={} elapsed_ms={}",
                batch.index,
                batch.total,
                batch.size,
                batch.processed,
                batch.created,
                batch.updated,
                batch.failed,
                self.processed,
                self.started_at.elapsed().as_millis()
            );
        }
    }

    fn finish(&self) {
        log::info!(
            "account import finished: processed={} created={} updated={} failed={} elapsed_ms={}",
            self.processed,
            self.created,
            self.updated,
            self.failed,
            self.started_at.elapsed().as_millis()
        );
    }
}

fn parse_items_from_content(content: &str) -> Result<Vec<Value>, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.starts_with('[') {
        let values: Vec<Value> = serde_json::from_str(trimmed)
            .map_err(|err| format!("invalid JSON array: {err}"))?;
        return Ok(values);
    }

    let mut out = Vec::new();
    let stream = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>();
    for value in stream {
        out.push(value.map_err(|err| format!("invalid JSON object stream: {err}"))?);
    }
    Ok(out)
}

fn import_single_item(
    storage: &Storage,
    index: &mut ExistingAccountIndex,
    item: &Value,
    sequence: usize,
) -> Result<bool, String> {
    let payload = extract_token_payload(&item)?;
    let claims = parse_id_token_claims(&payload.id_token).ok();
    let subject_account_id = claims
        .as_ref()
        .map(|c| c.sub.trim().to_string())
        .filter(|v| !v.is_empty());
    let logical_account_id = resolve_logical_account_id(&payload, subject_account_id.as_deref())?;

    let chatgpt_account_id = payload
        .account_id_hint
        .clone()
        .or_else(|| claims.as_ref().and_then(|c| c.auth.as_ref()?.chatgpt_account_id.clone()))
        .or_else(|| extract_chatgpt_account_id(&payload.id_token))
        .or_else(|| extract_chatgpt_account_id(&payload.access_token))
        .unwrap_or_else(|| logical_account_id.clone());

    let workspace_id = claims
        .as_ref()
        .and_then(|c| c.workspace_id.clone())
        .or_else(|| extract_workspace_id(&payload.id_token))
        .or_else(|| extract_workspace_id(&payload.access_token))
        .or_else(|| Some(chatgpt_account_id.clone()));

    let label = claims
        .as_ref()
        .and_then(|c| c.email.clone())
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            item.get("email")
                .and_then(Value::as_str)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| format!("导入账号{:04}", sequence));

    let now = now_ts();
    let existing_id = index.find_existing_account_id(
        &logical_account_id,
        subject_account_id.as_deref(),
        Some(chatgpt_account_id.as_str()),
    );
    let (account_id, account, created) = if let Some(existing_id) = existing_id {
        let existing = index
            .by_id
            .get(&existing_id)
            .cloned()
            .ok_or_else(|| format!("existing account not found in index: {existing_id}"))?;
        let updated = Account {
            id: existing.id.clone(),
            label: if existing.label.trim().is_empty() {
                label
            } else {
                existing.label.clone()
            },
            issuer: if existing.issuer.trim().is_empty() {
                DEFAULT_ISSUER.to_string()
            } else {
                existing.issuer.clone()
            },
            chatgpt_account_id: Some(chatgpt_account_id),
            workspace_id,
            group_name: existing
                .group_name
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some("import".to_string())),
            sort: existing.sort,
            status: "active".to_string(),
            created_at: existing.created_at,
            updated_at: now,
        };
        (existing_id, updated, false)
    } else {
        let next_sort = index.next_sort;
        index.next_sort += 1;
        let created = Account {
            id: logical_account_id.clone(),
            label,
            issuer: DEFAULT_ISSUER.to_string(),
            chatgpt_account_id: Some(chatgpt_account_id),
            workspace_id,
            group_name: Some("import".to_string()),
            sort: next_sort,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        (logical_account_id.clone(), created, true)
    };

    storage.insert_account(&account).map_err(|e| e.to_string())?;
    let token = Token {
        account_id: account_id.clone(),
        id_token: payload.id_token,
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        api_key_access_token: None,
        last_refresh: now,
    };
    storage.insert_token(&token).map_err(|e| e.to_string())?;
    index.upsert_index(&account);
    Ok(created)
}

fn extract_token_payload(item: &Value) -> Result<ImportTokenPayload, String> {
    let tokens = item
        .get("tokens")
        .ok_or_else(|| "missing field: tokens".to_string())?;
    let access_token = required_string(tokens, "access_token")?;
    let id_token = required_string(tokens, "id_token")?;
    let refresh_token = required_string(tokens, "refresh_token")?;
    let account_id_hint = optional_string(tokens, "account_id");
    Ok(ImportTokenPayload {
        access_token,
        id_token,
        refresh_token,
        account_id_hint,
    })
}

fn resolve_logical_account_id(
    payload: &ImportTokenPayload,
    subject_account_id: Option<&str>,
) -> Result<String, String> {
    let account_id_hint = payload.account_id_hint.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let hint_suffix = account_id_hint.and_then(|value| {
        value
            .split_once("::")
            .map(|(_, suffix)| suffix.trim())
            .filter(|suffix| !suffix.is_empty())
    });

    if let Some(sub) = subject_account_id.map(str::trim).filter(|v| !v.is_empty()) {
        return Ok(account_key(sub, hint_suffix));
    }

    if let Some(value) = account_id_hint {
        return Ok(value.to_string());
    }

    if let Some(value) = extract_chatgpt_account_id(&payload.id_token)
        .or_else(|| extract_chatgpt_account_id(&payload.access_token))
    {
        let normalized = value.trim().to_string();
        if !normalized.is_empty() {
            return Ok(normalized);
        }
    }

    Err("unable to resolve account id from tokens.account_id / id_token / access_token".to_string())
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    let raw = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing field: tokens.{key}"))?;
    let out = raw.trim();
    if out.is_empty() {
        return Err(format!("empty field: tokens.{key}"));
    }
    Ok(out.to_string())
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
