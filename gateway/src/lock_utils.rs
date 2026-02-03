//! Safe lock utilities that handle poisoned locks gracefully
//!
//! In Rust, when a thread panics while holding a lock, the lock becomes "poisoned".
//! These utilities recover from poisoned locks by extracting the inner data,
//! allowing the application to continue operating instead of cascading panics.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Safely acquire a read lock, recovering from poison if necessary
///
/// If the lock was poisoned (a thread panicked while holding it),
/// this function logs a warning and recovers the inner data.
pub fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| {
        tracing::warn!("RwLock was poisoned during read access, recovering");
        poisoned.into_inner()
    })
}

/// Safely acquire a write lock, recovering from poison if necessary
///
/// If the lock was poisoned (a thread panicked while holding it),
/// this function logs a warning and recovers the inner data.
pub fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|poisoned| {
        tracing::warn!("RwLock was poisoned during write access, recovering");
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_read_lock_normal() {
        let lock = RwLock::new(42);
        let guard = read_lock(&lock);
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_write_lock_normal() {
        let lock = RwLock::new(42);
        {
            let mut guard = write_lock(&lock);
            *guard = 100;
        }
        let guard = read_lock(&lock);
        assert_eq!(*guard, 100);
    }

    #[test]
    fn test_poisoned_lock_recovery() {
        let lock = Arc::new(RwLock::new(42));
        let lock_clone = Arc::clone(&lock);

        // Spawn a thread that will panic while holding the write lock
        let handle = thread::spawn(move || {
            let _guard = lock_clone.write().unwrap();
            panic!("intentional panic to poison the lock");
        });

        // Wait for the thread to finish (it will panic)
        let _ = handle.join();

        // The lock should now be poisoned, but we should still be able to read from it
        let guard = read_lock(&lock);
        assert_eq!(*guard, 42);
    }
}
