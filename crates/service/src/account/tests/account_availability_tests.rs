use super::{evaluate_snapshot, Availability};
use codexmanager_core::storage::UsageSnapshotRecord;

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
fn availability_marks_missing_secondary_available_when_both_secondary_fields_absent() {
    let record = snap(Some(10.0), Some(300), None, None);
    assert!(matches!(
        evaluate_snapshot(&record),
        Availability::Available
    ));
}

#[test]
fn availability_marks_partial_secondary_missing_unavailable() {
    let record = snap(Some(10.0), Some(300), None, Some(10080));
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
