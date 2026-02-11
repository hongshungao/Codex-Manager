use std::thread;
use std::time::Duration;

pub(crate) const DEFAULT_USAGE_POLL_INTERVAL_SECS: u64 = 600;
pub(crate) const DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS: u64 = 180;
pub(crate) const MIN_USAGE_POLL_INTERVAL_SECS: u64 = 30;
pub(crate) const MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS: u64 = 30;

pub(crate) fn run_blocking_poll_loop<F, L>(
    loop_name: &str,
    interval: Duration,
    mut task: F,
    mut should_log_error: L,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
{
    run_blocking_poll_loop_with_sleep(loop_name, interval, &mut task, &mut should_log_error, |d| {
        thread::sleep(d);
        true
    });
}

pub(crate) fn run_blocking_poll_loop_with_sleep<F, L, S>(
    loop_name: &str,
    interval: Duration,
    task: &mut F,
    should_log_error: &mut L,
    mut sleep: S,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
    S: FnMut(Duration) -> bool,
{
    loop {
        if let Err(err) = task() {
            if should_log_error(err.as_str()) {
                log::warn!("{loop_name} error: {err}");
            }
        }
        if !sleep(interval) {
            break;
        }
    }
}

pub(crate) fn parse_interval_secs(raw: Option<&str>, default_secs: u64, min_secs: u64) -> u64 {
    // 中文注释：低于最小间隔会导致线程空转并放大上游压力；这里统一夹紧，避免配置误填把服务打满。
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .map(|secs| secs.max(min_secs))
        .unwrap_or(default_secs)
}

#[cfg(test)]
mod tests {
    use super::{parse_interval_secs, run_blocking_poll_loop_with_sleep};
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    #[test]
    fn blocking_poll_loop_runs_task_and_respects_interval() {
        let task_runs = Cell::new(0usize);
        let sleep_calls = RefCell::new(Vec::new());

        run_blocking_poll_loop_with_sleep(
            "test-loop",
            Duration::from_secs(3),
            &mut || {
                task_runs.set(task_runs.get() + 1);
                Ok(())
            },
            &mut |_| true,
            |duration| {
                sleep_calls.borrow_mut().push(duration);
                task_runs.get() < 3
            },
        );

        assert_eq!(task_runs.get(), 3);
        assert_eq!(sleep_calls.borrow().len(), 3);
        assert!(sleep_calls
            .borrow()
            .iter()
            .all(|d| *d == Duration::from_secs(3)));
    }

    #[test]
    fn blocking_poll_loop_calls_error_filter_before_sleep() {
        let checks = RefCell::new(Vec::new());
        let runs = Cell::new(0usize);

        run_blocking_poll_loop_with_sleep(
            "test-loop",
            Duration::from_secs(1),
            &mut || {
                runs.set(runs.get() + 1);
                if runs.get() == 1 {
                    Err("ignored".to_string())
                } else {
                    Err("fatal".to_string())
                }
            },
            &mut |err| {
                checks.borrow_mut().push(err.to_string());
                !err.contains("ignored")
            },
            |_| runs.get() < 2,
        );

        assert_eq!(runs.get(), 2);
        assert_eq!(
            checks.borrow().as_slice(),
            ["ignored".to_string(), "fatal".to_string()]
        );
    }

    #[test]
    fn parse_interval_secs_falls_back_and_applies_minimum() {
        assert_eq!(parse_interval_secs(None, 600, 30), 600);
        assert_eq!(parse_interval_secs(Some(""), 600, 30), 600);
        assert_eq!(parse_interval_secs(Some("abc"), 600, 30), 600);
        assert_eq!(parse_interval_secs(Some("5"), 600, 30), 30);
        assert_eq!(parse_interval_secs(Some("120"), 600, 30), 120);
    }
}
