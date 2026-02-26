use codexmanager_core::rpc::types::ApiKeyCreateResult;
use codexmanager_core::storage::{now_ts, ApiKey};

use crate::apikey_profile::{
    normalize_protocol_type, normalize_static_headers_json, normalize_upstream_base_url,
    profile_from_protocol,
};
use crate::reasoning_effort::normalize_reasoning_effort_owned;
use crate::storage_helpers::{generate_key_id, generate_platform_key, hash_platform_key, open_storage};

pub(crate) fn create_api_key(
    name: Option<String>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
) -> Result<ApiKeyCreateResult, String> {
    // 创建平台 Key 并写入存储
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let key = generate_platform_key();
    let key_hash = hash_platform_key(&key);
    let key_id = generate_key_id();
    let protocol_type = normalize_protocol_type(protocol_type)?;
    let (client_type, protocol_type, auth_scheme) = profile_from_protocol(&protocol_type)?;
    let upstream_base_url = normalize_upstream_base_url(upstream_base_url)?;
    let static_headers_json = normalize_static_headers_json(static_headers_json)?;
    let record = ApiKey {
        id: key_id.clone(),
        name,
        model_slug,
        reasoning_effort: normalize_reasoning_effort_owned(reasoning_effort),
        client_type,
        protocol_type,
        auth_scheme,
        upstream_base_url,
        static_headers_json,
        key_hash,
        status: "active".to_string(),
        created_at: now_ts(),
        last_used_at: None,
    };
    storage.insert_api_key(&record).map_err(|e| e.to_string())?;
    if let Err(err) = storage.upsert_api_key_secret(&key_id, &key) {
        let _ = storage.delete_api_key(&key_id);
        return Err(format!("persist api key secret failed: {err}"));
    }
    Ok(ApiKeyCreateResult { id: key_id, key })
}
