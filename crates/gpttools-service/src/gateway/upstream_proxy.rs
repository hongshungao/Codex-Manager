use reqwest::header::{HeaderName, HeaderValue};
use reqwest::header::CONTENT_TYPE;
use tiny_http::{Request, Response};
use gpttools_core::storage::Account;

use super::local_validation::LocalValidationResult;

fn should_drop_header_for_attempt(name: &str, strip_session_affinity: bool) -> bool {
    if strip_session_affinity {
        super::should_drop_incoming_header_for_failover(name)
    } else {
        super::should_drop_incoming_header(name)
    }
}

fn send_upstream_request(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request: &Request,
    body: &[u8],
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let mut builder = client.request(method.clone(), target_url);
    let mut has_user_agent = false;
    for header in request.headers() {
        let name = header.field.as_str().as_str();
        if should_drop_header_for_attempt(name, strip_session_affinity) {
            continue;
        }
        if header.field.equiv("User-Agent") {
            has_user_agent = true;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if !has_user_agent {
        builder = builder.header("User-Agent", "codex-cli");
    }
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }
    builder = builder.header("Authorization", format!("Bearer {}", auth_token));
    if let Some(acc) = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref())
    {
        builder = builder.header("ChatGPT-Account-Id", acc);
    }
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }
    builder.send()
}

