//! Hooks for a native-code tier, behind the `native-hooks` feature: a count
//! of entries per chunk, and the tier the VM tells when a chunk becomes hot.
//!
//! This is an internal contract with `slug-native-tier`, not part of the
//! native ABI; see `docs/decisions/2026-11-18-desktop-native-tier-crate.md`.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use crate::Program;

/// A native-code tier. The VM counts each chunk's entries (calls and
/// `recur`s) and tells the tier once a chunk reaches its threshold.
#[doc(hidden)]
pub trait NativeTier: Send + Sync {
    /// Entries before a chunk is hot. Zero and one both mean the first.
    fn threshold(&self) -> u32;

    /// Chunk `chunk` of `program` became hot. Called at most once a chunk.
    fn hot(&self, program: &Program, chunk: usize);
}

static TIER: OnceLock<Arc<dyn NativeTier>> = OnceLock::new();

/// Installs this process's native tier, before any program runs. Returns
/// false, and changes nothing, if a tier was installed already.
#[doc(hidden)]
pub fn install_native_tier(tier: Arc<dyn NativeTier>) -> bool {
    TIER.set(tier).is_ok()
}

/// Not counted yet: the first entry reads the tier's threshold.
const FRESH: u32 = 0;
/// Hot already, or no tier is installed: nothing more to count.
const SETTLED: u32 = u32::MAX;

/// A chunk's entries left before it is hot. Chunks are shared between
/// threads; a racing decrement only loses a count, so relaxed loads and
/// stores suffice, as for [`crate::sync::Hint`]. Only settling uses a
/// compare-exchange, so the tier hears of a chunk once.
pub(crate) struct NativeSlot(AtomicU32);

impl NativeSlot {
    pub(crate) const fn new() -> Self {
        Self(AtomicU32::new(FRESH))
    }

    /// Counts one entry into chunk `chunk` of `program`.
    #[inline]
    pub(crate) fn tick(&self, program: &Program, chunk: usize) {
        let left = self.0.load(Ordering::Relaxed);
        // One comparison for the common case, 2 ..= SETTLED - 1 left.
        if left.wrapping_sub(2) < SETTLED - 2 {
            self.0.store(left - 1, Ordering::Relaxed);
        } else if left != SETTLED {
            self.reached(program, chunk, left);
        }
    }

    #[cold]
    #[inline(never)]
    fn reached(&self, program: &Program, chunk: usize, left: u32) {
        let Some(tier) = TIER.get() else {
            self.0.store(SETTLED, Ordering::Relaxed);
            return;
        };
        if left == FRESH {
            let threshold = tier.threshold();
            if threshold > 1 {
                self.0.store(threshold - 1, Ordering::Relaxed);
                return;
            }
        }
        if self
            .0
            .compare_exchange(left, SETTLED, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            tier.hot(program, chunk);
        }
    }
}

impl Clone for NativeSlot {
    /// A copied chunk starts counting again.
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for NativeSlot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_tuple("NativeSlot")
            .field(&self.0.load(Ordering::Relaxed))
            .finish()
    }
}
