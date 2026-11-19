//! Slug's desktop native-code tier.
//!
//! The tier compiles hot chunks to machine code through Cranelift; see
//! `docs/planning/native-tier.md` and
//! `docs/decisions/2026-11-18-desktop-native-tier-crate.md`. The tier lifts
//! each hot chunk into its IR ([`lift`]), checks it, and compiles it when
//! the baseline tier covers everything it does ([`compile`]).
//!
//! A tier configured to compile in the background does that on a thread
//! of its own: the chunk stays interpreted until its code is installed, so
//! compiling, about a microsecond for each Cranelift instruction, does not
//! stall the program.

use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::Instant,
};

use slug_vm::{HotChunk, NativeCode, NativeTier, Program};

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
    /// Compile on a thread of the tier's own, leaving a chunk interpreted
    /// until its code is ready, rather than at its hot entry.
    pub background: bool,
}

/// The threshold until measurements tune it.
pub const DEFAULT_THRESHOLD: u32 = 1_000;

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            trace: false,
            dump: false,
            background: false,
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
    /// Time spent lifting and compiling, in nanoseconds.
    compiling: AtomicU64,
    /// Hot chunks for the background compiler, once it runs.
    queue: OnceLock<Mutex<mpsc::Sender<HotChunk>>>,
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
            compiling: AtomicU64::new(0),
            queue: OnceLock::new(),
        }
    }

    /// Starts the background compiler for `tier`, which then compiles every
    /// chunk that becomes hot. Without a thread, chunks compile where they
    /// become hot.
    fn start_background(tier: &Arc<Self>) {
        let (sender, receiver) = mpsc::channel::<HotChunk>();
        let worker = Arc::clone(tier);
        let started = std::thread::Builder::new()
            .name("slug-native-compiler".into())
            .spawn(move || {
                for hot in receiver {
                    if let Some(code) = worker.compile_timed(hot.program(), hot.chunk()) {
                        let _ = hot.install(code);
                    }
                }
            });
        if started.is_ok() {
            let _ = tier.queue.set(Mutex::new(sender));
        }
    }

    /// Lifts and compiles chunk `chunk`, counting the time it takes.
    fn compile_timed(&self, program: &Program, chunk: usize) -> Option<Arc<dyn NativeCode>> {
        let name = program.chunk(chunk).map_or("?", |chunk| chunk.name());
        let started = Instant::now();
        let compiled = self.lift_and_compile(program, chunk, name);
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.compiling.fetch_add(elapsed, Ordering::Relaxed);
        compiled
    }

    /// What the tier did, for `SLUG_NATIVE_TRACE`.
    #[must_use]
    pub fn summary(&self) -> String {
        let count = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        #[allow(clippy::cast_precision_loss)] // Milliseconds to print.
        let compiling = count(&self.compiling) as f64 / 1e6;
        format!(
            "native: {} hot, {} compiled, {compiling:.1} ms lifting and compiling; \
             {} entries, {} activations left early, {} calls to functions without native code",
            self.hot_chunks(),
            self.compiled_chunks(),
            count(&runtime::stats::ENTRIES),
            count(&runtime::stats::EXITS),
            count(&runtime::stats::UNCOMPILED_CALLS),
        )
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

    fn hot(&self, hot: HotChunk) -> Option<Arc<dyn NativeCode>> {
        self.hot.fetch_add(1, Ordering::Relaxed);
        if self.config.trace {
            let name = hot
                .program()
                .chunk(hot.chunk())
                .map_or("?", |chunk| chunk.name());
            eprintln!("native: hot {name}");
        }
        let hot = match self.queue.get().map(Mutex::lock) {
            Some(Ok(queue)) => match queue.send(hot) {
                Ok(()) => return None,
                Err(mpsc::SendError(hot)) => hot,
            },
            _ => hot,
        };
        self.compile_timed(hot.program(), hot.chunk())
    }
}

impl Tier {
    fn lift_and_compile(
        &self,
        program: &Program,
        chunk: usize,
        name: &str,
    ) -> Option<Arc<dyn NativeCode>> {
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

static INSTALLED: OnceLock<Arc<Tier>> = OnceLock::new();

/// Installs a tier with `config` as this process's native tier. Returns
/// false if one was installed already.
#[must_use]
pub fn install(config: Config) -> bool {
    let tier = Arc::new(Tier::new(config));
    if config.background {
        Tier::start_background(&tier);
    }
    if !slug_vm::install_native_tier(tier.clone()) {
        return false;
    }
    runtime::stats::TRACING.store(config.trace, Ordering::Relaxed);
    let _ = INSTALLED.set(tier);
    true
}

/// The installed tier's [`Tier::summary`], if it was configured to trace.
#[must_use]
pub fn trace_summary() -> Option<String> {
    INSTALLED
        .get()
        .filter(|tier| tier.config.trace)
        .map(|tier| tier.summary())
}
