use crate::storage_helpers::open_storage;

pub(crate) fn clear_request_logs() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage.clear_request_logs().map_err(|e| e.to_string())
}
