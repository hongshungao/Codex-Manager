use rand::Rng;
use std::thread;
use std::time::Duration;

pub(crate) const DEFAULT_USAGE_POLL_INTERVAL_SECS: u64 = 600;
pub(crate) const DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS: u64 = 180;
pub(crate) const DEFAULT_USAGE_POLL_JITTER_SECS: u64 = 5;
pub(crate) const DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS: u64 = 5;
pub(crate) const DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS: u64 = 1800;
pub(crate) const DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS: u64 = 900;
pub(crate) const MIN_USAGE_POLL_INTERVAL_SECS: u64 = 30;
pub(crate) const MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS: u64 = 30;

#[allow(dead_code)]
pub(crate) fn run_blocking_poll_loop<F, L>(
    loop_name: &str,
    interval: Duration,
    jitter: Duration,
    failure_backoff_cap: Duration,
    mut task: F,
    mut should_log_error: L,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
{
    let jitter_cap_secs = jitter.as_secs();
    let mut rng = rand::thread_rng();
    run_blocking_poll_loop_with_sleep(
        loop_name,
        interval,
        jitter,
        failure_backoff_cap,
        &mut task,
        &mut should_log_error,
        |d| {
            thread::sleep(d);
            true
        },
        &mut || {
            if jitter_cap_secs == 0 {
                Duration::ZERO
            } else {
                Duration::from_secs(rng.gen_range(0..=jitter_cap_secs))
            }
        },
    );
}

#[allow(dead_code)]
pub(crate) fn run_blocking_poll_loop_with_sleep<F, L, S, J>(
    loop_name: &str,
    interval: Duration,
    jitter: Duration,
    failure_backoff_cap: Duration,
    task: &mut F,
    should_log_error: &mut L,
    mut sleep: S,
    next_jitter: &mut J,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
    S: FnMut(Duration) -> bool,
    J: FnMut() -> Duration,
{
    let mut consecutive_failures = 0u32;
    loop {
        let succeeded = match task() {
            Ok(_) => true,
            Err(err) => {
                if should_log_error(err.as_str()) {
                    log::warn!("{loop_name} error: {err}");
                }
                false
            }
        };

        if succeeded {
            consecutive_failures = 0;
        } else {
            consecutive_failures = consecutive_failures.saturating_add(1);
        }

        let delay = next_poll_delay(
            interval,
            jitter,
            failure_backoff_cap,
            consecutive_failures,
            next_jitter(),
        );
        if !sleep(delay) {
            break;
        }
    }
}

#[allow(dead_code)]
fn next_poll_delay(
    interval: Duration,
    jitter_cap: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
    sampled_jitter: Duration,
) -> Duration {
    let base_delay = next_failure_backoff(interval, failure_backoff_cap, consecutive_failures);
    let bounded_jitter = if jitter_cap.is_zero() {
        Duration::ZERO
    } else {
        sampled_jitter.min(jitter_cap)
    };
    base_delay
        .checked_add(bounded_jitter)
        .unwrap_or(Duration::MAX)
}

#[allow(dead_code)]
fn next_failure_backoff(
    interval: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
) -> Duration {
    if consecutive_failures == 0 {
        return interval;
    }

    let base_ms = interval.as_millis();
    if base_ms == 0 {
        return interval;
    }

    let cap_ms = failure_backoff_cap.max(interval).as_millis();
    let shift = (consecutive_failures.saturating_sub(1)).min(20);
    let multiplier = 1u128 << shift;
    let scaled_ms = base_ms.saturating_mul(multiplier);
    let bounded_ms = scaled_ms.min(cap_ms).max(base_ms);
    duration_from_millis(bounded_ms)
}

#[allow(dead_code)]
fn duration_from_millis(ms: u128) -> Duration {
    if ms > u64::MAX as u128 {
        Duration::from_millis(u64::MAX)
    } else {
        Duration::from_millis(ms as u64)
    }
}

pub(crate) fn parse_interval_secs(raw: Option<&str>, default_secs: u64, min_secs: u64) -> u64 {
    // 中文注释：低于最小间隔会导致线程空转并放大上游压力；这里统一夹紧，避免配置误填把服务打满。
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .map(|secs| secs.max(min_secs))
        .unwrap_or(default_secs)
}

#[cfg(test)]
#[path = "tests/usage_scheduler_tests.rs"]
mod tests;
