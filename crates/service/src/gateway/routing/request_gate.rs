use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use codexmanager_core::storage::now_ts;

const REQUEST_GATE_LOCK_TTL_SECS: i64 = 30 * 60;
const REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS: i64 = 60;

struct RequestGateLockEntry {
    lock: Arc<RequestGateLock>,
    last_seen_at: i64,
}

#[derive(Default)]
struct RequestGateLockTable {
    entries: HashMap<String, RequestGateLockEntry>,
    last_cleanup_at: i64,
}

static REQUEST_GATE_LOCKS: OnceLock<Mutex<RequestGateLockTable>> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum RequestGateAcquireError {
    Poisoned,
}

#[derive(Default)]
struct RequestGateState {
    held: bool,
}

pub(crate) struct RequestGateLock {
    state: Mutex<RequestGateState>,
    available: Condvar,
}

impl RequestGateLock {
    fn new() -> Self {
        Self {
            state: Mutex::new(RequestGateState::default()),
            available: Condvar::new(),
        }
    }

    pub(crate) fn try_acquire(
        self: &Arc<Self>,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        let mut state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        if state.held {
            return Ok(None);
        }
        state.held = true;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }

    pub(crate) fn acquire_with_timeout(
        self: &Arc<Self>,
        timeout: Duration,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        let state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        let wait_result = self
            .available
            .wait_timeout_while(state, timeout, |state| state.held);
        let Ok((mut state, _)) = wait_result else {
            log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait_timeout");
            return Err(RequestGateAcquireError::Poisoned);
        };
        if state.held {
            return Ok(None);
        }
        state.held = true;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }
}

pub(crate) struct RequestGateGuard {
    lock: Arc<RequestGateLock>,
}

impl Drop for RequestGateGuard {
    fn drop(&mut self) {
        let mut state = match self.lock.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=recover_release");
                poisoned.into_inner()
            }
        };
        state.held = false;
        self.lock.available.notify_one();
    }
}

fn gate_key(key_id: &str, path: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("-")
    )
}

pub(crate) fn request_gate_lock(key_id: &str, path: &str, model: Option<&str>) -> Arc<RequestGateLock> {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "request_gate_locks");
    let now = now_ts();
    maybe_cleanup_request_gate_locks(&mut table, now);
    let entry = table
        .entries
        .entry(gate_key(key_id, path, model))
        .or_insert_with(|| RequestGateLockEntry {
            lock: Arc::new(RequestGateLock::new()),
            last_seen_at: now,
        });
    entry.last_seen_at = now;
    entry.lock.clone()
}

fn maybe_cleanup_request_gate_locks(table: &mut RequestGateLockTable, now: i64) {
    if table.last_cleanup_at != 0
        && now.saturating_sub(table.last_cleanup_at) < REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    table.last_cleanup_at = now;
    table.entries.retain(|_, entry| {
        let stale = now.saturating_sub(entry.last_seen_at) > REQUEST_GATE_LOCK_TTL_SECS;
        !stale || Arc::strong_count(&entry.lock) > 1
    });
}

#[cfg(test)]
fn clear_request_gate_locks_for_tests() {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    if let Ok(mut table) = lock.lock() {
        table.entries.clear();
        table.last_cleanup_at = 0;
    }
}

#[cfg(test)]
fn request_gate_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static REQUEST_GATE_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    REQUEST_GATE_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("request gate test mutex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_scope_reuses_same_lock_instance() {
        let _guard = request_gate_test_guard();
        clear_request_gate_locks_for_tests();
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn different_scope_uses_different_lock_instances() {
        let _guard = request_gate_test_guard();
        clear_request_gate_locks_for_tests();
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex-high"));
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn stale_unshared_lock_entry_is_reclaimed() {
        let _guard = request_gate_test_guard();
        clear_request_gate_locks_for_tests();
        let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let weak = Arc::downgrade(&first);
        drop(first);

        let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
        let mut table = lock.lock().expect("request gate table lock");
        let now = now_ts();
        table
            .entries
            .get_mut(&key)
            .expect("request gate entry")
            .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
        table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
        drop(table);

        let _second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn stale_shared_lock_entry_is_not_reclaimed() {
        let _guard = request_gate_test_guard();
        clear_request_gate_locks_for_tests();
        let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));

        let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
        let mut table = lock.lock().expect("request gate table lock");
        let now = now_ts();
        table
            .entries
            .get_mut(&key)
            .expect("request gate entry")
            .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
        table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
        drop(table);

        let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        assert!(Arc::ptr_eq(&first, &second));
    }
}
