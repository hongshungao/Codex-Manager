use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::CONTENT_TYPE;
use std::time::Instant;
use tiny_http::Request;

use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
use super::primary_attempt::{run_primary_upstream_attempt, PrimaryAttemptResult};

pub(super) enum PrimaryFlowDecision {
    Continue {
        upstream: reqwest::blocking::Response,
        auth_token: String,
    },
    RespondUpstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_primary_upstream_flow<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    base: &str,
    path: &str,
    primary_url: &str,
    request_deadline: Option<Instant>,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let auth_token = match super::super::resolve_openai_bearer_token(storage, account, token) {
        Ok(token) => token,
        Err(err) => {
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(primary_url), 502, Some(err.as_str()));
            if has_more_candidates {
                return PrimaryFlowDecision::Failover;
            }
            return PrimaryFlowDecision::Terminal {
                status_code: 502,
                message: format!("resolve upstream bearer token failed: {err}"),
            };
        }
    };
    if debug {
        eprintln!(
            "gateway upstream: base={}, token_source=openai_bearer",
            base,
        );
    }

    let upstream = match run_primary_upstream_attempt(
        client,
        method,
        primary_url,
        request_deadline,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_cookie,
        auth_token.as_str(),
        account,
        strip_session_affinity,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        PrimaryAttemptResult::Upstream(resp) => resp,
        PrimaryAttemptResult::Failover => return PrimaryFlowDecision::Failover,
        PrimaryAttemptResult::Terminal {
            status_code,
            message,
        } => {
            return PrimaryFlowDecision::Terminal {
                status_code,
                message,
            };
        }
    };

    let status = upstream.status();
    match handle_openai_fallback_branch(
        client,
        storage,
        method,
        request,
        incoming_headers,
        body,
        is_stream,
        base,
        path,
        upstream_fallback_base,
        account,
        token,
        upstream_cookie,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        status,
        upstream.headers().get(CONTENT_TYPE),
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        FallbackBranchResult::NotTriggered => PrimaryFlowDecision::Continue {
            upstream,
            auth_token,
        },
        FallbackBranchResult::RespondUpstream(resp) => PrimaryFlowDecision::RespondUpstream(resp),
        FallbackBranchResult::Failover => PrimaryFlowDecision::Failover,
        FallbackBranchResult::Terminal {
            status_code,
            message,
        } => PrimaryFlowDecision::Terminal {
            status_code,
            message,
        },
    }
}


