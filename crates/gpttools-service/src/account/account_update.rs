use gpttools_core::storage::{now_ts, Event};

use crate::storage_helpers::open_storage;

pub(crate) fn update_account_sort(account_id: &str, sort: i64) -> Result<(), String> {
    // 更新账号排序并记录事件
    if account_id.is_empty() {
        return Err("missing accountId".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_account_sort(account_id, sort)
        .map_err(|e| e.to_string())?;
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "account_sort_update".to_string(),
        message: format!("sort={sort}"),
        created_at: now_ts(),
    });
    Ok(())
}
