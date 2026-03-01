use crate::apikey_profile::{PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_AZURE_OPENAI};
use serde_json::Value;
use std::time::{Duration, Instant};
use tiny_http::{Request, Response};

use super::super::request_log::RequestLogUsage;
use super::super::local_validation::LocalValidationResult;
use super::candidate_flow::{process_candidate_upstream_flow, CandidateUpstreamDecision};
use super::execution_context::GatewayUpstreamExecutionContext;
use super::precheck::{prepare_candidates_for_proxy, CandidatePrecheckResult};

fn body_has_encrypted_content_hint(body: &[u8]) -> bool {
    // Fast path: avoid JSON parsing unless we hit a recovery path.
    std::str::from_utf8(body)
        .ok()
        .is_some_and(|text| text.contains("\"encrypted_content\""))
}

fn strip_encrypted_content_value(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = map.remove("encrypted_content").is_some();
            for v in map.values_mut() {
                if strip_encrypted_content_value(v) {
                    changed = true;
                }
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                if strip_encrypted_content_value(item) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

fn strip_encrypted_content_from_body(body: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    if !strip_encrypted_content_value(&mut value) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

fn respond_terminal(request: Request, status_code: u16, message: String) -> Result<(), String> {
    let response = Response::from_string(message).with_status_code(status_code);
    let _ = request.respond(response);
    Ok(())
}

fn is_client_disconnect_error(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("broken pipe")
        || normalized.contains("connection reset")
        || normalized.contains("connection aborted")
        || normalized.contains("connection was forcibly closed")
        || normalized.contains("os error 32")
        || normalized.contains("os error 54")
        || normalized.contains("os error 104")
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
    let client_is_stream = is_stream;
    // 中文注释：对齐 CPA：/v1/responses 上游固定走 SSE。
    // 下游是否流式仍由客户端 `stream` 参数决定（在 response bridge 层聚合/透传）。
    let upstream_is_stream = client_is_stream || path.starts_with("/v1/responses");
    let request_deadline = super::deadline::request_deadline(started_at, client_is_stream);

    super::super::trace_log::log_request_start(
        trace_id.as_str(),
        key_id.as_str(),
        request_method.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        client_is_stream,
        protocol_type.as_str(),
    );
    super::super::trace_log::log_request_body_preview(trace_id.as_str(), body.as_ref());

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
            upstream_is_stream,
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
        !(protocol_type == PROTOCOL_ANTHROPIC_NATIVE && body.len() <= 2 * 1024)
            && !path.starts_with("/v1/responses");
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
    let has_body_encrypted_content = body_has_encrypted_content_hint(body.as_ref());
    let mut stripped_body: Option<bytes::Bytes> = None;

    // For `anthropic_native` with `prompt_cache_key`, keep Session/Conversation affinity within the
    // same Chatgpt-Account-Id "scope" (chatgpt_account_id preferred, otherwise workspace_id).
    // Switching scope on failover can increase upstream challenge probability.
    let mut first_candidate_account_scope: Option<String> = None;
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
            let candidate_scope = account
                .chatgpt_account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .or_else(|| {
                    account
                        .workspace_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|value| value.to_string())
                });
            if idx == 0 {
                first_candidate_account_scope = candidate_scope.clone();
                false
            } else {
                candidate_scope != first_candidate_account_scope
            }
        } else {
            idx > 0
        };

        let body_for_attempt = if strip_session_affinity && has_body_encrypted_content {
            if stripped_body.is_none() {
                stripped_body = strip_encrypted_content_from_body(body.as_ref())
                    .map(bytes::Bytes::from)
                    .or_else(|| Some(body.clone()));
            }
            stripped_body
                .as_ref()
                .expect("stripped body should be initialized")
        } else {
            &body
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
            body_for_attempt.len(),
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
            body_for_attempt,
            upstream_is_stream,
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
            CandidateUpstreamDecision::RespondUpstream(mut resp) => {
                let mut status_code = resp.status().as_u16();
                // If the client is continuing a previous session but the selected upstream account belongs
                // to another org/workspace, the server can reject the org-scoped encrypted blobs with:
                // `invalid_encrypted_content`. Attempt a one-shot stateless retry (strip affinity + drop
                // encrypted_content fields) to salvage the request.
                if status_code == 400
                    && !strip_session_affinity
                    && (incoming_turn_state.is_some() || has_body_encrypted_content)
                {
                    let retry_body = if has_body_encrypted_content {
                        if stripped_body.is_none() {
                            stripped_body = strip_encrypted_content_from_body(body.as_ref())
                                .map(bytes::Bytes::from)
                                .or_else(|| Some(body.clone()));
                        }
                        stripped_body
                            .as_ref()
                            .expect("stripped body should be initialized")
                    } else {
                        &body
                    };

                    let retry_decision = process_candidate_upstream_flow(
                        &storage,
                        &method,
                        request_ref,
                        &incoming_headers,
                        retry_body,
                        upstream_is_stream,
                        base,
                        &path,
                        url.as_str(),
                        url_alt.as_deref(),
                        request_deadline,
                        upstream_fallback_base.as_deref(),
                        &account,
                        &mut token,
                        upstream_cookie.as_deref(),
                        true,
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

                    match retry_decision {
                        CandidateUpstreamDecision::RespondUpstream(retry_resp) => {
                            resp = retry_resp;
                            status_code = resp.status().as_u16();
                        }
                        CandidateUpstreamDecision::Failover => {
                            let _ = super::super::clear_manual_preferred_account_if(&account.id);
                            super::super::record_gateway_failover_attempt();
                            continue;
                        }
                        CandidateUpstreamDecision::Terminal { status_code, message } => {
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
                    }
                }

                if status_code >= 400 {
                    let _ = super::super::clear_manual_preferred_account_if(&account.id);
                }
                let mut final_error: Option<String> =
                    if status_code >= 400 { last_attempt_error.clone() } else { None };
                let elapsed_ms = started_at.elapsed().as_millis();
                let request = request
                    .take()
                    .expect("request should be available before terminal response");
                let guard = inflight_guard
                    .take()
                    .expect("inflight guard should be available before terminal response");
                let bridge = super::super::respond_with_upstream(
                    request,
                    resp,
                    guard,
                    response_adapter,
                    client_is_stream,
                )?;
                let bridge_ok = bridge.is_ok(client_is_stream);
                let bridge_error_message = if bridge_ok {
                    None
                } else {
                    Some(
                        bridge
                            .error_message(client_is_stream)
                            .unwrap_or_else(|| "upstream response incomplete".to_string()),
                    )
                };
                if !bridge_ok {
                    let bridge_error = bridge_error_message
                        .as_deref()
                        .unwrap_or("upstream response incomplete");
                    match final_error.as_deref() {
                        Some(existing) if existing != bridge_error => {
                            final_error = Some(format!("{existing}; {bridge_error}"));
                        }
                        None => {
                            final_error = Some(bridge_error.to_string());
                        }
                        _ => {}
                    }
                }

                // 中文注释：流式响应可能以 200 开始，但在未收到终止事件时提前断流（上游 5xx/网络抖动）。
                // 这种情况对客户端等同失败，日志里也应标记为 5xx（或 499 客户端断开）。
                let upstream_stream_failed = client_is_stream
                    && (!bridge.stream_terminal_seen || bridge.stream_terminal_error.is_some());
                let client_delivery_failed = bridge
                    .delivery_error
                    .as_deref()
                    .is_some_and(is_client_disconnect_error);
                let status_for_log = if status_code >= 400 {
                    status_code
                } else if upstream_stream_failed {
                    502
                } else if bridge_ok {
                    status_code
                } else if client_delivery_failed {
                    499
                } else {
                    502
                };

                if upstream_stream_failed {
                    // 下次请求尽量避开该账号，避免连续断流造成体验很差。
                    let _ = super::super::clear_manual_preferred_account_if(&account.id);
                    super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
                    super::super::record_route_quality(&account.id, 502);
                }

                let usage = bridge.usage;
                context.log_final_result(
                    Some(&account.id),
                    last_attempt_url.as_deref(),
                    status_for_log,
                    RequestLogUsage {
                        input_tokens: usage.input_tokens,
                        cached_input_tokens: usage.cached_input_tokens,
                        output_tokens: usage.output_tokens,
                        total_tokens: usage.total_tokens,
                        reasoning_output_tokens: usage.reasoning_output_tokens,
                    },
                    final_error.as_deref(),
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



