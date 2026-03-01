use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;
use tiny_http::Request;

use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
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
    upstream_base: &str,
    path: &str,
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
    token: &mut Token,
    upstream_fallback_base: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    disable_challenge_stateless_retry: bool,
    has_more_candidates: bool,
    mut upstream: reqwest::blocking::Response,
    mut log_gateway_result: F,
) -> PostRetryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let mut status = upstream.status();
    // 中文注释：CPA 无 cookie 兼容模式下尽量保持“单跳上游”语义，避免多次 retry 反而触发 challenge。
    let compact_no_cookie_mode =
        super::super::cpa_no_cookie_header_mode_enabled() && upstream_cookie.is_none();
    if !status.is_success() {
        log::warn!(
            "gateway upstream non-success: status={}, account_id={}",
            status,
            account.id
        );
    }

    if !compact_no_cookie_mode {
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
    }

    // 中文注释：主流程 fallback 只覆盖首跳响应，这里补齐“重试后仍 challenge/401/403/429”场景。
    match handle_openai_fallback_branch(
        client,
        storage,
        method,
        request,
        incoming_headers,
        body,
        is_stream,
        upstream_base,
        path,
        upstream_fallback_base,
        account,
        token,
        upstream_cookie,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        status,
        upstream.headers().get(reqwest::header::CONTENT_TYPE),
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        FallbackBranchResult::NotTriggered => {}
        FallbackBranchResult::RespondUpstream(resp) => {
            return PostRetryFlowDecision::RespondUpstream(resp);
        }
        FallbackBranchResult::Failover => {
            return PostRetryFlowDecision::Failover;
        }
        FallbackBranchResult::Terminal {
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



