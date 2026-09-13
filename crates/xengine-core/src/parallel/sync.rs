//! Cross-thread one-shot wait/notify primitive (`JobConditional`).
//!
//! Mirrors NeoX `JobConditional` (`notify_all` / `wait` / `reset`): a
//! notification that happens *before* a wait is not lost.

use std::sync::{Condvar, Mutex};

/// A one-shot, resettable condition usable from job payloads and the main
/// thread alike.
///
/// A `notify_all` before any `wait` is remembered: the next `wait` returns
/// immediately, which removes the "signal arrived too early" race that a bare
/// `Condvar` has.
#[derive(Debug, Default)]
pub struct JobConditional {
    notified: Mutex<bool>,
    cv: Condvar,
}

impl JobConditional {
    /// Creates a fresh, un-notified conditional.
    pub fn new() -> Self {
        Self {
            notified: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    /// Marks the conditional notified and wakes every current waiter.
    pub fn notify_all(&self) {
        let mut notified = self.notified.lock().unwrap_or_else(|e| e.into_inner());
        *notified = true;
        self.cv.notify_all();
    }

    /// Blocks until the conditional has been notified (returns immediately when
    /// it was already notified).
    pub fn wait(&self) {
        let mut notified = self.notified.lock().unwrap_or_else(|e| e.into_inner());
        while !*notified {
            notified = self.cv.wait(notified).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Clears the notification so the conditional can be reused.
    pub fn reset(&self) {
        let mut notified = self.notified.lock().unwrap_or_else(|e| e.into_inner());
        *notified = false;
    }

    /// Whether the conditional is currently notified.
    pub fn is_notified(&self) -> bool {
        *self.notified.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn notify_then_wait_returns_immediately() {
        let cond = JobConditional::new();
        assert!(!cond.is_notified());
        cond.notify_all();
        cond.wait();
        assert!(cond.is_notified());
    }

    #[test]
    fn wait_blocks_until_notified() {
        let cond = Arc::new(JobConditional::new());
        let waiter = {
            let cond = Arc::clone(&cond);
            thread::spawn(move || {
                cond.wait();
                cond.is_notified()
            })
        };
        thread::sleep(Duration::from_millis(20));
        assert!(!waiter.is_finished());
        cond.notify_all();
        assert!(waiter.join().expect("waiter thread"));
    }

    #[test]
    fn reset_allows_reuse() {
        let cond = JobConditional::new();
        cond.notify_all();
        cond.wait();
        cond.reset();
        assert!(!cond.is_notified());

        let cond = Arc::new(cond);
        let waiter = {
            let cond = Arc::clone(&cond);
            thread::spawn(move || {
                cond.wait();
            })
        };
        thread::sleep(Duration::from_millis(20));
        cond.notify_all();
        waiter.join().expect("second wait");
        assert!(cond.is_notified());
    }
}
