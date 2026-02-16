use crate::apikey_profile::{normalize_protocol_type, profile_from_protocol};
use crate::storage_helpers::open_storage;
use crate::reasoning_effort::normalize_reasoning_effort;

pub(crate) fn update_api_key_model(
    key_id: &str,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    protocol_type: Option<String>,
) -> Result<(), String> {
    if key_id.is_empty() {
        return Err("key id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let normalized = model_slug
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .as_deref()
        .and_then(normalize_reasoning_effort);
    storage
        .update_api_key_model_config(key_id, normalized, normalized_reasoning)
        .map_err(|e| e.to_string())?;

    if let Some(protocol) = protocol_type {
        let current = storage
            .find_api_key_by_id(key_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "api key not found".to_string())?;
        let normalized_protocol = normalize_protocol_type(Some(protocol))?;
        let (next_client, next_protocol, next_auth) = profile_from_protocol(&normalized_protocol)?;
        storage
            .update_api_key_profile_config(
                key_id,
                &next_client,
                &next_protocol,
                &next_auth,
                current.upstream_base_url.as_deref(),
                current.static_headers_json.as_deref(),
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
