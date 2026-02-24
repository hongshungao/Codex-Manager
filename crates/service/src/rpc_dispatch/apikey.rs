use codexmanager_core::rpc::types::{ApiKeyListResult, JsonRpcRequest, JsonRpcResponse};

use crate::{
    apikey_create, apikey_delete, apikey_disable, apikey_enable, apikey_list, apikey_models,
    apikey_read_secret, apikey_update_model,
};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "apikey/list" => super::value_or_error(
            apikey_list::read_api_keys().map(|items| ApiKeyListResult { items }),
        ),
        "apikey/create" => {
            let name = super::string_param(req, "name");
            let model_slug = super::string_param(req, "modelSlug");
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let protocol_type = super::string_param(req, "protocolType");
            super::value_or_error(apikey_create::create_api_key(
                name,
                model_slug,
                reasoning_effort,
                protocol_type,
            ))
        }
        "apikey/readSecret" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::value_or_error(apikey_read_secret::read_api_key_secret(key_id))
        }
        "apikey/models" => super::value_or_error(apikey_models::read_model_options()),
        "apikey/updateModel" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            let model_slug = super::string_param(req, "modelSlug");
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let protocol_type = super::string_param(req, "protocolType");
            super::ok_or_error(apikey_update_model::update_api_key_model(
                key_id,
                model_slug,
                reasoning_effort,
                protocol_type,
            ))
        }
        "apikey/delete" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_delete::delete_api_key(key_id))
        }
        "apikey/disable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_disable::disable_api_key(key_id))
        }
        "apikey/enable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(apikey_enable::enable_api_key(key_id))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
