use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;
use tiny_http::Request;

use super::openai_base::{handle_openai_base_attempt, OpenAiAttemptResult};
use super::postprocess::{process_upstream_post_retry_flow, PostRetryFlowDecision};
use super::primary_flow::{run_primary_upstream_flow, PrimaryFlowDecision};

pub(super) enum CandidateUpstreamDecision {
    RespondUpstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn process_candidate_upstream_flow<F>(
    storage: &Storage,
    method: &reqwest::Method,
    request: &Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    base: &str,
    path: &str,
    primary_url: &str,
    alt_url: Option<&str>,
    request_deadline: Option<Instant>,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    disable_challenge_stateless_retry: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> CandidateUpstreamDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let client = super::super::upstream_client_for_account(account.id.as_str());

    if super::deadline::is_expired(request_deadline) {
        return CandidateUpstreamDecision::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if super::super::is_openai_api_base(base) {
        match handle_openai_base_attempt(
            client,
            storage,
            method,
            path,
            request,
            incoming_headers,
            body,
            is_stream,
            base,
            account,
            token,
            upstream_cookie,
            strip_session_affinity,
            debug,
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            OpenAiAttemptResult::Upstream(resp) => {
                return CandidateUpstreamDecision::RespondUpstream(resp);
            }
            OpenAiAttemptResult::Failover => {
                return CandidateUpstreamDecision::Failover;
            }
            OpenAiAttemptResult::Terminal {
                status_code,
                message,
            } => {
                return CandidateUpstreamDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    let (upstream, auth_token) = match run_primary_upstream_flow(
        client,
        storage,
        method,
        request,
        incoming_headers,
        body,
        is_stream,
        base,
        path,
        primary_url,
        request_deadline,
        upstream_fallback_base,
        account,
        token,
        upstream_cookie,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        PrimaryFlowDecision::Continue {
            upstream,
            auth_token,
        } => (upstream, auth_token),
        PrimaryFlowDecision::RespondUpstream(resp) => {
            return CandidateUpstreamDecision::RespondUpstream(resp);
        }
        PrimaryFlowDecision::Failover => {
            return CandidateUpstreamDecision::Failover;
        }
        PrimaryFlowDecision::Terminal {
            status_code,
            message,
        } => {
            return CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            };
        }
    };

    match process_upstream_post_retry_flow(
        client,
        storage,
        method,
        primary_url,
        alt_url,
        request_deadline,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_cookie,
        auth_token.as_str(),
        account,
        strip_session_affinity,
        debug,
        disable_challenge_stateless_retry,
        has_more_candidates,
        upstream,
        &mut log_gateway_result,
    ) {
        PostRetryFlowDecision::Failover => CandidateUpstreamDecision::Failover,
        PostRetryFlowDecision::Terminal {
            status_code,
            message,
        } => CandidateUpstreamDecision::Terminal {
            status_code,
            message,
        },
        PostRetryFlowDecision::RespondUpstream(resp) => {
            CandidateUpstreamDecision::RespondUpstream(resp)
        }
    }
}


