use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, RequestLogListResult};

use crate::{requestlog_clear, requestlog_list, requestlog_today_summary};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "requestlog/list" => {
            let query = super::string_param(req, "query");
            let limit = super::i64_param(req, "limit");
            super::value_or_error(
                requestlog_list::read_request_logs(query, limit)
                    .map(|items| RequestLogListResult { items }),
            )
        }
        "requestlog/clear" => super::ok_or_error(requestlog_clear::clear_request_logs()),
        "requestlog/today_summary" => {
            super::value_or_error(requestlog_today_summary::read_requestlog_today_summary())
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
