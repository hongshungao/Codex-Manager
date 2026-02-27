use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::blocking::Client;
use reqwest::Method;
use tiny_http::Request;

pub(super) fn try_openai_fallback(
    client: &Client,
    storage: &Storage,
    method: &Method,
    request_path: &str,
    _request: &Request,
    incoming_headers: &super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let (url, _url_alt) = super::compute_upstream_url(upstream_base, request_path);
    let bearer = super::resolve_openai_bearer_token(storage, account, token)?;

    let account_id = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref());
    let header_input = super::upstream::header_profile::CodexUpstreamHeaderInput {
        auth_token: bearer.as_str(),
        account_id,
        upstream_cookie,
        incoming_session_id: incoming_headers.session_id(),
        fallback_session_id: None,
        incoming_turn_state: incoming_headers.turn_state(),
        incoming_conversation_id: incoming_headers.conversation_id(),
        fallback_conversation_id: None,
        strip_session_affinity,
        is_stream,
        has_body: !body.is_empty(),
    };
    let upstream_headers =
        super::upstream::header_profile::build_codex_upstream_headers(header_input);
    if debug {
        log::debug!(
            "event=gateway_upstream_token_source path={} account_id={} token_source=api_key_access_token upstream_base={}",
            request_path,
            account_id.unwrap_or("-"),
            upstream_base
        );
    }
    let build_request = |http: &Client| {
        let mut builder = http.request(method.clone(), &url);
        for (name, value) in upstream_headers.iter() {
            builder = builder.header(name, value);
        }
        if !body.is_empty() {
            builder = builder.body(body.to_vec());
        }
        builder
    };
    let resp = match build_request(client).send() {
        Ok(resp) => resp,
        Err(first_err) => {
            let fresh = super::fresh_upstream_client_for_account(account.id.as_str());
            match build_request(&fresh).send() {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_fresh_client: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    Ok(Some(resp))
}
