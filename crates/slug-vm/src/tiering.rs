//! Hooks for a native-code tier, behind the `native-hooks` feature: a count
//! of entries per chunk, the tier the VM tells when a chunk becomes hot, and
//! the compiled code the VM then enters instead of interpreting.
//!
//! This is an internal contract with `slug-native-tier`, not part of the
//! native ABI; see `docs/decisions/2026-11-18-desktop-native-tier-crate.md`.

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use crate::{Program, Value, Vm};

/// A native-code tier. The VM counts each chunk's entries (calls and
/// `recur`s) and tells the tier once a chunk reaches its threshold.
#[doc(hidden)]
pub trait NativeTier: Send + Sync {
    /// Entries before a chunk is hot. Zero and one both mean the first.
    fn threshold(&self) -> u32;

    /// Chunk `chunk` of `program` became hot: the tier may compile it.
    /// Called at most once a chunk.
    fn hot(&self, program: &Program, chunk: usize) -> Option<Arc<dyn NativeCode>>;
}

/// A chunk compiled to native code.
#[doc(hidden)]
pub trait NativeCode: Send + Sync {
    /// Runs the chunk from its first instruction, in the active frame, whose
    /// function is `closure`, with that frame's `locals`.
    fn run(&self, vm: &mut Vm, closure: Value, locals: Vec<Value>) -> NativeOutcome;

    /// The tier's own type, for native code that calls this code directly.
    fn as_any(&self) -> &dyn core::any::Any;
}

/// How native code left its frame.
#[doc(hidden)]
pub enum NativeOutcome {
    /// The function returned this value.
    Return(Value),
    /// Native code stopped before instruction `ip`, leaving the frame's
    /// locals and its own operand-stack values as given, and above it the
    /// `frames` of the calls native code was making, outermost first; the
    /// interpreter continues in the innermost.
    Exit {
        ip: usize,
        locals: Vec<Value>,
        stack: Vec<Value>,
        frames: Vec<NativeFrame>,
    },
}

/// A call native code was making when it exited, as an interpreter frame:
/// `closure` stopped before instruction `ip`.
#[doc(hidden)]
pub struct NativeFrame {
    pub closure: Value,
    pub ip: usize,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
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
/// Compiled: entries run the native code.
const COMPILED: u32 = u32::MAX - 1;
/// Hot but not compiled, or no tier is installed: nothing more to count.
const SETTLED: u32 = u32::MAX;

/// A chunk's entries left before it is hot, and its native code once
/// compiled. Chunks are shared between threads; a racing decrement only
/// loses a count, so relaxed loads and stores suffice, as for
/// [`crate::sync::Hint`]. Only settling uses a compare-exchange, so the
/// tier hears of a chunk once.
pub(crate) struct NativeSlot {
    state: AtomicU32,
    code: OnceLock<Arc<dyn NativeCode>>,
    /// Entries into the native code from the interpreter, and how many of
    /// them it left again early, or declined.
    entries: AtomicU32,
    exits: AtomicU32,
}

/// Entries before a chunk's exits are judged.
const JUDGED_AFTER: u32 = 64;
/// Counts are halved here, so recent entries weigh most.
const DECAY_AT: u32 = 1 << 16;

impl NativeSlot {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU32::new(FRESH),
            code: OnceLock::new(),
            entries: AtomicU32::new(0),
            exits: AtomicU32::new(0),
        }
    }

    /// Records an entry into the native code from the interpreter, and
    /// whether it left early or was declined. A chunk that does so on most
    /// entries, such as a loop around a call native code cannot make,
    /// costs more entering and leaving than it saves: it settles, and stays
    /// interpreted from then on.
    #[inline]
    pub(crate) fn record_entry(&self, exited: bool) {
        let mut entries = self.entries.load(Ordering::Relaxed) + 1;
        let mut exits = self.exits.load(Ordering::Relaxed) + u32::from(exited);
        let decayed = entries >= DECAY_AT;
        if decayed {
            entries /= 2;
            exits /= 2;
        }
        self.entries.store(entries, Ordering::Relaxed);
        if exited || decayed {
            self.exits.store(exits, Ordering::Relaxed);
        }
        if exited && entries >= JUDGED_AFTER && exits * 2 > entries {
            self.state.store(SETTLED, Ordering::Relaxed);
        }
    }

    /// Counts one entry into chunk `chunk` of `program`, and returns its
    /// native code once it has some.
    #[inline]
    pub(crate) fn tick(&self, program: &Program, chunk: usize) -> Option<&Arc<dyn NativeCode>> {
        let left = self.state.load(Ordering::Relaxed);
        // One comparison for the common case, 2 ..= COMPILED - 1 left.
        if left.wrapping_sub(2) < COMPILED - 2 {
            self.state.store(left - 1, Ordering::Relaxed);
            None
        } else if left == COMPILED {
            self.code.get()
        } else if left == SETTLED {
            None
        } else {
            self.reached(program, chunk, left)
        }
    }

    #[cold]
    #[inline(never)]
    fn reached(&self, program: &Program, chunk: usize, left: u32) -> Option<&Arc<dyn NativeCode>> {
        let Some(tier) = TIER.get() else {
            self.state.store(SETTLED, Ordering::Relaxed);
            return None;
        };
        if left == FRESH {
            let threshold = tier.threshold();
            if threshold > 1 {
                self.state.store(threshold - 1, Ordering::Relaxed);
                return None;
            }
        }
        if self
            .state
            .compare_exchange(left, SETTLED, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        let code = tier.hot(program, chunk)?;
        let code = self.code.get_or_init(|| code);
        self.state.store(COMPILED, Ordering::Relaxed);
        Some(code)
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
            .debug_struct("NativeSlot")
            .field("state", &self.state.load(Ordering::Relaxed))
            .field("compiled", &self.code.get().is_some())
            .field("entries", &self.entries.load(Ordering::Relaxed))
            .field("exits", &self.exits.load(Ordering::Relaxed))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled() -> NativeSlot {
        let slot = NativeSlot::new();
        slot.state.store(COMPILED, Ordering::Relaxed);
        slot
    }

    #[test]
    fn a_chunk_that_mostly_exits_settles() {
        let slot = compiled();
        for _ in 0..JUDGED_AFTER - 1 {
            slot.record_entry(true);
        }
        assert_eq!(slot.state.load(Ordering::Relaxed), COMPILED);
        slot.record_entry(true);
        assert_eq!(slot.state.load(Ordering::Relaxed), SETTLED);
    }

    #[test]
    fn a_chunk_that_mostly_returns_stays_compiled() {
        let slot = compiled();
        for entry in 0..DECAY_AT * 3 {
            slot.record_entry(entry % 3 == 0);
        }
        assert_eq!(slot.state.load(Ordering::Relaxed), COMPILED);
        assert!(slot.entries.load(Ordering::Relaxed) < DECAY_AT);
    }
}
