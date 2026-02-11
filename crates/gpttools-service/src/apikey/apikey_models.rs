use gpttools_core::rpc::types::ApiKeyModelListResult;

use crate::gateway;

pub(crate) fn read_model_options() -> Result<ApiKeyModelListResult, String> {
    let items = gateway::fetch_models_for_picker()?;
    Ok(ApiKeyModelListResult { items })
}
