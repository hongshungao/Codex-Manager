use codexmanager_core::storage::UsageSnapshotRecord;

pub(crate) enum Availability {
    Available,
    Unavailable(&'static str),
}

pub(crate) fn evaluate_snapshot(snap: &UsageSnapshotRecord) -> Availability {
    let primary_missing = snap.used_percent.is_none() || snap.window_minutes.is_none();
    let secondary_present =
        snap.secondary_used_percent.is_some() || snap.secondary_window_minutes.is_some();
    let secondary_missing =
        snap.secondary_used_percent.is_none() || snap.secondary_window_minutes.is_none();
    if primary_missing {
        return Availability::Unavailable("usage_missing_primary");
    }
    // 兼容仅返回单窗口额度的账号（如免费周额度）：secondary 完全缺失时视为可用。
    // 但只要 secondary 已出现部分字段，仍要求字段完整，避免异常数据误判可用。
    if secondary_present && secondary_missing {
        return Availability::Unavailable("usage_missing_secondary");
    }
    if let Some(value) = snap.used_percent {
        if value >= 100.0 {
            return Availability::Unavailable("usage_exhausted_primary");
        }
    }
    if let Some(value) = snap.secondary_used_percent {
        if value >= 100.0 {
            return Availability::Unavailable("usage_exhausted_secondary");
        }
    }
    Availability::Available
}

pub(crate) fn is_available(snap: Option<&UsageSnapshotRecord>) -> bool {
    match snap {
        None => true,
        Some(record) => matches!(evaluate_snapshot(record), Availability::Available),
    }
}

#[cfg(test)]
#[path = "tests/account_availability_tests.rs"]
mod tests;
