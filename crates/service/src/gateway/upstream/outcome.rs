use codexmanager_core::storage::Storage;
use reqwest::header::HeaderValue;

pub(super) enum UpstreamOutcomeDecision {
    Failover,
    Terminal { status_code: u16, message: String },
    RespondUpstream,
}

pub(super) fn decide_upstream_outcome<F>(
    storage: &Storage,
    account_id: &str,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&HeaderValue>,
    url: &str,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> UpstreamOutcomeDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    if matches!(status.as_u16(), 429 | 500..=599) {
        // 中文注释：即使当前响应会回给客户端，也要先标记冷却，
        // 否则并发流量会继续命中同一故障账号造成雪崩。
        super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
    }
    if status.is_success() {
        super::super::clear_account_cooldown(account_id);
        log_gateway_result(Some(url), status.as_u16(), None);
        return UpstreamOutcomeDecision::RespondUpstream;
    }
    if status.as_u16() == 404 && has_more_candidates {
        // 中文注释：模型/路径 404 在多账号场景下通常是“该账号不可用”，
        // 优先切换候选账号，最后一个候选再透传原始 404 给客户端。
        super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        log_gateway_result(Some(url), status.as_u16(), Some("upstream not-found failover"));
        return UpstreamOutcomeDecision::Failover;
    }

    let is_challenge = super::super::is_upstream_challenge_response(status.as_u16(), upstream_content_type);
    if is_challenge {
        super::super::mark_account_cooldown(account_id, super::super::CooldownReason::Challenge);
        log_gateway_result(Some(url), status.as_u16(), Some("upstream challenge blocked"));
        if has_more_candidates {
            return UpstreamOutcomeDecision::Failover;
        }
        return UpstreamOutcomeDecision::Terminal {
            status_code: 502,
            message: "upstream blocked by Cloudflare/WAF; please refresh account auth or configure CODEXMANAGER_UPSTREAM_COOKIE".to_string(),
        };
    }

    let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
    let should_failover = super::super::should_failover_from_cached_snapshot(storage, account_id);
    if should_failover {
        super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
    }
    log_gateway_result(Some(url), status.as_u16(), Some("upstream non-success"));
    if should_failover && has_more_candidates {
        return UpstreamOutcomeDecision::Failover;
    }

    UpstreamOutcomeDecision::RespondUpstream
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_404_with_more_candidates_triggers_failover() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let decision = decide_upstream_outcome(
            &storage,
            "acc-404",
            reqwest::StatusCode::NOT_FOUND,
            None,
            "https://chatgpt.com/backend-api/codex/chat/completions",
            true,
            |_, _, _| {},
        );
        assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
    }

    #[test]
    fn status_404_on_last_candidate_keeps_upstream_response() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let decision = decide_upstream_outcome(
            &storage,
            "acc-404",
            reqwest::StatusCode::NOT_FOUND,
            None,
            "https://chatgpt.com/backend-api/codex/chat/completions",
            false,
            |_, _, _| {},
        );
        assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
    }
}


