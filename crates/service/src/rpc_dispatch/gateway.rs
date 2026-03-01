use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "gateway/routeStrategy/get" => {
            let strategy = crate::gateway::current_route_strategy();
            super::as_json(serde_json::json!({
                "strategy": strategy,
                "options": ["ordered", "balanced"],
                "manualPreferredAccountId": crate::gateway::manual_preferred_account(),
            }))
        }
        "gateway/routeStrategy/set" => {
            let strategy = super::str_param(req, "strategy").unwrap_or("");
            super::value_or_error(crate::gateway::set_route_strategy(strategy).map(|applied| {
                serde_json::json!({
                    "strategy": applied
                })
            }))
        }
        "gateway/manualAccount/get" => super::as_json(serde_json::json!({
            "accountId": crate::gateway::manual_preferred_account()
        })),
        "gateway/manualAccount/set" => {
            let account_id = super::str_param(req, "accountId").unwrap_or("");
            super::ok_or_error(crate::gateway::set_manual_preferred_account(account_id))
        }
        "gateway/manualAccount/clear" => {
            crate::gateway::clear_manual_preferred_account();
            super::ok_result()
        }
        "gateway/headerPolicy/get" => super::as_json(serde_json::json!({
            "cpaNoCookieHeaderModeEnabled": crate::gateway::cpa_no_cookie_header_mode_enabled(),
            "envKey": "CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE",
        })),
        "gateway/headerPolicy/set" => {
            let enabled = super::bool_param(req, "cpaNoCookieHeaderModeEnabled")
                .or_else(|| super::bool_param(req, "enabled"))
                .unwrap_or(false);
            super::as_json(serde_json::json!({
                "cpaNoCookieHeaderModeEnabled": crate::gateway::set_cpa_no_cookie_header_mode(enabled),
            }))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
