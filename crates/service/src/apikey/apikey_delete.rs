use crate::storage_helpers::open_storage;

pub(crate) fn delete_api_key(key_id: &str) -> Result<(), String> {
    // 删除平台 Key
    if key_id.is_empty() {
        return Err("missing id".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage.delete_api_key(key_id).map_err(|e| e.to_string())?;
    Ok(())
}
