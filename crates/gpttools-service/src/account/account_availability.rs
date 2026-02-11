use gpttools_core::storage::UsageSnapshotRecord;

pub(crate) enum Availability {
    Available,
    Unavailable(&'static str),
}

pub(crate) fn evaluate_snapshot(snap: &UsageSnapshotRecord) -> Availability {
    let primary_missing = snap.used_percent.is_none() || snap.window_minutes.is_none();
    let secondary_missing =
        snap.secondary_used_percent.is_none() || snap.secondary_window_minutes.is_none();
    if primary_missing {
        return Availability::Unavailable("usage_missing_primary");
    }
    if secondary_missing {
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
mod tests {
    use super::{evaluate_snapshot, Availability};
    use gpttools_core::storage::UsageSnapshotRecord;

    fn snap(
        primary_used: Option<f64>,
        primary_window: Option<i64>,
        secondary_used: Option<f64>,
        secondary_window: Option<i64>,
    ) -> UsageSnapshotRecord {
        UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: primary_used,
            window_minutes: primary_window,
            resets_at: None,
            secondary_used_percent: secondary_used,
            secondary_window_minutes: secondary_window,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: 0,
        }
    }

    #[test]
    fn availability_marks_missing_primary_unavailable() {
        let record = snap(None, Some(300), Some(10.0), Some(10080));
        assert!(matches!(
            evaluate_snapshot(&record),
            Availability::Unavailable(_)
        ));
    }

    #[test]
    fn availability_marks_exhausted_secondary_unavailable() {
        let record = snap(Some(10.0), Some(300), Some(100.0), Some(10080));
        assert!(matches!(
            evaluate_snapshot(&record),
            Availability::Unavailable(_)
        ));
    }

    #[test]
    fn availability_marks_ok_available() {
        let record = snap(Some(10.0), Some(300), Some(20.0), Some(10080));
        assert!(matches!(
            evaluate_snapshot(&record),
            Availability::Available
        ));
    }
}
