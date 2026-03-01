use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;

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
        "gateway/backgroundTasks/get" => super::as_json(crate::usage_refresh::background_tasks_settings()),
        "gateway/backgroundTasks/set" => {
            let patch = crate::usage_refresh::BackgroundTasksSettingsPatch {
                usage_polling_enabled: super::bool_param(req, "usagePollingEnabled")
                    .or_else(|| super::bool_param(req, "usagePolling")),
                usage_poll_interval_secs: u64_param(req, "usagePollIntervalSecs"),
                gateway_keepalive_enabled: super::bool_param(req, "gatewayKeepaliveEnabled")
                    .or_else(|| super::bool_param(req, "gatewayKeepalive")),
                gateway_keepalive_interval_secs: u64_param(req, "gatewayKeepaliveIntervalSecs"),
                token_refresh_polling_enabled: super::bool_param(req, "tokenRefreshPollingEnabled")
                    .or_else(|| super::bool_param(req, "tokenRefreshPolling")),
                token_refresh_poll_interval_secs: u64_param(req, "tokenRefreshPollIntervalSecs"),
                usage_refresh_workers: usize_param(req, "usageRefreshWorkers"),
                http_worker_factor: usize_param(req, "httpWorkerFactor"),
                http_worker_min: usize_param(req, "httpWorkerMin"),
                http_stream_worker_factor: usize_param(req, "httpStreamWorkerFactor"),
                http_stream_worker_min: usize_param(req, "httpStreamWorkerMin"),
            };
            super::as_json(crate::usage_refresh::set_background_tasks_settings(patch))
        }
        _ => return None,
    };

    Some(super::response(req, result))
}

fn u64_param(req: &JsonRpcRequest, key: &str) -> Option<u64> {
    let value = req.params.as_ref()?.get(key)?;
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn usize_param(req: &JsonRpcRequest, key: &str) -> Option<usize> {
    u64_param(req, key).and_then(|value| usize::try_from(value).ok())
}
