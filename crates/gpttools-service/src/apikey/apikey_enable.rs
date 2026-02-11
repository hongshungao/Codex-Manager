use crate::storage_helpers::open_storage;

pub(crate) fn enable_api_key(key_id: &str) -> Result<(), String> {
    // 启用平台 Key，恢复网关鉴权可用。
    if key_id.is_empty() {
        return Err("missing id".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_api_key_status(key_id, "active")
        .map_err(|e| e.to_string())?;
    Ok(())
}
