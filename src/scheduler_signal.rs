use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

/// A generation-counted wake signal shared by a scheduler and event sources.
pub(crate) struct SchedulerSignal {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl SchedulerSignal {
    pub(crate) fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            changed: Condvar::new(),
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
        self.changed.notify_all();
    }

    pub(crate) fn wait(&self, observed: u64, timeout: Option<Duration>) {
        let generation = self
            .generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *generation != observed {
            return;
        }
        if let Some(timeout) = timeout {
            drop(
                self.changed
                    .wait_timeout_while(generation, timeout, |generation| *generation == observed)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        } else {
            drop(
                self.changed
                    .wait_while(generation, |generation| *generation == observed)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
}
