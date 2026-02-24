use codexmanager_core::rpc::types::ApiKeySecretResult;

use crate::storage_helpers::open_storage;

pub(crate) fn read_api_key_secret(key_id: &str) -> Result<ApiKeySecretResult, String> {
    let normalized = key_id.trim();
    if normalized.is_empty() {
        return Err("missing key id".to_string());
    }
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let secret = storage
        .find_api_key_secret_by_id(normalized)
        .map_err(|err| format!("read api key secret failed: {err}"))?;
    let key = secret.ok_or_else(|| "api key secret not found".to_string())?;
    Ok(ApiKeySecretResult {
        id: normalized.to_string(),
        key,
    })
}
