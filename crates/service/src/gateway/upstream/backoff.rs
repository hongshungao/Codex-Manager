use rand::Rng;
use std::time::{Duration, Instant};

fn as_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

pub(super) fn exponential_jitter_delay(
    base: Duration,
    cap: Duration,
    attempt: u32,
) -> Duration {
    let base_ms = as_millis_u64(base);
    let cap_ms = as_millis_u64(cap);
    if base_ms == 0 || cap_ms == 0 {
        return Duration::from_millis(0);
    }
    let multiplier = 1_u64 << attempt.min(10);
    let max_delay_ms = base_ms.saturating_mul(multiplier).min(cap_ms).max(1);
    let jitter_ms = rand::thread_rng().gen_range(0..=max_delay_ms);
    Duration::from_millis(jitter_ms)
}

pub(super) fn sleep_with_exponential_jitter(
    base: Duration,
    cap: Duration,
    attempt: u32,
    deadline: Option<Instant>,
) -> bool {
    let delay = exponential_jitter_delay(base, cap, attempt);
    let Some(delay) = super::deadline::cap_wait(delay, deadline) else {
        return false;
    };
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::exponential_jitter_delay;
    use std::time::Duration;

    #[test]
    fn jitter_delay_stays_within_cap() {
        let base = Duration::from_millis(80);
        let cap = Duration::from_millis(600);
        for _ in 0..32 {
            let delay = exponential_jitter_delay(base, cap, 4);
            assert!(delay <= cap);
        }
    }
}
