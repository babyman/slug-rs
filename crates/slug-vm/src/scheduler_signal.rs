use std::{
    sync::{Arc, Condvar, Mutex, Weak},
    time::Duration,
};

/// A generation-counted notification that an owner may be able to make VM
/// progress.  It deliberately carries no task identity and never executes
/// Slug code on the notifying thread.
pub(crate) struct ProgressSignal {
    generation: Mutex<u64>,
    blocking_waiters: Mutex<Vec<Weak<BlockingProgressState>>>,
}

struct BlockingProgressState {
    changed: Condvar,
    lock: Mutex<()>,
}

/// The blocking adapter for a [`ProgressSignal`].  The condition variable is
/// intentionally outside the shared notification object: hosts which drive
/// the VM themselves never need to wait on it.
pub(crate) struct BlockingProgressWaiter {
    state: Arc<BlockingProgressState>,
}

impl ProgressSignal {
    pub(crate) fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            blocking_waiters: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> u64 {
        *self
            .generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn notify(&self) {
        let mut generation = self
            .generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *generation = generation.wrapping_add(1);
        drop(generation);
        let waiters = {
            let mut waiters = self
                .blocking_waiters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            waiters.retain(|waiter| waiter.strong_count() > 0);
            waiters.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
        };
        for waiter in waiters {
            let _lock = waiter
                .lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            waiter.changed.notify_all();
        }
    }

    pub(crate) fn blocking_waiter(&self) -> BlockingProgressWaiter {
        let state = Arc::new(BlockingProgressState {
            changed: Condvar::new(),
            lock: Mutex::new(()),
        });
        self.blocking_waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::downgrade(&state));
        BlockingProgressWaiter { state }
    }
}

impl BlockingProgressWaiter {
    pub(crate) fn wait(&self, signal: &ProgressSignal, observed: u64, timeout: Option<Duration>) {
        if signal.snapshot() != observed {
            return;
        }
        let lock = self
            .state
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if signal.snapshot() != observed {
            return;
        }
        if let Some(timeout) = timeout {
            drop(
                self.state
                    .changed
                    .wait_timeout(lock, timeout)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        } else {
            drop(
                self.state
                    .changed
                    .wait(lock)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
}
