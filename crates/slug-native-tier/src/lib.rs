//! Slug's desktop native-code tier.
//!
//! The tier compiles hot chunks to machine code through Cranelift; see
//! `docs/planning/native-tier.md` and
//! `docs/decisions/2026-11-18-desktop-native-tier-crate.md`. Phase 1,
//! step 1: the tier hears which chunks become hot and compiles nothing, so
//! the VM's entry counts can be measured on their own.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use slug_vm::{NativeTier, Program};

/// How the host configured the tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Calls and `recur`s before a chunk is hot; 1 means its first entry.
    pub threshold: u32,
    /// Report each hot chunk on standard error.
    pub trace: bool,
}

/// The threshold until measurements tune it.
pub const DEFAULT_THRESHOLD: u32 = 1_000;

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            trace: false,
        }
    }
}

/// The native tier.
pub struct Tier {
    config: Config,
    hot: AtomicUsize,
}

impl Tier {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            hot: AtomicUsize::new(0),
        }
    }

    /// How many chunks have become hot.
    #[must_use]
    pub fn hot_chunks(&self) -> usize {
        self.hot.load(Ordering::Relaxed)
    }
}

impl NativeTier for Tier {
    fn threshold(&self) -> u32 {
        self.config.threshold
    }

    fn hot(&self, program: &Program, chunk: usize) {
        self.hot.fetch_add(1, Ordering::Relaxed);
        if self.config.trace {
            let name = program.chunk(chunk).map_or("?", |chunk| chunk.name());
            eprintln!("native: hot {name}");
        }
    }
}

/// Installs a tier with `config` as this process's native tier. Returns
/// false if one was installed already.
#[must_use]
pub fn install(config: Config) -> bool {
    slug_vm::install_native_tier(Arc::new(Tier::new(config)))
}
