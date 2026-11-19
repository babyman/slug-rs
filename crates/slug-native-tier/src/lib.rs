//! Slug's desktop native-code tier.
//!
//! The tier compiles hot chunks to machine code through Cranelift; see
//! `docs/planning/native-tier.md` and
//! `docs/decisions/2026-11-18-desktop-native-tier-crate.md`. The tier lifts
//! each hot chunk into its IR ([`lift`]), checks it, and compiles it when
//! the baseline tier covers everything it does ([`compile`]).

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use slug_vm::{NativeCode, NativeTier, Program};

pub mod compile;
pub mod ir;
mod jit;
pub mod lift;
mod runtime;

/// How the host configured the tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Calls and `recur`s before a chunk is hot; 1 means its first entry.
    pub threshold: u32,
    /// Report each hot chunk, and any it cannot lift, on standard error.
    pub trace: bool,
    /// Print each hot chunk's IR, and the machine code of each it
    /// compiles, on standard error.
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
    /// Cranelift's module, unless this machine or build cannot use it.
    jit: Option<jit::Jit>,
    hot: AtomicUsize,
    unlifted: AtomicUsize,
    compiled: AtomicUsize,
}

impl Tier {
    #[must_use]
    pub fn new(config: Config) -> Self {
        let jit = if runtime::layout_matches() {
            jit::Jit::new()
                .map_err(|message| {
                    if config.trace {
                        eprintln!("native: no code generation: {message}");
                    }
                })
                .ok()
        } else {
            if config.trace {
                eprintln!("native: no code generation: `Value`'s layout is not the one expected");
            }
            None
        };
        Self {
            config,
            jit,
            hot: AtomicUsize::new(0),
            unlifted: AtomicUsize::new(0),
            compiled: AtomicUsize::new(0),
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

    /// How many hot chunks were compiled.
    #[must_use]
    pub fn compiled_chunks(&self) -> usize {
        self.compiled.load(Ordering::Relaxed)
    }
}

impl NativeTier for Tier {
    fn threshold(&self) -> u32 {
        self.config.threshold
    }

    fn hot(&self, program: &Program, chunk: usize) -> Option<Arc<dyn NativeCode>> {
        self.hot.fetch_add(1, Ordering::Relaxed);
        let name = program.chunk(chunk).map_or("?", |chunk| chunk.name());
        if self.config.trace {
            eprintln!("native: hot {name}");
        }
        let lifted = lift::lift(program, chunk).and_then(|function| {
            function.check()?;
            Ok(function)
        });
        let function = match lifted {
            Ok(function) => function,
            Err(message) => {
                self.unlifted.fetch_add(1, Ordering::Relaxed);
                if self.config.trace {
                    eprintln!("native: cannot lift {name}: {message}");
                }
                return None;
            }
        };
        if self.config.dump {
            eprint!("{function}");
        }
        let jit = self.jit.as_ref()?;
        // A bug in code generation leaves the chunk interpreted rather than
        // ending the program.
        let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compile::compile(jit, &function, self.config.dump)
        }))
        .unwrap_or_else(|_| Err("code generation panicked".into()));
        match compiled {
            Ok(compiled) => {
                self.compiled.fetch_add(1, Ordering::Relaxed);
                if self.config.trace {
                    eprintln!("native: compiled {name}");
                }
                Some(Arc::new(compiled))
            }
            Err(message) => {
                if self.config.trace {
                    eprintln!("native: not compiled {name}: {message}");
                }
                None
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
