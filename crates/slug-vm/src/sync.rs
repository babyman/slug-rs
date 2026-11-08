//! The reference counts the VM shares values with.
//!
//! A build with `threads` counts atomically, so values can be shared with
//! forks on other threads; a build without it, such as an embedded one, keeps
//! the cheaper single-threaded counts.

#[cfg(feature = "threads")]
pub(crate) use alloc::sync::{Arc as Rc, Weak};

#[cfg(not(feature = "threads"))]
pub(crate) use alloc::rc::{Rc, Weak};

/// A cached position in shared bytecode, such as a global's slot or a map
/// key's entry. Any thread may read or refresh it, and a stale one is only
/// a missed hint, so relaxed loads and stores suffice: on the targets the VM
/// runs on they are plain loads and stores, as a `Cell` would be.
#[derive(Debug, Default)]
pub(crate) struct Hint(core::sync::atomic::AtomicU32);

impl Hint {
    pub(crate) const fn new(value: u32) -> Self {
        Self(core::sync::atomic::AtomicU32::new(value))
    }

    #[inline]
    pub(crate) fn get(&self) -> u32 {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn set(&self, value: u32) {
        self.0.store(value, core::sync::atomic::Ordering::Relaxed);
    }
}

impl Clone for Hint {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}
