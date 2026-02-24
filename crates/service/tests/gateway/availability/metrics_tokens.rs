use super::*;
use std::thread;
use std::time::Duration;

fn metric_value(text: &str, name: &str) -> u64 {
    text.lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let metric_name = parts.next()?;
            if metric_name != name {
                return None;
            }
            parts.next()?.parse::<u64>().ok()
        })
        .unwrap_or(0)
}

#[test]
fn token_exchange_lock_reuses_same_account_lock() {
    let first = account_token_exchange_lock("acc-1");
    let second = account_token_exchange_lock("acc-1");
    let third = account_token_exchange_lock("acc-2");
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first, &third));
}

#[test]
fn metrics_prometheus_contains_expected_series() {
    let text = gateway_metrics_prometheus();
    assert!(text.contains("codexmanager_gateway_requests_total "));
    assert!(text.contains("codexmanager_gateway_requests_active "));
    assert!(text.contains("codexmanager_gateway_account_inflight_total "));
    assert!(text.contains("codexmanager_gateway_failover_attempts_total "));
    assert!(text.contains("codexmanager_gateway_cooldown_marks_total "));
    assert!(text.contains("codexmanager_rpc_requests_total "));
    assert!(text.contains("codexmanager_rpc_requests_failed_total "));
    assert!(text.contains("codexmanager_rpc_request_duration_milliseconds_total "));
    assert!(text.contains("codexmanager_rpc_request_duration_milliseconds_count "));
    assert!(text.contains("codexmanager_usage_refresh_attempts_total "));
    assert!(text.contains("codexmanager_usage_refresh_success_total "));
    assert!(text.contains("codexmanager_usage_refresh_failures_total "));
    assert!(text.contains("codexmanager_usage_refresh_duration_milliseconds_total "));
    assert!(text.contains("codexmanager_usage_refresh_duration_milliseconds_count "));
}

#[test]
fn rpc_metrics_track_failures_and_duration() {
    let before = gateway_metrics_prometheus();
    let before_total = metric_value(&before, "codexmanager_rpc_requests_total");
    let before_failed = metric_value(&before, "codexmanager_rpc_requests_failed_total");
    let before_duration =
        metric_value(&before, "codexmanager_rpc_request_duration_milliseconds_total");

    {
        let mut guard = begin_rpc_request();
        thread::sleep(Duration::from_millis(2));
        guard.mark_success();
    }
    {
        let _guard = begin_rpc_request();
    }

    let after = gateway_metrics_prometheus();
    let after_total = metric_value(&after, "codexmanager_rpc_requests_total");
    let after_failed = metric_value(&after, "codexmanager_rpc_requests_failed_total");
    let after_duration = metric_value(&after, "codexmanager_rpc_request_duration_milliseconds_total");

    assert!(after_total >= before_total + 2);
    assert!(after_failed >= before_failed + 1);
    assert!(after_duration >= before_duration + 1);
}

#[test]
fn usage_refresh_metrics_track_success_and_failure() {
    let before = gateway_metrics_prometheus();
    let before_attempts = metric_value(&before, "codexmanager_usage_refresh_attempts_total");
    let before_success = metric_value(&before, "codexmanager_usage_refresh_success_total");
    let before_failures = metric_value(&before, "codexmanager_usage_refresh_failures_total");
    let before_duration =
        metric_value(&before, "codexmanager_usage_refresh_duration_milliseconds_total");

    record_usage_refresh_outcome(true, 3);
    record_usage_refresh_outcome(false, 7);

    let after = gateway_metrics_prometheus();
    let after_attempts = metric_value(&after, "codexmanager_usage_refresh_attempts_total");
    let after_success = metric_value(&after, "codexmanager_usage_refresh_success_total");
    let after_failures = metric_value(&after, "codexmanager_usage_refresh_failures_total");
    let after_duration = metric_value(&after, "codexmanager_usage_refresh_duration_milliseconds_total");

    assert!(after_attempts >= before_attempts + 2);
    assert!(after_success >= before_success + 1);
    assert!(after_failures >= before_failures + 1);
    assert!(after_duration >= before_duration + 10);
}
