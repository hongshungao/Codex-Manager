use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::HeaderValue;

pub(super) enum FallbackBranchResult {
    NotTriggered,
    RespondUpstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_openai_fallback_branch<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    request: &tiny_http::Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_base: &str,
    path: &str,
    fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&HeaderValue>,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> FallbackBranchResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    if !allow_openai_fallback {
        return FallbackBranchResult::NotTriggered;
    }

    let should_fallback = super::super::should_try_openai_fallback(upstream_base, path, upstream_content_type)
        || super::super::should_try_openai_fallback_by_status(upstream_base, path, status.as_u16());
    if !should_fallback {
        return FallbackBranchResult::NotTriggered;
    }

    if let Some(fallback_base) = fallback_base {
        if debug {
            log::warn!(
                "event=gateway_upstream_fallback path={} status={} account_id={} from={} to={}",
                path,
                status.as_u16(),
                account.id,
                upstream_base,
                fallback_base
            );
        }
        match super::super::try_openai_fallback(
            client,
            storage,
            method,
            path,
            request,
            incoming_headers,
            body,
            is_stream,
            fallback_base,
            account,
            token,
            upstream_cookie,
            strip_session_affinity,
            debug,
        ) {
            Ok(Some(resp)) => {
                if resp.status().is_success() {
                    super::super::clear_account_cooldown(&account.id);
                    log_gateway_result(Some(fallback_base), resp.status().as_u16(), None);
                    return FallbackBranchResult::RespondUpstream(resp);
                }
                let fallback_status = resp.status().as_u16();
                super::super::mark_account_cooldown_for_status(&account.id, fallback_status);
                log_gateway_result(
                    Some(fallback_base),
                    fallback_status,
                    Some("upstream fallback non-success"),
                );
                // 中文注释：fallback 返回业务错误时优先切换候选账号；
                // 若无候选才透传，避免把可恢复错误过早暴露给客户端。
                if has_more_candidates {
                    FallbackBranchResult::Failover
                } else {
                    FallbackBranchResult::RespondUpstream(resp)
                }
            }
            Ok(None) => {
                super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
                log_gateway_result(Some(fallback_base), 502, Some("upstream fallback unavailable"));
                if has_more_candidates {
                    FallbackBranchResult::Failover
                } else {
                    FallbackBranchResult::Terminal {
                        status_code: 502,
                        message: "upstream blocked by Cloudflare; set CODEXMANAGER_UPSTREAM_COOKIE or enable OpenAI API-key fallback".to_string(),
                    }
                }
            }
            Err(err) => {
                super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
                log_gateway_result(Some(fallback_base), 502, Some(err.as_str()));
                if has_more_candidates {
                    FallbackBranchResult::Failover
                } else {
                    FallbackBranchResult::Terminal {
                        status_code: 502,
                        message: format!("upstream fallback error: {err}"),
                    }
                }
            }
        }
    } else {
        super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Challenge);
        log_gateway_result(
            Some(upstream_base),
            502,
            Some("upstream returned HTML challenge"),
        );
        if has_more_candidates {
            FallbackBranchResult::Failover
        } else {
            FallbackBranchResult::Terminal {
                status_code: 502,
                message: "upstream returned HTML challenge; configure CODEXMANAGER_UPSTREAM_COOKIE or CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL".to_string(),
            }
        }
    }
}


