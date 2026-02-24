use codexmanager_core::storage::{now_ts, Event, Storage};

pub(crate) fn set_account_status(
    storage: &Storage,
    account_id: &str,
    status: &str,
    reason: &str,
) {
    if matches!(
        storage.update_account_status_if_changed(account_id, status),
        Ok(true)
    ) {
        let _ = storage.insert_event(&Event {
            account_id: Some(account_id.to_string()),
            event_type: "account_status_update".to_string(),
            message: format!("status={status} reason={reason}"),
            created_at: now_ts(),
        });
    }
}
