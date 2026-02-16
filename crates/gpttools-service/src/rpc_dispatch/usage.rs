use gpttools_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, UsageListResult, UsageReadResult};

use crate::{usage_list, usage_read, usage_refresh};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/usage/read" => {
            let account_id = super::str_param(req, "accountId");
            super::as_json(UsageReadResult {
                snapshot: usage_read::read_usage_snapshot(account_id),
            })
        }
        "account/usage/list" => super::as_json(UsageListResult {
            items: usage_list::read_usage_snapshots(),
        }),
        "account/usage/refresh" => {
            let account_id = super::str_param(req, "accountId");
            let result = match account_id {
                Some(account_id) => usage_refresh::refresh_usage_for_account(account_id),
                None => usage_refresh::refresh_usage_for_all_accounts(),
            };
            super::ok_or_error(result)
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
