use alloc::{
    rc::{Rc, Weak},
    sync::Arc,
    vec::Vec,
};
use core::cell::RefCell;

#[cfg(target_os = "none")]
use crate::scheduler_signal::ProgressSignal;
#[cfg(not(target_os = "none"))]
use crate::scheduler_signal::{BlockingProgressWaiter, ProgressSignal};
use crate::value::Channel;

/// Shared native-ingress and wake state. It intentionally has no knowledge of
/// Slug tasks, nurseries, or timer policy so a slim VM can own it directly.
pub(super) struct ProgressDriver {
    native_channels: RefCell<Vec<Weak<Channel>>>,
    signal: Arc<ProgressSignal>,
    #[cfg(not(target_os = "none"))]
    blocking_waiter: BlockingProgressWaiter,
}

impl ProgressDriver {
    pub(super) fn new() -> Self {
        let signal = Arc::new(ProgressSignal::new());
        #[cfg(not(target_os = "none"))]
        let blocking_waiter = signal.blocking_waiter();
        Self {
            native_channels: RefCell::new(Vec::new()),
            signal,
            #[cfg(not(target_os = "none"))]
            blocking_waiter,
        }
    }

    pub(super) fn clear(&self) {
        self.native_channels.borrow_mut().clear();
    }

    pub(super) fn track_native_channel(&self, channel: &Rc<Channel>) {
        if !channel.has_native_producer() {
            return;
        }
        channel.register_progress_signal(&self.signal);
        let mut channels = self.native_channels.borrow_mut();
        if !channels.iter().any(|candidate| {
            candidate
                .upgrade()
                .is_some_and(|candidate| Rc::ptr_eq(&candidate, channel))
        }) {
            channels.push(Rc::downgrade(channel));
        }
    }

    pub(super) fn drain_native_channels(&self) -> bool {
        let mut changed = false;
        for channel in self.native_channels() {
            changed |= channel.drain_native();
        }
        changed
    }

    /// Performs the shared, non-blocking portion of a host progress round.
    /// Scheduler task dispatch and timer policy deliberately live elsewhere.
    pub(super) fn make_available_progress(&self) -> bool {
        self.drain_native_channels()
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn has_live_native_source(&self) -> bool {
        self.native_channels()
            .iter()
            .any(|channel| channel.has_live_native_producer())
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn snapshot(&self) -> u64 {
        self.signal.snapshot()
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn wait(&self, observed: u64, timeout: Option<std::time::Duration>) {
        self.blocking_waiter.wait(&self.signal, observed, timeout);
    }

    /// Waits only when a live native producer can create future progress.
    #[cfg(all(not(target_os = "none"), not(feature = "concurrency")))]
    pub(super) fn wait_for_progress(&self) -> bool {
        if !self.has_live_native_source() {
            // A producer may have sent, closed, and released since the
            // caller's last drain; whatever it sent before closing is queued.
            return self.make_available_progress();
        }
        let observed = self.snapshot();
        if self.make_available_progress() {
            return true;
        }
        self.wait(observed, None);
        true
    }

    /// Freestanding hosts must return to their event loop rather than block.
    #[cfg(target_os = "none")]
    pub(super) fn wait_for_progress(&self) -> bool {
        false
    }

    fn native_channels(&self) -> Vec<Rc<Channel>> {
        let mut channels = self.native_channels.borrow_mut();
        channels.retain(|channel| channel.strong_count() > 0);
        channels.iter().filter_map(Weak::upgrade).collect()
    }
}
