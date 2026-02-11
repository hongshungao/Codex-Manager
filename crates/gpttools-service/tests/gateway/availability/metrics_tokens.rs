use super::*;

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
    assert!(text.contains("gpttools_gateway_requests_total "));
    assert!(text.contains("gpttools_gateway_requests_active "));
    assert!(text.contains("gpttools_gateway_account_inflight_total "));
    assert!(text.contains("gpttools_gateway_failover_attempts_total "));
    assert!(text.contains("gpttools_gateway_cooldown_marks_total "));
}
