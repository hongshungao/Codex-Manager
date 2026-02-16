use gpttools_core::rpc::types::{AccountListResult, JsonRpcRequest, JsonRpcResponse};

use crate::{account_delete, account_list, account_update, auth_login, auth_tokens};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/list" => {
            let items = account_list::read_accounts();
            super::as_json(AccountListResult { items })
        }
        "account/delete" => {
            let account_id = super::str_param(req, "accountId").unwrap_or("");
            super::ok_or_error(account_delete::delete_account(account_id))
        }
        "account/update" => {
            let account_id = super::str_param(req, "accountId").unwrap_or("");
            let sort = super::i64_param(req, "sort").unwrap_or(0);
            super::ok_or_error(account_update::update_account_sort(account_id, sort))
        }
        "account/login/start" => {
            let login_type = super::str_param(req, "type").unwrap_or("chatgpt");
            let open_browser = super::bool_param(req, "openBrowser").unwrap_or(true);
            let note = super::string_param(req, "note");
            let tags = super::string_param(req, "tags");
            let group_name = super::string_param(req, "groupName");
            let workspace_id = super::string_param(req, "workspaceId")
                .and_then(|v| if v.trim().is_empty() { None } else { Some(v) });
            super::value_or_error(auth_login::login_start(
                login_type,
                open_browser,
                note,
                tags,
                group_name,
                workspace_id,
            ))
        }
        "account/login/status" => {
            let login_id = super::str_param(req, "loginId").unwrap_or("");
            super::as_json(auth_login::login_status(login_id))
        }
        "account/login/complete" => {
            let state = super::str_param(req, "state").unwrap_or("");
            let code = super::str_param(req, "code").unwrap_or("");
            let redirect_uri = super::str_param(req, "redirectUri");
            if state.is_empty() || code.is_empty() {
                serde_json::json!({"ok": false, "error": "missing code/state"})
            } else {
                super::ok_or_error(auth_tokens::complete_login_with_redirect(
                    state,
                    code,
                    redirect_uri,
                ))
            }
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
