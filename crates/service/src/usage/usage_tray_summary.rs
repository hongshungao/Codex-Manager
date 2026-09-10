use codexmanager_core::storage::now_ts;

use crate::storage_helpers::open_storage;

const MINUTES_PER_DAY: i64 = 24 * 60;
const WINDOW_ROUNDING_BIAS_MINUTES: i64 = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayUsageResetSummary {
    pub five_hour_resets_at: Option<i64>,
    pub seven_day_resets_at: Option<i64>,
    pub five_hour_known_count: usize,
    pub seven_day_known_count: usize,
}

#[derive(Clone, Copy)]
enum ResetWindow {
    FiveHour,
    SevenDay,
}

pub fn read_tray_usage_reset_summary() -> TrayUsageResetSummary {
    let Some(storage) = open_storage() else {
        return TrayUsageResetSummary::default();
    };
    let Ok(items) = storage.latest_usage_snapshots_by_account() else {
        return TrayUsageResetSummary::default();
    };
    let now = now_ts();
    let mut summary = TrayUsageResetSummary::default();
    for item in items {
        add_reset_window(
            &mut summary,
            item.window_minutes,
            item.resets_at,
            ResetWindow::FiveHour,
            now,
        );
        add_reset_window(
            &mut summary,
            item.secondary_window_minutes,
            item.secondary_resets_at,
            ResetWindow::SevenDay,
            now,
        );
    }
    summary
}

fn add_reset_window(
    summary: &mut TrayUsageResetSummary,
    window_minutes: Option<i64>,
    resets_at: Option<i64>,
    fallback: ResetWindow,
    now: i64,
) {
    let Some(resets_at) = future_ts(resets_at, now) else {
        return;
    };
    let window = match window_minutes {
        Some(minutes) if minutes > MINUTES_PER_DAY + WINDOW_ROUNDING_BIAS_MINUTES => {
            ResetWindow::SevenDay
        }
        Some(_) => ResetWindow::FiveHour,
        None => fallback,
    };
    match window {
        ResetWindow::FiveHour => {
            summary.five_hour_known_count += 1;
            summary.five_hour_resets_at = min_ts(summary.five_hour_resets_at, resets_at);
        }
        ResetWindow::SevenDay => {
            summary.seven_day_known_count += 1;
            summary.seven_day_resets_at = min_ts(summary.seven_day_resets_at, resets_at);
        }
    }
}

fn future_ts(value: Option<i64>, now: i64) -> Option<i64> {
    value.filter(|ts| *ts > now)
}

fn min_ts(current: Option<i64>, candidate: i64) -> Option<i64> {
    Some(
        current
            .map(|value| value.min(candidate))
            .unwrap_or(candidate),
    )
}

#[cfg(test)]
mod tests {
    use super::{add_reset_window, future_ts, min_ts, ResetWindow, TrayUsageResetSummary};

    #[test]
    fn future_ts_ignores_missing_or_elapsed_values() {
        assert_eq!(future_ts(None, 100), None);
        assert_eq!(future_ts(Some(99), 100), None);
        assert_eq!(future_ts(Some(100), 100), None);
        assert_eq!(future_ts(Some(101), 100), Some(101));
    }

    #[test]
    fn min_ts_keeps_earliest_value() {
        assert_eq!(min_ts(None, 120), Some(120));
        assert_eq!(min_ts(Some(180), 120), Some(120));
        assert_eq!(min_ts(Some(90), 120), Some(90));
    }

    #[test]
    fn single_primary_seven_day_window_is_not_reported_as_five_hour() {
        let mut summary = TrayUsageResetSummary::default();

        add_reset_window(
            &mut summary,
            Some(10_080),
            Some(200),
            ResetWindow::FiveHour,
            100,
        );

        assert_eq!(summary.five_hour_resets_at, None);
        assert_eq!(summary.five_hour_known_count, 0);
        assert_eq!(summary.seven_day_resets_at, Some(200));
        assert_eq!(summary.seven_day_known_count, 1);
    }

    #[test]
    fn reset_windows_are_classified_by_duration_when_fields_are_swapped() {
        let mut summary = TrayUsageResetSummary::default();

        add_reset_window(
            &mut summary,
            Some(10_080),
            Some(300),
            ResetWindow::FiveHour,
            100,
        );
        add_reset_window(
            &mut summary,
            Some(300),
            Some(200),
            ResetWindow::SevenDay,
            100,
        );

        assert_eq!(summary.five_hour_resets_at, Some(200));
        assert_eq!(summary.seven_day_resets_at, Some(300));
    }
}
