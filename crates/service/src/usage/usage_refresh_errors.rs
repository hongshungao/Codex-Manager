use codexmanager_core::storage::{now_ts, Event, Storage};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::account_status::set_account_status;

const DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS: i64 = 60;
const USAGE_REFRESH_FAILURE_EVENT_WINDOW_ENV: &str =
    "CODEXMANAGER_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FailureThrottleKey {
    account_id: String,
    error_class: String,
}

static FAILURE_EVENT_THROTTLE: OnceLock<Mutex<HashMap<FailureThrottleKey, i64>>> = OnceLock::new();

pub(super) fn record_usage_refresh_failure(storage: &Storage, account_id: &str, message: &str) {
    let created_at = now_ts();
    let error_class = classify_usage_refresh_error(message);
    let dedupe_window_secs = usage_refresh_failure_event_window_secs();

    if !should_record_failure_event(account_id, &error_class, created_at, dedupe_window_secs) {
        return;
    }

    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "usage_refresh_failed".to_string(),
        message: message.to_string(),
        created_at,
    });
}

pub(super) fn mark_usage_unreachable_if_needed(storage: &Storage, account_id: &str, err: &str) {
    // 中文注释：仅当上游明确返回 usage endpoint 状态错误才降级账号，
    // 否则网络抖动等瞬态错误也会误标 inactive，导致可用账号被过早摘除。
    if err.starts_with("usage endpoint status") {
        set_account_status(storage, account_id, "inactive", "usage_unreachable");
    }
}

pub(super) fn should_retry_with_refresh(err: &str) -> bool {
    err.contains("401") || err.contains("403")
}

fn usage_refresh_failure_event_window_secs() -> i64 {
    std::env::var(USAGE_REFRESH_FAILURE_EVENT_WINDOW_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .map(|secs| secs.max(0))
        .unwrap_or(DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS)
}

fn classify_usage_refresh_error(message: &str) -> String {
    let normalized = message.trim().to_ascii_lowercase();
    if let Some(status_code) = extract_usage_status_code(&normalized) {
        return format!("usage_status_{status_code}");
    }
    if normalized.contains("timeout") {
        return "timeout".to_string();
    }
    if normalized.contains("connection") || normalized.contains("connect") {
        return "connection".to_string();
    }
    if normalized.contains("dns") {
        return "dns".to_string();
    }
    if normalized.contains("storage unavailable") {
        return "storage_unavailable".to_string();
    }
    if normalized.contains("refresh token") || normalized.contains("token refresh") {
        return "token_refresh".to_string();
    }
    "other".to_string()
}

fn extract_usage_status_code(message: &str) -> Option<u16> {
    let rest = message.strip_prefix("usage endpoint status ")?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u16>().ok()
}

fn should_record_failure_event(
    account_id: &str,
    error_class: &str,
    created_at: i64,
    dedupe_window_secs: i64,
) -> bool {
    let key = FailureThrottleKey {
        account_id: account_id.to_string(),
        error_class: error_class.to_string(),
    };
    let throttle = FAILURE_EVENT_THROTTLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut state = throttle.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    should_record_failure_event_with_state(&mut state, key, created_at, dedupe_window_secs)
}

fn should_record_failure_event_with_state(
    state: &mut HashMap<FailureThrottleKey, i64>,
    key: FailureThrottleKey,
    created_at: i64,
    dedupe_window_secs: i64,
) -> bool {
    if dedupe_window_secs <= 0 {
        state.insert(key, created_at);
        return true;
    }

    if let Some(previous) = state.get(&key).copied() {
        let within_window = if created_at <= previous {
            true
        } else {
            created_at - previous < dedupe_window_secs
        };
        if within_window {
            return false;
        }
    }

    state.insert(key, created_at);
    prune_failure_event_state(state, created_at, dedupe_window_secs);
    true
}

fn prune_failure_event_state(
    state: &mut HashMap<FailureThrottleKey, i64>,
    now: i64,
    dedupe_window_secs: i64,
) {
    let retain_secs = dedupe_window_secs
        .saturating_mul(10)
        .max(DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS);
    state.retain(|_, recorded_at| {
        if *recorded_at > now {
            true
        } else {
            now - *recorded_at <= retain_secs
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{classify_usage_refresh_error, should_record_failure_event_with_state, FailureThrottleKey};
    use std::collections::HashMap;

    #[test]
    fn usage_refresh_error_class_groups_by_status_code() {
        assert_eq!(
            classify_usage_refresh_error("usage endpoint status 500 Internal Server Error"),
            "usage_status_500"
        );
        assert_eq!(
            classify_usage_refresh_error("usage endpoint status 503 Service Unavailable"),
            "usage_status_503"
        );
    }

    #[test]
    fn usage_refresh_error_class_catches_timeout_and_connection() {
        assert_eq!(
            classify_usage_refresh_error("request timeout while calling usage"),
            "timeout"
        );
        assert_eq!(
            classify_usage_refresh_error("connection reset by peer"),
            "connection"
        );
        assert_eq!(classify_usage_refresh_error("unknown error"), "other");
    }

    #[test]
    fn failure_event_throttle_dedupes_within_window() {
        let mut state = HashMap::new();
        let key = FailureThrottleKey {
            account_id: "acc-1".to_string(),
            error_class: "usage_status_500".to_string(),
        };

        assert!(should_record_failure_event_with_state(
            &mut state,
            key.clone(),
            100,
            60
        ));
        assert!(!should_record_failure_event_with_state(
            &mut state,
            key.clone(),
            120,
            60
        ));
        assert!(should_record_failure_event_with_state(
            &mut state,
            key,
            161,
            60
        ));
    }

    #[test]
    fn failure_event_throttle_isolated_by_error_class() {
        let mut state = HashMap::new();
        let key_500 = FailureThrottleKey {
            account_id: "acc-1".to_string(),
            error_class: "usage_status_500".to_string(),
        };
        let key_timeout = FailureThrottleKey {
            account_id: "acc-1".to_string(),
            error_class: "timeout".to_string(),
        };

        assert!(should_record_failure_event_with_state(
            &mut state,
            key_500,
            100,
            60
        ));
        assert!(should_record_failure_event_with_state(
            &mut state,
            key_timeout,
            110,
            60
        ));
    }
}
