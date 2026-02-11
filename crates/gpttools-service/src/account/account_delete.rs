use gpttools_core::storage::{now_ts, Event};

use crate::storage_helpers::open_storage;

pub(crate) fn delete_account(account_id: &str) -> Result<(), String> {
    // 删除账号并记录事件
    if account_id.is_empty() {
        return Err("missing accountId".to_string());
    }
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_account(account_id)
        .map_err(|e| e.to_string())?;
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "account_delete".to_string(),
        message: "account deleted".to_string(),
        created_at: now_ts(),
    });
    Ok(())
}
