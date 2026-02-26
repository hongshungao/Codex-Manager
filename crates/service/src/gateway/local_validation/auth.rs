use codexmanager_core::storage::{ApiKey, Storage};

use crate::storage_helpers::{hash_platform_key, open_storage, StorageHandle};

pub(super) fn open_storage_or_error() -> Result<StorageHandle, super::LocalValidationError> {
    open_storage().ok_or_else(|| super::LocalValidationError::new(500, "storage unavailable"))
}

pub(super) fn load_active_api_key(
    storage: &Storage,
    platform_key: &str,
    request_url: &str,
    debug: bool,
) -> Result<ApiKey, super::LocalValidationError> {
    let key_hash = hash_platform_key(platform_key);
    let api_key = storage.find_api_key_by_hash(&key_hash).map_err(|err| {
        super::LocalValidationError::new(500, format!("storage read failed: {err}"))
    })?;

    let Some(api_key) = api_key else {
        if debug {
            log::warn!(
                "event=gateway_auth_invalid path={} status=403 key_hash_prefix={}",
                request_url,
                &key_hash[..8]
            );
        }
        return Err(super::LocalValidationError::new(403, "invalid api key"));
    };

    if api_key.status != "active" {
        if debug {
            log::warn!(
                "event=gateway_auth_disabled path={} status=403 key_id={}",
                request_url, api_key.id
            );
        }
        return Err(super::LocalValidationError::new(403, "api key disabled"));
    }

    Ok(api_key)
}
