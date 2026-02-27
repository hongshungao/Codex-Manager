use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage};
use std::time::Instant;
use tiny_http::Request;

use super::outcome::{decide_upstream_outcome, UpstreamOutcomeDecision};
use super::retry::{retry_with_alternate_path, AltPathRetryResult};
use super::stateless_retry::{
    retry_stateless_then_optional_alt, StatelessRetryResult,
};

pub(super) enum PostRetryFlowDecision {
    Failover,
    Terminal { status_code: u16, message: String },
    RespondUpstream(reqwest::blocking::Response),
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_upstream_post_retry_flow<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    url: &str,
    url_alt: Option<&str>,
    request_deadline: Option<Instant>,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    debug: bool,
    disable_challenge_stateless_retry: bool,
    has_more_candidates: bool,
    mut upstream: reqwest::blocking::Response,
    mut log_gateway_result: F,
) -> PostRetryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let mut status = upstream.status();
    if !status.is_success() {
        log::warn!(
            "gateway upstream non-success: status={}, account_id={}",
            status,
            account.id
        );
    }

    if let Some(alt_url) = url_alt {
        match retry_with_alternate_path(
            client,
            method,
            Some(alt_url),
            request_deadline,
            request,
            incoming_headers,
            body,
            is_stream,
            upstream_cookie,
            auth_token,
            account,
            strip_session_affinity,
            status,
            debug,
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            AltPathRetryResult::NotTriggered => {}
            AltPathRetryResult::Upstream(resp) => {
                upstream = resp;
                status = upstream.status();
            }
            AltPathRetryResult::Failover => {
                return PostRetryFlowDecision::Failover;
            }
            AltPathRetryResult::Terminal {
                status_code,
                message,
            } => {
                return PostRetryFlowDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    match retry_stateless_then_optional_alt(
        client,
        method,
        url,
        url_alt,
        request_deadline,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_cookie,
        auth_token,
        account,
        strip_session_affinity,
        status,
        debug,
        disable_challenge_stateless_retry,
    ) {
        StatelessRetryResult::NotTriggered => {}
        StatelessRetryResult::Upstream(resp) => {
            upstream = resp;
            status = upstream.status();
        }
        StatelessRetryResult::Terminal {
            status_code,
            message,
        } => {
            return PostRetryFlowDecision::Terminal {
                status_code,
                message,
            };
        }
    }

    match decide_upstream_outcome(
        storage,
        &account.id,
        status,
        upstream.headers().get(reqwest::header::CONTENT_TYPE),
        url,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        UpstreamOutcomeDecision::Failover => PostRetryFlowDecision::Failover,
        UpstreamOutcomeDecision::Terminal {
            status_code,
            message,
        } => PostRetryFlowDecision::Terminal {
            status_code,
            message,
        },
        UpstreamOutcomeDecision::RespondUpstream => PostRetryFlowDecision::RespondUpstream(upstream),
    }
}



