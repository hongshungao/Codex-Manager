use codexmanager_core::storage::Account;
use std::time::Instant;

pub(super) enum PrimaryAttemptResult {
    Upstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_primary_upstream_attempt<F>(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    url: &str,
    request_deadline: Option<Instant>,
    request: &tiny_http::Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryAttemptResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    if super::deadline::is_expired(request_deadline) {
        log_gateway_result(Some(url), 504, Some("upstream total timeout exceeded"));
        return PrimaryAttemptResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    match super::transport::send_upstream_request(
        client,
        method,
        url,
        request_deadline,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_cookie,
        auth_token,
        account,
        strip_session_affinity,
    ) {
        Ok(resp) => PrimaryAttemptResult::Upstream(resp),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(url), 502, Some(err_msg.as_str()));
            // 中文注释：主链路首次请求失败不代表所有候选都失败，
            // 先 failover 才能避免单账号抖动放大成全局不可用。
            if has_more_candidates {
                PrimaryAttemptResult::Failover
            } else {
                PrimaryAttemptResult::Terminal {
                    status_code: 502,
                    message: format!("upstream error: {err}"),
                }
            }
        }
    }
}


