use codexmanager_core::auth::parse_id_token_claims;
use codexmanager_core::storage::{now_ts, Event, UsageSnapshotRecord};
use serde_json::Value;
use serde::Serialize;
use std::collections::HashMap;

use crate::account_availability::{evaluate_snapshot, Availability};
use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteUnavailableFreeResult {
    scanned: usize,
    deleted: usize,
    skipped_available: usize,
    skipped_non_free: usize,
    skipped_missing_usage: usize,
    skipped_missing_token: usize,
    deleted_account_ids: Vec<String>,
}

pub(crate) fn delete_unavailable_free_accounts() -> Result<DeleteUnavailableFreeResult, String> {
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let accounts = storage.list_accounts().map_err(|err| err.to_string())?;
    let usage_by_account: HashMap<String, UsageSnapshotRecord> = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|snapshot| (snapshot.account_id.clone(), snapshot))
        .collect();

    let mut result = DeleteUnavailableFreeResult {
        scanned: 0,
        deleted: 0,
        skipped_available: 0,
        skipped_non_free: 0,
        skipped_missing_usage: 0,
        skipped_missing_token: 0,
        deleted_account_ids: Vec::new(),
    };

    for account in accounts {
        result.scanned += 1;

        let Some(snapshot) = usage_by_account.get(&account.id) else {
            result.skipped_missing_usage += 1;
            continue;
        };
        if matches!(evaluate_snapshot(snapshot), Availability::Available) {
            result.skipped_available += 1;
            continue;
        }

        let token = storage
            .find_token_by_account_id(&account.id)
            .map_err(|err| err.to_string())?;
        let Some(token) = token else {
            result.skipped_missing_token += 1;
            continue;
        };

        let plan_type = extract_plan_type_from_id_token(&token.id_token);
        if !is_free_plan_type(plan_type.as_deref())
            && !is_free_plan_from_credits_json(snapshot.credits_json.as_deref())
        {
            result.skipped_non_free += 1;
            continue;
        }

        storage
            .delete_account(&account.id)
            .map_err(|err| err.to_string())?;

        let event_message = match plan_type.as_deref() {
            Some(plan) => format!("bulk delete unavailable free account: plan={plan}"),
            None => "bulk delete unavailable free account".to_string(),
        };
        let _ = storage.insert_event(&Event {
            account_id: Some(account.id.clone()),
            event_type: "account_bulk_delete_unavailable_free".to_string(),
            message: event_message,
            created_at: now_ts(),
        });

        result.deleted += 1;
        result.deleted_account_ids.push(account.id);
    }

    Ok(result)
}

fn extract_plan_type_from_id_token(id_token: &str) -> Option<String> {
    parse_id_token_claims(id_token)
        .ok()
        .and_then(|claims| claims.auth)
        .and_then(|auth| auth.chatgpt_plan_type)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn is_free_plan_type(plan_type: Option<&str>) -> bool {
    let Some(plan_type) = plan_type else {
        return false;
    };
    let normalized = plan_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("free")
}

fn is_free_plan_from_credits_json(raw_credits_json: Option<&str>) -> bool {
    let Some(raw_credits_json) = raw_credits_json else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_credits_json) else {
        return false;
    };
    let keys = [
        "plan_type",
        "planType",
        "subscription_tier",
        "subscriptionTier",
        "tier",
        "account_type",
        "accountType",
        "type",
    ];
    let extracted = extract_string_by_keys_recursive(&value, &keys);
    is_free_plan_type(extracted.as_deref())
}

fn extract_string_by_keys_recursive(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(object) = value.as_object() {
        for key in keys {
            let candidate = object
                .get(*key)
                .and_then(Value::as_str)
                .map(|text| text.trim().to_ascii_lowercase())
                .filter(|text| !text.is_empty());
            if candidate.is_some() {
                return candidate;
            }
        }
        for child in object.values() {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
        return None;
    }
    if let Some(array) = value.as_array() {
        for child in array {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{is_free_plan_from_credits_json, is_free_plan_type};

    #[test]
    fn free_plan_detection_accepts_common_variants() {
        assert!(is_free_plan_type(Some("free")));
        assert!(is_free_plan_type(Some("ChatGPT_Free")));
        assert!(is_free_plan_type(Some("free_tier")));
    }

    #[test]
    fn free_plan_detection_rejects_paid_or_unknown_variants() {
        assert!(!is_free_plan_type(None));
        assert!(!is_free_plan_type(Some("")));
        assert!(!is_free_plan_type(Some("plus")));
        assert!(!is_free_plan_type(Some("pro")));
        assert!(!is_free_plan_type(Some("team")));
    }

    #[test]
    fn free_plan_detection_accepts_credits_json_marker() {
        let credits_json = r#"{"planType":"free"}"#;
        assert!(is_free_plan_from_credits_json(Some(credits_json)));
    }
}
