//! Slug's desktop native-code tier.
//!
//! The tier compiles hot chunks to machine code through Cranelift; see
//! `docs/planning/native-tier.md` and
//! `docs/decisions/2026-11-18-desktop-native-tier-crate.md`. So far (Phase
//! 1, step 2) the tier lifts each hot chunk into its IR and checks it, and
//! compiles nothing.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use slug_vm::{NativeTier, Program};

pub mod ir;
pub mod lift;

/// How the host configured the tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Calls and `recur`s before a chunk is hot; 1 means its first entry.
    pub threshold: u32,
    /// Report each hot chunk, and any it cannot lift, on standard error.
    pub trace: bool,
    /// Print each hot chunk's IR on standard error.
    pub dump: bool,
}

/// The threshold until measurements tune it.
pub const DEFAULT_THRESHOLD: u32 = 1_000;

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            trace: false,
            dump: false,
        }
    }
}

/// The native tier.
pub struct Tier {
    config: Config,
    hot: AtomicUsize,
    unlifted: AtomicUsize,
}

impl Tier {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            hot: AtomicUsize::new(0),
            unlifted: AtomicUsize::new(0),
        }
    }

    /// How many chunks have become hot.
    #[must_use]
    pub fn hot_chunks(&self) -> usize {
        self.hot.load(Ordering::Relaxed)
    }

    /// How many hot chunks could not be lifted.
    #[must_use]
    pub fn unlifted_chunks(&self) -> usize {
        self.unlifted.load(Ordering::Relaxed)
    }
}

impl NativeTier for Tier {
    fn threshold(&self) -> u32 {
        self.config.threshold
    }

    fn hot(&self, program: &Program, chunk: usize) {
        self.hot.fetch_add(1, Ordering::Relaxed);
        let name = program.chunk(chunk).map_or("?", |chunk| chunk.name());
        if self.config.trace {
            eprintln!("native: hot {name}");
        }
        let lifted = lift::lift(program, chunk).and_then(|function| {
            function.check()?;
            Ok(function)
        });
        match lifted {
            Ok(function) => {
                if self.config.dump {
                    eprint!("{function}");
                }
            }
            Err(message) => {
                self.unlifted.fetch_add(1, Ordering::Relaxed);
                if self.config.trace {
                    eprintln!("native: cannot lift {name}: {message}");
                }
            }
        }
    }
}

/// Installs a tier with `config` as this process's native tier. Returns
/// false if one was installed already.
#[must_use]
pub fn install(config: Config) -> bool {
    slug_vm::install_native_tier(Arc::new(Tier::new(config)))
}
