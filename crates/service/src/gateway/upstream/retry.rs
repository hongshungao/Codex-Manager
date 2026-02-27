use bytes::Bytes;
use codexmanager_core::storage::Account;
use reqwest::StatusCode;
use std::time::{Duration, Instant};
use tiny_http::Request;

use super::transport::send_upstream_request;

pub(super) enum AltPathRetryResult {
    NotTriggered,
    Upstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_with_alternate_path<F>(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    alt_url: Option<&str>,
    request_deadline: Option<Instant>,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    status: StatusCode,
    debug: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> AltPathRetryResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let Some(alt_url) = alt_url else {
        return AltPathRetryResult::NotTriggered;
    };
    if !matches!(status.as_u16(), 400 | 404) {
        return AltPathRetryResult::NotTriggered;
    }
    if debug {
        log::warn!(
            "event=gateway_upstream_alt_retry path={} status={} account_id={} upstream_url={}",
            request.url(),
            status.as_u16(),
            account.id,
            alt_url
        );
    }
    if super::deadline::is_expired(request_deadline) {
        return AltPathRetryResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if !super::backoff::sleep_with_exponential_jitter(
        Duration::from_millis(40),
        Duration::from_millis(200),
        0,
        request_deadline,
    ) {
        return AltPathRetryResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    match send_upstream_request(
        client,
        method,
        alt_url,
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
        Ok(response) => AltPathRetryResult::Upstream(response),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(alt_url), 502, Some(err_msg.as_str()));
            // 中文注释：alt 路径失败时若还有候选账号必须优先切换，
            // 不这样做会把单账号路径差异放大成整次请求失败。
            if has_more_candidates {
                AltPathRetryResult::Failover
            } else {
                AltPathRetryResult::Terminal {
                    status_code: 502,
                    message: format!("upstream error: {err}"),
                }
            }
        }
    }
}


