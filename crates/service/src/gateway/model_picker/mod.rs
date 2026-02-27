use codexmanager_core::rpc::types::ModelOption;
use reqwest::Method;

mod parse;
mod request;

use parse::parse_model_options;
use request::send_models_request;

pub(crate) fn fetch_models_for_picker() -> Result<Vec<ModelOption>, String> {
    let storage = super::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut candidates = super::collect_gateway_candidates(&storage)?;
    if candidates.is_empty() {
        return Err("no available account".to_string());
    }

    let upstream_base = super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::resolve_upstream_fallback_base_url(base);
    let path = super::normalize_models_path("/v1/models");
    let method = Method::GET;
    let upstream_cookie = super::upstream_cookie();
    candidates.sort_by_key(|(account, _)| {
        (
            super::is_account_in_cooldown(&account.id),
            super::account_inflight_count(&account.id),
        )
    });
    let mut last_error = "models request failed".to_string();
    for (account, mut token) in candidates {
        let client = super::upstream_client_for_account(account.id.as_str());
        match send_models_request(
            client,
            &storage,
            &method,
            &upstream_base,
            &path,
            &account,
            &mut token,
            upstream_cookie.as_deref(),
        ) {
            Ok(response_body) => return Ok(parse_model_options(&response_body)),
            Err(err) => {
                // ChatGPT upstream occasionally returns HTML challenge. Try OpenAI fallback.
                if err.contains("text/html") || err.contains("cloudflare") {
                    if let Some(fallback_base) = upstream_fallback_base.as_deref() {
                        if let Ok(response_body) = send_models_request(
                            client,
                            &storage,
                            &method,
                            fallback_base,
                            &path,
                            &account,
                            &mut token,
                            upstream_cookie.as_deref(),
                        ) {
                            return Ok(parse_model_options(&response_body));
                        }
                    }
                }
                last_error = err;
            }
        }
    }

    Err(last_error)
}
