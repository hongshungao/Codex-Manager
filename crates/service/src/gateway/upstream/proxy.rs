use crate::apikey_profile::{PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_AZURE_OPENAI};
use std::time::{Duration, Instant};
use tiny_http::{Request, Response};

use super::super::request_log::RequestLogUsage;
use super::super::local_validation::LocalValidationResult;
use super::candidate_flow::{process_candidate_upstream_flow, CandidateUpstreamDecision};
use super::execution_context::GatewayUpstreamExecutionContext;
use super::precheck::{prepare_candidates_for_proxy, CandidatePrecheckResult};

fn respond_terminal(request: Request, status_code: u16, message: String) -> Result<(), String> {
    let response = Response::from_string(message).with_status_code(status_code);
    let _ = request.respond(response);
    Ok(())
}

fn respond_total_timeout(
    request: Request,
    context: &GatewayUpstreamExecutionContext<'_>,
    started_at: Instant,
) -> Result<(), String> {
    let message = "upstream total timeout exceeded".to_string();
    context.log_final_result(
        None,
        None,
        504,
        RequestLogUsage::default(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    respond_terminal(request, 504, message)
}

pub(in super::super) fn proxy_validated_request(
    request: Request,
    validated: LocalValidationResult,
    debug: bool,
) -> Result<(), String> {
    let LocalValidationResult {
        trace_id,
        incoming_headers,
        storage,
        path,
        body,
        is_stream,
        has_prompt_cache_key,
        request_shape,
        protocol_type,
        upstream_base_url,
        static_headers_json,
        response_adapter,
        request_method,
        key_id,
        model_for_log,
        reasoning_for_log,
        method,
    } = validated;
    let started_at = Instant::now();
    let request_deadline =
        super::super::upstream_total_timeout().map(|timeout| started_at + timeout);

    super::super::trace_log::log_request_start(
        trace_id.as_str(),
        key_id.as_str(),
        request_method.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        is_stream,
        protocol_type.as_str(),
    );
    super::super::trace_log::log_request_body_preview(trace_id.as_str(), &body);

    if protocol_type == PROTOCOL_AZURE_OPENAI {
        return super::protocol::azure_openai::proxy_azure_request(
            request,
            &storage,
            trace_id.as_str(),
            key_id.as_str(),
            path.as_str(),
            request_method.as_str(),
            &method,
            &body,
            is_stream,
            response_adapter,
            model_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            upstream_base_url.as_deref(),
            static_headers_json.as_deref(),
            request_deadline,
            started_at,
        );
    }

    let (request, mut candidates) = match prepare_candidates_for_proxy(
        request,
        &storage,
        trace_id.as_str(),
        &key_id,
        &path,
        &request_method,
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
    ) {
        CandidatePrecheckResult::Ready { request, candidates } => (request, candidates),
        CandidatePrecheckResult::Responded => return Ok(()),
    };
    let mut request = Some(request);

    let upstream_base = super::super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::super::resolve_upstream_fallback_base_url(base);
    let (url, url_alt) = super::super::request_rewrite::compute_upstream_url(base, &path);

    let upstream_cookie = super::super::upstream_cookie();

    let candidate_count = candidates.len();
    let account_max_inflight = super::super::account_max_inflight_limit();
    let anthropic_has_prompt_cache_key = protocol_type == PROTOCOL_ANTHROPIC_NATIVE
        && has_prompt_cache_key;
    super::super::apply_route_strategy(&mut candidates, &key_id, model_for_log.as_deref());
    let candidate_order = candidates
        .iter()
        .map(|(account, _)| format!("{}#sort={}", account.id, account.sort))
        .collect::<Vec<_>>();
    super::super::trace_log::log_candidate_pool(
        trace_id.as_str(),
        key_id.as_str(),
        super::super::current_route_strategy(),
        candidate_order.as_slice(),
    );

    let context = GatewayUpstreamExecutionContext::new(
        &trace_id,
        &storage,
        &key_id,
        &path,
        &request_method,
        protocol_type.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        candidate_count,
        account_max_inflight,
    );
    let allow_openai_fallback = true;
    let disable_challenge_stateless_retry =
        !(protocol_type == PROTOCOL_ANTHROPIC_NATIVE && body.len() <= 2 * 1024);
    let request_gate_lock =
        super::super::request_gate_lock(&key_id, &path, model_for_log.as_deref());
    let request_gate_wait_timeout = super::super::request_gate_wait_timeout();
    super::super::trace_log::log_request_gate_wait(
        trace_id.as_str(),
        key_id.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
    );
    let gate_wait_started_at = Instant::now();
    let _request_gate_guard = match request_gate_lock.try_acquire() {
        Ok(Some(guard)) => {
            super::super::trace_log::log_request_gate_acquired(
                trace_id.as_str(),
                key_id.as_str(),
                path.as_str(),
                model_for_log.as_deref(),
                0,
            );
            Some(guard)
        }
        Ok(None) => {
            let effective_wait = super::deadline::cap_wait(request_gate_wait_timeout, request_deadline)
                .unwrap_or(Duration::from_millis(0));
            let wait_result = if effective_wait.is_zero() {
                Ok(None)
            } else {
                request_gate_lock.acquire_with_timeout(effective_wait)
            };
            if let Ok(Some(guard)) = wait_result {
                super::super::trace_log::log_request_gate_acquired(
                    trace_id.as_str(),
                    key_id.as_str(),
                    path.as_str(),
                    model_for_log.as_deref(),
                    gate_wait_started_at.elapsed().as_millis(),
                );
                Some(guard)
            } else {
                match wait_result {
                    Err(super::super::RequestGateAcquireError::Poisoned) => {
                        super::super::trace_log::log_request_gate_skip(
                            trace_id.as_str(),
                            "lock_poisoned",
                        );
                    }
                    _ => {
                        let reason = if super::deadline::is_expired(request_deadline) {
                            "total_timeout"
                        } else {
                            "gate_wait_timeout"
                        };
                        super::super::trace_log::log_request_gate_skip(trace_id.as_str(), reason);
                    }
                }
                None
            }
        }
        Err(super::super::RequestGateAcquireError::Poisoned) => {
            super::super::trace_log::log_request_gate_skip(trace_id.as_str(), "lock_poisoned");
            None
        }
    };
    let has_sticky_fallback_session =
        super::header_profile::derive_sticky_session_id_from_headers(&incoming_headers).is_some();
    let has_sticky_fallback_conversation =
        super::header_profile::derive_sticky_conversation_id_from_headers(&incoming_headers)
            .is_some();

    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        if super::deadline::is_expired(request_deadline) {
            let request = request
                .take()
                .expect("request should be available before timeout response");
            return respond_total_timeout(request, &context, started_at);
        }
        // 中文注释：Claude 兼容入口命中 prompt_cache_key 时，优先保持会话粘性；
        // failover 时若强制重置 Session/Conversation，更容易触发 upstream challenge。
        let strip_session_affinity = if anthropic_has_prompt_cache_key {
            false
        } else {
            idx > 0
        };
        context.log_candidate_start(&account.id, idx, strip_session_affinity);
        if let Some(skip_reason) = context.should_skip_candidate(&account.id, idx) {
            context.log_candidate_skip(&account.id, idx, skip_reason);
            let _ = super::super::clear_manual_preferred_account_if(&account.id);
            continue;
        }

        let request_ref = request
            .as_ref()
            .ok_or_else(|| "request already consumed".to_string())?;
        let incoming_session_id = incoming_headers.session_id();
        let incoming_turn_state = incoming_headers.turn_state();
        let incoming_conversation_id = incoming_headers.conversation_id();
        super::super::trace_log::log_attempt_profile(
            trace_id.as_str(),
            &account.id,
            idx,
            candidate_count,
            strip_session_affinity,
            incoming_session_id.is_some() || has_sticky_fallback_session,
            incoming_turn_state.is_some(),
            incoming_conversation_id.is_some() || has_sticky_fallback_conversation,
            None,
            request_shape.as_deref(),
            body.len(),
            model_for_log.as_deref(),
        );
        // 中文注释：把 inflight 计数覆盖到整个响应生命周期，确保下一批请求能看到真实负载。
        let mut inflight_guard = Some(super::super::acquire_account_inflight(&account.id));
        let mut last_attempt_url: Option<String> = None;
        let mut last_attempt_error: Option<String> = None;

        let decision = process_candidate_upstream_flow(
            &storage,
            &method,
            request_ref,
            &incoming_headers,
            &body,
            is_stream,
            base,
            &path,
            url.as_str(),
            url_alt.as_deref(),
            request_deadline,
            upstream_fallback_base.as_deref(),
            &account,
            &mut token,
            upstream_cookie.as_deref(),
            strip_session_affinity,
            debug,
            allow_openai_fallback,
            disable_challenge_stateless_retry,
            context.has_more_candidates(idx),
            |upstream_url, status_code, error| {
                last_attempt_url = upstream_url.map(str::to_string);
                last_attempt_error = error.map(str::to_string);
                super::super::record_route_quality(&account.id, status_code);
                context.log_attempt_result(&account.id, upstream_url, status_code, error);
            },
        );

        match decision {
            CandidateUpstreamDecision::Failover => {
                let _ = super::super::clear_manual_preferred_account_if(&account.id);
                super::super::record_gateway_failover_attempt();
                continue;
            }
            CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            } => {
                let _ = super::super::clear_manual_preferred_account_if(&account.id);
                let elapsed_ms = started_at.elapsed().as_millis();
                context.log_final_result(
                    Some(&account.id),
                    last_attempt_url.as_deref(),
                    status_code,
                    RequestLogUsage::default(),
                    Some(message.as_str()),
                    elapsed_ms,
                );
                let request = request
                    .take()
                    .expect("request should be available before terminal response");
                return respond_terminal(request, status_code, message);
            }
            CandidateUpstreamDecision::RespondUpstream(resp) => {
                let status_code = resp.status().as_u16();
                if status_code >= 400 {
                    let _ = super::super::clear_manual_preferred_account_if(&account.id);
                }
                let final_error = if status_code >= 400 {
                    last_attempt_error.as_deref()
                } else {
                    None
                };
                let elapsed_ms = started_at.elapsed().as_millis();
                let request = request
                    .take()
                    .expect("request should be available before terminal response");
                let guard = inflight_guard
                    .take()
                    .expect("inflight guard should be available before terminal response");
                let usage = super::super::respond_with_upstream(
                    request,
                    resp,
                    guard,
                    response_adapter,
                    is_stream,
                )?;
                context.log_final_result(
                    Some(&account.id),
                    last_attempt_url.as_deref(),
                    status_code,
                    RequestLogUsage {
                        input_tokens: usage.input_tokens,
                        cached_input_tokens: usage.cached_input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.total_tokens,
                        reasoning_output_tokens: usage.reasoning_output_tokens,
                    },
                    final_error,
                    elapsed_ms,
                );
                return Ok(());
            }
        }
    }

    context.log_final_result(
        None,
        Some(base),
        503,
        RequestLogUsage::default(),
        Some("no available account"),
        started_at.elapsed().as_millis(),
    );
    let request = request
        .take()
        .ok_or_else(|| "request already consumed".to_string())?;
    respond_terminal(request, 503, "no available account".to_string())
}