pub(super) fn proxy_validated_request(
    request: Request,
    validated: LocalValidationResult,
    debug: bool,
) -> Result<(), String> {
    let LocalValidationResult {
        storage,
        path,
        body,
        request_method,
        key_id,
        model_for_log,
        reasoning_for_log,
        method,
    } = validated;

    let mut candidates = match super::collect_gateway_candidates(&storage) {
        Ok(v) => v,
        Err(err) => {
            let err_text = format!("candidate resolve failed: {err}");
            super::write_request_log(
                &storage,
                Some(&key_id),
                &path,
                &request_method,
                model_for_log.as_deref(),
                reasoning_for_log.as_deref(),
                None,
                Some(500),
                Some(err_text.as_str()),
            );
            let response = Response::from_string(err_text).with_status_code(500);
            let _ = request.respond(response);
            return Ok(());
        }
    };
    if candidates.is_empty() {
        super::write_request_log(
            &storage,
            Some(&key_id),
            &path,
            &request_method,
            model_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            None,
            Some(503),
            Some("no available account"),
        );
        let response = Response::from_string("no available account").with_status_code(503);
        let _ = request.respond(response);
        return Ok(());
    }
    // 中文注释：先避开冷却中的账号，再按并发负载排序，减少并发时反复命中不稳定账号。
    candidates.sort_by_key(|(account, _)| {
        (
            super::is_account_in_cooldown(&account.id),
            super::account_inflight_count(&account.id),
        )
    });
    super::rotate_candidates_for_fairness(&mut candidates);

    let upstream_base = super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::resolve_upstream_fallback_base_url(base);
    let (url, url_alt) = super::compute_upstream_url(base, &path);

    let client = super::upstream_client();
    let upstream_cookie = std::env::var("GPTTOOLS_UPSTREAM_COOKIE").ok();

    let candidate_count = candidates.len();
    let account_max_inflight = super::account_max_inflight_limit();
    let has_more_candidates = |idx: usize| idx + 1 < candidate_count;
    let log_gateway_result = |upstream_url: Option<&str>, status_code: u16, error: Option<&str>| {
        super::write_request_log(
            &storage,
            Some(&key_id),
            &path,
            &request_method,
            model_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            upstream_url,
            Some(status_code),
            error,
        );
    };
    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        let strip_session_affinity = idx > 0;
        if super::is_account_in_cooldown(&account.id) && has_more_candidates(idx) {
            super::record_gateway_failover_attempt();
            continue;
        }
        if account_max_inflight > 0
            && super::account_inflight_count(&account.id) >= account_max_inflight
            && has_more_candidates(idx)
        {
            // 中文注释：上限仅作为软约束，优先跳过；若已是最后候选则继续尝试，避免直接 503。
            super::record_gateway_failover_attempt();
            continue;
        }
        // 中文注释：把 inflight 计数覆盖到整个响应生命周期，确保下一批请求能看到真实负载。
        let inflight_guard = super::acquire_account_inflight(&account.id);
        if super::is_openai_api_base(base) {
            match super::try_openai_fallback(
                &client,
                &storage,
                &method,
                &request,
                &body,
                base,
                &account,
                &mut token,
                upstream_cookie.as_deref(),
                strip_session_affinity,
                debug,
            ) {
                Ok(Some(resp)) => {
                    let status = resp.status().as_u16();
                    if status < 400 {
                        super::clear_account_cooldown(&account.id);
                    } else {
                        super::mark_account_cooldown_for_status(&account.id, status);
                    }
                    log_gateway_result(
                        Some(base),
                        status,
                        if status >= 400 {
                            Some("openai upstream non-success")
                        } else {
                            None
                        },
                    );
                    return super::respond_with_upstream(request, resp, inflight_guard);
                }
                Ok(None) => {
                    super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                    log_gateway_result(Some(base), 502, Some("openai upstream unavailable"));
                    // 中文注释：并发场景下某个账号可能临时不可用；如果还有候选账号，继续切换而不是直接失败。
                    if has_more_candidates(idx) {
                        super::record_gateway_failover_attempt();
                        continue;
                    }
                    let response =
                        Response::from_string("openai upstream unavailable").with_status_code(502);
                    let _ = request.respond(response);
                    return Ok(());
                }
                Err(err) => {
                    super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                    log_gateway_result(Some(base), 502, Some(err.as_str()));
                    // 中文注释：这里若直接返回，会把“单账号瞬时失败”放大成“整次请求失败”；
                    // 继续 failover 能让同一 key 在并发下命中其它可用账号。
                    if has_more_candidates(idx) {
                        super::record_gateway_failover_attempt();
                        continue;
                    }
                    let response = Response::from_string(format!("openai upstream error: {err}"))
                        .with_status_code(502);
                    let _ = request.respond(response);
                    return Ok(());
                }
            }
        }

        let auth_token = token.access_token.clone();
        if debug {
            eprintln!(
                "gateway upstream: base={}, token_source=access_token",
                upstream_base
            );
        }
        let mut upstream = match send_upstream_request(
            &client,
            &method,
            &url,
            &request,
            &body,
            upstream_cookie.as_deref(),
            auth_token.as_str(),
            &account,
            strip_session_affinity,
        ) {
            Ok(resp) => resp,
            Err(err) => {
                let err_msg = err.to_string();
                super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                log_gateway_result(Some(url.as_str()), 502, Some(err_msg.as_str()));
                // 中文注释：上游连接错误可能是短暂抖动或单账号限流，不应立刻结束整次请求。
                if has_more_candidates(idx) {
                    super::record_gateway_failover_attempt();
                    continue;
                }
                let response =
                    Response::from_string(format!("upstream error: {err}")).with_status_code(502);
                let _ = request.respond(response);
                return Ok(());
            }
        };

        let mut status = upstream.status();
        if super::should_try_openai_fallback(base, &path, upstream.headers().get(CONTENT_TYPE))
            || super::should_try_openai_fallback_by_status(base, &path, status.as_u16())
        {
            if let Some(fallback_base) = upstream_fallback_base.as_deref() {
                if debug {
                    eprintln!(
                        "gateway upstream fallback: from={} to={}",
                        upstream_base, fallback_base
                    );
                }
                match super::try_openai_fallback(
                    &client,
                    &storage,
                    &method,
                    &request,
                    &body,
                    fallback_base,
                    &account,
                    &mut token,
                    upstream_cookie.as_deref(),
                    strip_session_affinity,
                    debug,
                ) {
                    Ok(Some(resp)) => {
                        if resp.status().is_success() {
                            super::clear_account_cooldown(&account.id);
                            log_gateway_result(Some(fallback_base), resp.status().as_u16(), None);
                            return super::respond_with_upstream(request, resp, inflight_guard);
                        }
                        let fallback_status = resp.status().as_u16();
                        super::mark_account_cooldown_for_status(&account.id, fallback_status);
                        log_gateway_result(
                            Some(fallback_base),
                            fallback_status,
                            Some("upstream fallback non-success"),
                        );
                        // 中文注释：fallback 明确返回业务错误时优先切换候选账号；
                        // 若已是最后候选则直接透传该错误，避免误报成 Cloudflare/WAF。
                        if has_more_candidates(idx) {
                            super::record_gateway_failover_attempt();
                            continue;
                        }
                        return super::respond_with_upstream(request, resp, inflight_guard);
                    }
                    Ok(None) => {
                        super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                        log_gateway_result(
                            Some(fallback_base),
                            502,
                            Some("upstream fallback unavailable"),
                        );
                        // 中文注释：fallback 基座不可用时仍可尝试下一个账号，避免单点失败。
                        if has_more_candidates(idx) {
                            super::record_gateway_failover_attempt();
                            continue;
                        }
                        let response = Response::from_string(
                            "upstream blocked by Cloudflare; set GPTTOOLS_UPSTREAM_COOKIE or enable OpenAI API-key fallback",
                        )
                        .with_status_code(502);
                        let _ = request.respond(response);
                        return Ok(());
                    }
                    Err(err) => {
                        super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                        log_gateway_result(Some(fallback_base), 502, Some(err.as_str()));
                        // 中文注释：fallback 调用报错时优先切到其它候选账号，只有最后一个候选才直接对外失败。
                        if has_more_candidates(idx) {
                            super::record_gateway_failover_attempt();
                            continue;
                        }
                        let response =
                            Response::from_string(format!("upstream fallback error: {err}"))
                                .with_status_code(502);
                        let _ = request.respond(response);
                        return Ok(());
                    }
                }
            } else {
                super::mark_account_cooldown(&account.id, super::CooldownReason::Challenge);
                log_gateway_result(
                    Some(upstream_base.as_str()),
                    502,
                    Some("upstream returned HTML challenge"),
                );
                if has_more_candidates(idx) {
                    super::record_gateway_failover_attempt();
                    continue;
                }
                let response = Response::from_string(
                    "upstream returned HTML challenge; configure GPTTOOLS_UPSTREAM_COOKIE or GPTTOOLS_UPSTREAM_FALLBACK_BASE_URL",
                )
                .with_status_code(502);
                let _ = request.respond(response);
                return Ok(());
            }
        }
        if !status.is_success() {
            log::warn!(
                "gateway upstream non-success: status={}, account_id={}",
                status,
                account.id
            );
        }
        if (status.as_u16() == 400 || status.as_u16() == 404) && url_alt.is_some() {
            let alt_url = url_alt.as_ref().unwrap();
            if debug {
                eprintln!("gateway upstream retry: url={alt_url}");
            }
            match send_upstream_request(
                &client,
                &method,
                alt_url,
                &request,
                &body,
                upstream_cookie.as_deref(),
                auth_token.as_str(),
                &account,
                strip_session_affinity,
            ) {
                Ok(resp) => upstream = resp,
                Err(err) => {
                    let err_msg = err.to_string();
                    super::mark_account_cooldown(&account.id, super::CooldownReason::Network);
                    if has_more_candidates(idx) {
                        super::record_gateway_failover_attempt();
                        continue;
                    }
                    let response =
                        Response::from_string(format!("upstream error: {err}")).with_status_code(502);
                    log_gateway_result(Some(alt_url.as_str()), 502, Some(err_msg.as_str()));
                    let _ = request.respond(response);
                    return Ok(());
                }
            }
            status = upstream.status();
        }

        if !strip_session_affinity && matches!(status.as_u16(), 401 | 403 | 404) {
            if debug {
                eprintln!(
                    "gateway upstream stateless retry: account_id={}, status={}",
                    account.id, status
                );
            }
            match send_upstream_request(
                &client,
                &method,
                &url,
                &request,
                &body,
                upstream_cookie.as_deref(),
                auth_token.as_str(),
                &account,
                true,
            ) {
                Ok(resp) => {
                    upstream = resp;
                    status = upstream.status();
                    if (status.as_u16() == 400 || status.as_u16() == 404) && url_alt.is_some() {
                        let alt_url = url_alt.as_ref().unwrap();
                        match send_upstream_request(
                            &client,
                            &method,
                            alt_url,
                            &request,
                            &body,
                            upstream_cookie.as_deref(),
                            auth_token.as_str(),
                            &account,
                            true,
                        ) {
                            Ok(resp) => {
                                upstream = resp;
                                status = upstream.status();
                            }
                            Err(err) => {
                                log::warn!(
                                    "gateway stateless alt retry error: account_id={}, err={}",
                                    account.id,
                                    err
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    log::warn!(
                        "gateway stateless retry error: account_id={}, err={}",
                        account.id,
                        err
                    );
                }
            }
        }

        if matches!(status.as_u16(), 429 | 500..=599) {
            // 中文注释：即使本次把上游错误透传给客户端，也要对账号做退避，避免下一批并发继续打在故障账号上。
            super::mark_account_cooldown_for_status(&account.id, status.as_u16());
        }
        if status.is_success() {
            super::clear_account_cooldown(&account.id);
            log_gateway_result(Some(url.as_str()), status.as_u16(), None);
            return super::respond_with_upstream(request, upstream, inflight_guard);
        }

        // Cloudflare / WAF challenge 不应透传给客户端；优先切换候选账号重试。
        let is_challenge =
            super::is_upstream_challenge_response(status.as_u16(), upstream.headers().get(CONTENT_TYPE));
        if is_challenge {
            super::mark_account_cooldown(&account.id, super::CooldownReason::Challenge);
            log_gateway_result(
                Some(url.as_str()),
                status.as_u16(),
                Some("upstream challenge blocked"),
            );
            if has_more_candidates(idx) {
                super::record_gateway_failover_attempt();
                continue;
            }
            let response = Response::from_string(
                "upstream blocked by Cloudflare/WAF; please refresh account auth or configure GPTTOOLS_UPSTREAM_COOKIE",
            )
            .with_status_code(502);
            let _ = request.respond(response);
            return Ok(());
        }

        let refresh_result = crate::usage_refresh::refresh_usage_for_account(&account.id);
        let should_failover = super::should_failover_after_refresh(&storage, &account.id, refresh_result);
        if should_failover {
            super::mark_account_cooldown_for_status(&account.id, status.as_u16());
        }
        log_gateway_result(Some(url.as_str()), status.as_u16(), Some("upstream non-success"));
        if should_failover && has_more_candidates(idx) {
            super::record_gateway_failover_attempt();
            continue;
        }

        return super::respond_with_upstream(request, upstream, inflight_guard);
    }

    log_gateway_result(Some(base), 503, Some("no available account"));
    let response = Response::from_string("no available account").with_status_code(503);
    let _ = request.respond(response);
    Ok(())
}
