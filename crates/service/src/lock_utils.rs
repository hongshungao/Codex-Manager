use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Consistent lock-poison strategy for this crate:
/// - Recover via `into_inner()` (best-effort keep service running)
/// - Emit a warn log with a stable lock name for diagnostics.
pub(crate) fn lock_recover<'a, T>(mutex: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("event=lock_poisoned lock={} action=recover", name);
            poisoned.into_inner()
        }
    }
}

pub(crate) fn read_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("event=lock_poisoned lock={} action=recover_read", name);
            poisoned.into_inner()
        }
    }
}

pub(crate) fn write_recover<'a, T>(lock: &'a RwLock<T>, name: &str) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("event=lock_poisoned lock={} action=recover_write", name);
            poisoned.into_inner()
        }
    }
}

