use codexmanager_core::storage::{Account, Storage, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateSkipReason {
    Cooldown,
    Inflight,
}

pub(crate) fn prepare_gateway_candidates(
    storage: &Storage,
) -> Result<Vec<(Account, Token)>, String> {
    // 中文注释：保持账号原始顺序（按账户排序字段）作为候选顺序，失败时再依次切下一个。
    super::super::collect_gateway_candidates(storage)
}

pub(crate) fn candidate_skip_reason_for_proxy(
    account_id: &str,
    idx: usize,
    candidate_count: usize,
    account_max_inflight: usize,
) -> Option<CandidateSkipReason> {
    // 中文注释：当用户手动“切到当前”后，首候选应持续优先命中；
    // 仅在真实请求失败时由上游流程自动清除手动锁定，再回退常规轮转。
    let is_manual_preferred_head = idx == 0
        && super::super::manual_preferred_account()
            .as_deref()
            .is_some_and(|manual_id| manual_id == account_id);
    if is_manual_preferred_head {
        return None;
    }

    let has_more_candidates = idx + 1 < candidate_count;
    if super::super::is_account_in_cooldown(account_id) && has_more_candidates {
        super::super::record_gateway_failover_attempt();
        return Some(CandidateSkipReason::Cooldown);
    }

    if account_max_inflight > 0
        && super::super::account_inflight_count(account_id) >= account_max_inflight
        && has_more_candidates
    {
        // 中文注释：并发上限是软约束，最后一个候选仍要尝试，避免把可恢复抖动直接放大成全局不可用。
        super::super::record_gateway_failover_attempt();
        return Some(CandidateSkipReason::Inflight);
    }

    None
}




