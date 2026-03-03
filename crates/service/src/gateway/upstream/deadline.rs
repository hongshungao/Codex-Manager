use std::time::{Duration, Instant};

fn effective_request_timeout(
    total_timeout: Option<Duration>,
    stream_timeout: Option<Duration>,
    is_stream: bool,
) -> Option<Duration> {
    if !is_stream {
        return total_timeout;
    }
    match (total_timeout, stream_timeout) {
        (Some(total_timeout), Some(stream_timeout)) => Some(total_timeout.max(stream_timeout)),
        (Some(total_timeout), None) => Some(total_timeout),
        (None, Some(stream_timeout)) => Some(stream_timeout),
        (None, None) => None,
    }
}

pub(super) fn request_deadline(started_at: Instant, is_stream: bool) -> Option<Instant> {
    let total_timeout = super::super::upstream_total_timeout();
    let stream_timeout = super::super::upstream_stream_timeout();
    effective_request_timeout(total_timeout, stream_timeout, is_stream)
        .map(|timeout| started_at + timeout)
}

pub(super) fn remaining(deadline: Option<Instant>) -> Option<Duration> {
    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
}

pub(super) fn is_expired(deadline: Option<Instant>) -> bool {
    remaining(deadline).is_some_and(|remaining| remaining.is_zero())
}

pub(super) fn cap_wait(wait: Duration, deadline: Option<Instant>) -> Option<Duration> {
    match remaining(deadline) {
        Some(remaining) if remaining.is_zero() => None,
        Some(remaining) => Some(wait.min(remaining)),
        None => Some(wait),
    }
}

pub(super) fn send_timeout(deadline: Option<Instant>, is_stream: bool) -> Option<Duration> {
    if is_stream {
        let configured = super::super::upstream_stream_timeout();
        return match (configured, remaining(deadline)) {
            (Some(configured), Some(remaining)) => Some(configured.min(remaining)),
            (Some(configured), None) => Some(configured),
            (None, Some(remaining)) => Some(remaining),
            (None, None) => None,
        }
        .map(|timeout| timeout.max(Duration::from_millis(1)));
    }
    remaining(deadline).map(|remaining| remaining.max(Duration::from_millis(1)))
}

#[cfg(test)]
#[path = "tests/deadline_tests.rs"]
mod tests;
