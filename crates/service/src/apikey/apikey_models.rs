use codexmanager_core::rpc::types::ApiKeyModelListResult;
use codexmanager_core::rpc::types::ModelOption;
use codexmanager_core::storage::now_ts;

use crate::gateway;
use crate::storage_helpers;

const MODEL_CACHE_SCOPE_DEFAULT: &str = "default";

pub(crate) fn read_model_options(refresh_remote: bool) -> Result<ApiKeyModelListResult, String> {
    if !refresh_remote {
        let items = read_cached_model_options()?;
        return Ok(ApiKeyModelListResult { items });
    }

    match gateway::fetch_models_for_picker() {
        Ok(items) => {
            let _ = save_model_options_cache(&items);
            Ok(ApiKeyModelListResult { items })
        }
        Err(err) => {
            let cached = read_cached_model_options()?;
            if !cached.is_empty() {
                return Ok(ApiKeyModelListResult { items: cached });
            }
            Err(err)
        }
    }
}

fn save_model_options_cache(items: &[ModelOption]) -> Result<(), String> {
    let storage = storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let items_json = serde_json::to_string(items).map_err(|e| e.to_string())?;
    storage
        .upsert_model_options_cache(MODEL_CACHE_SCOPE_DEFAULT, &items_json, now_ts())
        .map_err(|e| e.to_string())
}

fn read_cached_model_options() -> Result<Vec<ModelOption>, String> {
    let storage = storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let Some(cache) = storage
        .get_model_options_cache(MODEL_CACHE_SCOPE_DEFAULT)
        .map_err(|e| e.to_string())?
    else {
        return Ok(Vec::new());
    };
    let items = serde_json::from_str::<Vec<ModelOption>>(&cache.items_json).unwrap_or_default();
    Ok(items)
}
