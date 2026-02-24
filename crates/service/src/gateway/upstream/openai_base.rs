use codexmanager_core::storage::{Account, Storage, Token};

pub(super) enum OpenAiAttemptResult {
    Upstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

pub(super) fn handle_openai_base_attempt<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    path: &str,
    request: &tiny_http::Request,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    body: &[u8],
    is_stream: bool,
    base: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> OpenAiAttemptResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    match super::super::try_openai_fallback(
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
    ) {
        Ok(Some(resp)) => {
            let status = resp.status().as_u16();
            if status < 400 {
                super::super::clear_account_cooldown(&account.id);
            } else {
                super::super::mark_account_cooldown_for_status(&account.id, status);
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
            OpenAiAttemptResult::Upstream(resp)
        }
        Ok(None) => {
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(base), 502, Some("openai upstream unavailable"));
            // 中文注释：OpenAI 上游不可用时如果还有候选账号就继续 failover，
            // 不这样做会把单账号瞬时抖动放大成整次请求失败。
            if has_more_candidates {
                OpenAiAttemptResult::Failover
            } else {
                OpenAiAttemptResult::Terminal {
                    status_code: 502,
                    message: "openai upstream unavailable".to_string(),
                }
            }
        }
        Err(err) => {
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(base), 502, Some(err.as_str()));
            // 中文注释：异常分支同样优先切换候选账号，
            // 只有最后一个候选才直接向客户端返回错误，避免过早失败。
            if has_more_candidates {
                OpenAiAttemptResult::Failover
            } else {
                OpenAiAttemptResult::Terminal {
                    status_code: 502,
                    message: format!("openai upstream error: {err}"),
                }
            }
        }
    }
}


