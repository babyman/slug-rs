use std::{cell::RefCell, collections::HashSet, path::Path, rc::Rc};

#[cfg(feature = "concurrency")]
use std::cell::Cell;
#[cfg(any(feature = "concurrency", feature = "metrics"))]
use std::time::Duration;
#[cfg(any(feature = "concurrency", feature = "metrics"))]
use std::time::Instant;

use crate::source::environment::CallableIdentity;
use crate::source::{InteractiveCompilation, InteractiveCompilerState};
#[cfg(feature = "concurrency")]
use crate::value::Task;
#[cfg(feature = "concurrency")]
use crate::value::TaskAdmission;
use crate::{
    CallArgumentKind, Capture, ModuleDeclaration, ModuleLoader, NativeDescriptorError,
    NativeFunction, Program, SourceSpan, SpanId, Value,
    bytecode::{EntrypointArguments, Op, PackedInstruction, PackedOpcode, SelectCase},
    collections::{List, Map},
    native::{NativeInvocation, NativeResourceRegistry, native_resource_registry},
    value::{
        Builtin, Channel, ChannelReceive, ChannelSend, Closure, GlobalEnvironment, RootWaiter,
        SelectWake, WaitRegistration, WaitSet, Waiter, binding_cell, global_environment,
        module_binding,
    },
};

#[cfg(feature = "concurrency")]
use crate::value::task_state_layout;

mod calls;
mod cleanup;
mod error;
mod frames;
mod operations;
mod progress;
#[cfg(feature = "concurrency")]
mod scheduler;
mod stack;
#[cfg(feature = "concurrency")]
pub(crate) mod timers;

#[cfg(feature = "concurrency")]
use calls::ClosureCallOptions;
use calls::{CallableRuntimeSignature, ExpandedCallArguments, NamedArgument};
use cleanup::{Cleanup, Deferred};
use error::render_stacktrace;
pub use error::{CallFrame, NativeErrorDetails, RuntimeError, RuntimeErrorKind};
use frames::{CallSpan, Frame, LocalSlot, ProvidedArguments, frame_locals};
use operations::{
    add, bit_not, bitwise, construct_struct, copy_value, divide, index_value, is_map_key,
    list_append, list_prepend, matches_pattern, modulo, multiply, negate, numbers, shift,
    slice_value, subtract,
};
use progress::ProgressDriver;
#[cfg(feature = "concurrency")]
use scheduler::Nursery;

pub type VmResult<T> = Result<T, RuntimeError>;

/// The observable result of one host-driven VM progress turn.
#[derive(Clone, Debug)]
pub enum VmProgress {
    MadeProgress,
    Stalled,
    Completed(Value),
    Failed(RuntimeError),
}

struct HostExecution {
    root: RootWaiter,
    result: Option<VmResult<Value>>,
}

/// Deterministic Rust-layout measurements for the VM's hot data structures.
///
/// These sizes exclude heap allocations owned by vectors, reference counts,
/// and host allocator bookkeeping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmLayoutMetrics {
    pub value_size_bytes: usize,
    pub value_alignment_bytes: usize,
    pub local_slot_size_bytes: usize,
    pub local_slot_alignment_bytes: usize,
    pub frame_size_bytes: usize,
    pub frame_alignment_bytes: usize,
    pub closure_size_bytes: usize,
    pub closure_alignment_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_size_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_alignment_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_state_size_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_state_alignment_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_execution_size_bytes: usize,
    #[cfg(feature = "concurrency")]
    pub task_execution_alignment_bytes: usize,
    pub instruction_size_bytes: usize,
    pub instruction_alignment_bytes: usize,
}

/// Execution counters for one public VM invocation.
///
/// The counters describe private representation costs, not source semantics.
/// They are available only with the opt-in `metrics` feature and are not a
/// profiling or compatibility API.
#[cfg(feature = "metrics")]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VmMetrics {
    /// Instructions fetched by the dispatch loop, including spawned tasks.
    pub instructions_executed: usize,
    /// Whole instructions cloned while fetching them for dispatch.
    pub instruction_clones: usize,
    /// Source spans cloned because execution needs an owned diagnostic or state.
    pub source_span_clones: usize,
    /// Source-table entries resolved because execution needs an owned span.
    pub source_span_lookups: usize,
    /// Frames allocated for the root invocation, calls, and spawned tasks.
    pub frames_created: usize,
    /// Frame-local binding cells allocated by the current representation.
    pub local_binding_cells_created: usize,
    /// Frame-local vectors constructed while entering or restarting frames.
    pub frame_local_vectors_created: usize,
    /// Total capacity reserved by newly constructed frame-local vectors.
    pub frame_local_capacity_total: usize,
    /// Argument values written into frame-local slots.
    pub argument_values_copied_to_locals: usize,
    /// Temporary argument vectors constructed before ordinary closure entry.
    pub closure_argument_vectors_created: usize,
    /// Exact positional argument values initialized directly from operand-stack slots.
    pub exact_positional_stack_local_initializations: usize,
    /// Non-uniform provided-argument bitmaps retained by frames.
    pub provided_argument_bitmaps_created: usize,
    /// Total capacity retained by non-uniform provided-argument bitmaps.
    pub provided_argument_bitmap_capacity_total: usize,
    /// Scope stacks materialized only after a frame registers a deferred action.
    pub defer_scope_stacks_created: usize,
    /// Scope entries initialized when lazy defer storage is materialized.
    pub defer_scope_entries_materialized: usize,
    /// `recur` restarts that reused an all-direct frame-local vector.
    pub recur_local_vectors_reused: usize,
    /// `recur` restarts that replaced locals to preserve captured-cell identity.
    pub recur_local_vectors_replaced: usize,
    /// Exact positional `recur` restarts that bypassed generic argument binding.
    pub exact_positional_recur_restarts: usize,
    /// `recur` restarts that entered generic argument binding.
    pub generic_recur_argument_bindings: usize,
    /// Exact positional closure calls that bypassed generic argument binding.
    pub exact_positional_closure_calls: usize,
    /// Calls that entered generic argument expansion and binding.
    pub generic_call_argument_bindings: usize,
    /// Lists, maps, bytes, and structs constructed by VM collection operations.
    pub collection_constructions: usize,
    /// Elements or fields supplied while constructing collection values.
    pub collection_elements_constructed: usize,
    /// Collection index operations, including map lookups.
    pub collection_lookups: usize,
    /// Slice operations over lists, bytes, or strings.
    pub collection_slices: usize,
    /// Map entries inspected by the current lookup representation.
    pub map_entries_examined: usize,
    /// Persistent collection update operations.
    pub collection_updates: usize,
    /// Elements or fields copied while producing a persistent update.
    pub collection_elements_copied: usize,
    /// Updates whose source collection had one reference-counted owner.
    pub collection_unique_owner_updates: usize,
    /// Updates whose source collection was shared by another value.
    pub collection_shared_owner_updates: usize,
    /// Timed waits registered with nursery timer services.
    #[cfg(feature = "concurrency")]
    pub timer_registrations: usize,
    /// Timer-service scans for the next deadline.
    #[cfg(feature = "concurrency")]
    pub timer_deadline_lookups: usize,
    /// Waiters resumed after their timer deadline became due.
    #[cfg(feature = "concurrency")]
    pub timer_wakeups: usize,
    /// Wait registrations removed when a select settles or a task is cancelled.
    pub wait_registration_removals: usize,
    /// Timer entries examined while finding the next deadline.
    #[cfg(feature = "concurrency")]
    pub timer_deadline_entries_examined: usize,
    /// Timer entries examined while waking due waiters.
    #[cfg(feature = "concurrency")]
    pub timer_wakeup_entries_examined: usize,
    /// Channel waiter entries examined while removing registrations.
    pub channel_waiter_entries_examined: usize,
    /// Task waiter entries examined while removing registrations.
    #[cfg(feature = "concurrency")]
    pub task_waiter_entries_examined: usize,
    /// Indexed timer-registration lookups while removing registrations.
    #[cfg(feature = "concurrency")]
    pub timer_waiter_entries_examined: usize,
    /// Largest timer queue depth in the invocation.
    #[cfg(feature = "concurrency")]
    pub peak_timer_waiters: usize,
    /// Largest ready-queue depth in the invocation.
    #[cfg(feature = "concurrency")]
    pub peak_ready_queue: usize,
    /// Largest channel waiter queue observed during registration removal.
    pub peak_channel_waiters: usize,
    /// Largest task waiter queue observed during registration removal.
    #[cfg(feature = "concurrency")]
    pub peak_task_waiters: usize,
    /// Time spent blocked in the scheduler signal wait.
    #[cfg(feature = "concurrency")]
    pub scheduler_wait_time: Duration,
    /// Time spent structurally validating private bytecode.
    pub verification_time: Duration,
    /// Structural bytecode validations performed while installing programs.
    pub program_validations: usize,
    /// Whole programs cloned to establish an installed execution owner.
    pub program_clones: usize,
    /// Estimated inline bytecode bytes copied by whole-program clones.
    pub program_clone_bytes: usize,
}

/// Immutable, checked bytecode installed by a [`Vm`].
///
/// A program is installed for one root entry. The VM validates that entry and
/// takes ownership of the mutable [`Program`] before this value is created.
/// Reusing an `InstalledProgram` therefore does not clone or revalidate its
/// bytecode. It is an in-process execution object, not a portable artifact.
#[derive(Clone, Debug)]
pub struct InstalledProgram {
    program: Rc<Program>,
    entry: usize,
}

impl InstalledProgram {
    /// Returns the immutable bytecode retained by this installed program.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Returns the root chunk validated for this installed program.
    #[must_use]
    pub const fn entry(&self) -> usize {
        self.entry
    }
}

/// The independently owned interpreter state for a spawned task.
///
/// It currently runs to settlement, but keeping the VM intact makes future
/// blocking operations able to return it to the scheduler without rebuilding
/// frames, locals, or the operand stack.
#[cfg(feature = "concurrency")]
pub(crate) struct TaskExecution {
    vm: Vm,
    settle_nursery: bool,
    interactive_imported_globals: Option<Rc<RefCell<HashSet<String>>>>,
}

#[cfg(feature = "concurrency")]
enum TaskRunOutcome {
    Settled(VmResult<Value>),
    Suspended(Box<TaskExecution>),
}

enum ExecutionOutcome {
    Settled(VmResult<Value>),
    Suspended,
}

enum BorrowedSpanOpOutcome {
    Continue,
    Settled(Value),
}

#[derive(Clone)]
enum Suspension {
    Select {
        #[cfg(feature = "concurrency")]
        span: Option<SourceSpan>,
    },
}

enum RuntimeSelectCase {
    Receive {
        channel: Rc<Channel>,
        handler: Option<Value>,
    },
    Send {
        channel: Rc<Channel>,
        value: Value,
        handler: Option<Value>,
    },
    #[cfg(feature = "concurrency")]
    After {
        deadline: Instant,
        handler: Option<Value>,
    },
    #[cfg(feature = "concurrency")]
    Await {
        task: Rc<Task>,
        handler: Option<Value>,
    },
    Default {
        handler: Option<Value>,
    },
}

#[cfg(feature = "concurrency")]
impl TaskExecution {
    fn run(mut self) -> TaskRunOutcome {
        match self.vm.execute() {
            ExecutionOutcome::Suspended => {
                self.sync_interactive_imports();
                TaskRunOutcome::Suspended(Box::new(self))
            }
            ExecutionOutcome::Settled(result) => {
                self.sync_interactive_imports();
                let result = if self.settle_nursery {
                    self.vm.settle_tasks(&result)
                } else {
                    result
                };
                TaskRunOutcome::Settled(result)
            }
        }
    }

    fn sync_interactive_imports(&self) {
        if let Some(imported_globals) = &self.interactive_imported_globals {
            imported_globals
                .borrow_mut()
                .clone_from(&self.vm.imported_globals);
        }
    }

    pub(crate) fn set_current_task(&mut self, task: &Rc<Task>) {
        self.vm.current_waiter = Some(Waiter::task(task));
    }

    pub(crate) fn resume(&mut self, result: VmResult<Value>) {
        self.vm.resume = Some(result);
    }

    pub(crate) fn reject_closed_send(&mut self) {
        let span = match &self.vm.suspension {
            Some(Suspension::Select { span }) => span.clone(),
            _ => None,
        };
        self.vm.resume = Some(Err(self.vm.error(
            RuntimeErrorKind::InvalidCall,
            "send on a closed channel".into(),
            span,
        )));
    }

    pub(crate) fn take_wait_registration(&mut self) -> Option<WaitSet> {
        self.vm.wait_registration.take()
    }
}

/// A small, checked stack VM for compiler-produced Slug bytecode.
pub struct Vm {
    module_loader: Option<ModuleLoader>,
    module_program: Option<Rc<Program>>,
    globals: GlobalEnvironment,
    imported_globals: HashSet<String>,
    module_metadata: Vec<ModuleDeclaration>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    cleanup: Vec<Cleanup>,
    progress: Rc<ProgressDriver>,
    #[cfg(feature = "concurrency")]
    nursery: Rc<Nursery>,
    #[cfg(feature = "concurrency")]
    direct_task_limit: Option<usize>,
    #[cfg(feature = "concurrency")]
    direct_task_count: Option<Rc<Cell<usize>>>,
    native_resources: NativeResourceRegistry,
    current_waiter: Option<Waiter>,
    suspension: Option<Suspension>,
    resume: Option<VmResult<Value>>,
    wait_registration: Option<WaitSet>,
    host_execution: Option<HostExecution>,
    active_span: Option<SpanId>,
    shutdown: bool,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

/// Global bindings retained by one interactive session using a shared VM.
#[doc(hidden)]
pub struct InteractiveEnvironment {
    globals: GlobalEnvironment,
    host_globals: GlobalEnvironment,
    local_bindings: HashSet<String>,
    imported_globals: HashSet<String>,
}

/// Detached root-execution state for a slim interactive session.
#[cfg(not(feature = "concurrency"))]
#[doc(hidden)]
pub struct InteractiveExecution {
    module_program: Option<Rc<Program>>,
    imported_globals: HashSet<String>,
    module_metadata: Vec<ModuleDeclaration>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    cleanup: Vec<Cleanup>,
    progress: Rc<ProgressDriver>,
    current_waiter: Option<Waiter>,
    suspension: Option<Suspension>,
    resume: Option<VmResult<Value>>,
    wait_registration: Option<WaitSet>,
    host_execution: Option<HostExecution>,
    active_span: Option<SpanId>,
}

/// A scheduler-owned interactive submission running in the shared VM.
#[cfg(feature = "concurrency")]
#[derive(Clone)]
#[doc(hidden)]
pub struct InteractiveTask {
    task: Rc<Task>,
    nursery: Rc<Nursery>,
    globals: GlobalEnvironment,
    imported_globals: Rc<RefCell<HashSet<String>>>,
}

#[cfg(feature = "concurrency")]
impl InteractiveTask {
    #[doc(hidden)]
    #[must_use]
    pub fn outcome(&self) -> Option<VmResult<Value>> {
        self.task.outcome()
    }

    #[doc(hidden)]
    pub fn synchronize_environment(
        &self,
        environment: &mut InteractiveEnvironment,
        program: &Program,
    ) {
        let task_globals = self.globals.borrow();
        let mut session_globals = environment.globals.borrow_mut();
        for name in program.bindings() {
            if let Some(value) = task_globals.get(name) {
                let value = Self::rebind_interactive_value(
                    value.clone(),
                    &self.globals,
                    &environment.globals,
                );
                let mutable = program
                    .declarations()
                    .iter()
                    .any(|declaration| declaration.mutable && declaration.bindings.contains(name));
                if mutable {
                    if !session_globals
                        .get(name)
                        .is_some_and(|binding| binding.replace_binding(value.clone()))
                    {
                        session_globals.insert(
                            name.clone(),
                            Value::Binding {
                                name: name.clone().into(),
                                cell: binding_cell(value),
                            },
                        );
                    }
                } else {
                    session_globals.insert(name.clone(), value);
                }
            }
        }
        for name in self.imported_globals.borrow().iter() {
            if let Some(value) = task_globals.get(name) {
                session_globals.insert(name.clone(), value.clone());
            }
        }
        drop(session_globals);
        environment
            .imported_globals
            .clone_from(&self.imported_globals.borrow());
    }

    fn rebind_interactive_value(
        value: Value,
        task_globals: &GlobalEnvironment,
        session_globals: &GlobalEnvironment,
    ) -> Value {
        match value {
            Value::Closure(closure)
                if closure
                    .globals
                    .as_ref()
                    .is_some_and(|globals| Rc::ptr_eq(globals, task_globals)) =>
            {
                Value::Closure(Rc::new(Closure {
                    chunk: closure.chunk,
                    captures: closure.captures.clone(),
                    program: closure.program.clone(),
                    globals: Some(session_globals.clone()),
                    #[cfg(feature = "concurrency")]
                    capture_sources: closure.capture_sources.clone(),
                }))
            }
            Value::Overloads(values) => Value::Overloads(Rc::new(
                values
                    .iter()
                    .cloned()
                    .map(|value| {
                        Self::rebind_interactive_value(value, task_globals, session_globals)
                    })
                    .collect(),
            )),
            value => value,
        }
    }
}

impl Default for Vm {
    fn default() -> Self {
        #[cfg(feature = "metrics")]
        let metrics = Rc::new(RefCell::new(VmMetrics::default()));
        let progress = Rc::new(ProgressDriver::new());
        Self {
            module_loader: None,
            module_program: None,
            globals: global_environment(),
            imported_globals: HashSet::new(),
            module_metadata: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            cleanup: Vec::new(),
            progress: progress.clone(),
            #[cfg(feature = "concurrency")]
            nursery: Rc::new(Nursery::root(
                progress,
                #[cfg(feature = "metrics")]
                metrics.clone(),
            )),
            #[cfg(feature = "concurrency")]
            direct_task_limit: None,
            #[cfg(feature = "concurrency")]
            direct_task_count: None,
            native_resources: native_resource_registry(),
            current_waiter: None,
            suspension: None,
            resume: None,
            wait_registration: None,
            host_execution: None,
            active_span: None,
            shutdown: false,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }
}

impl Vm {
    #[must_use]
    pub fn new() -> Self {
        let mut vm = Self::default();
        vm.install_configuration_builtins();
        vm
    }

    /// Returns deterministic Rust-layout measurements for VM runtime state.
    #[must_use]
    pub fn layout_metrics() -> VmLayoutMetrics {
        #[cfg(feature = "concurrency")]
        let (task_state_size_bytes, task_state_alignment_bytes) = task_state_layout();
        VmLayoutMetrics {
            value_size_bytes: std::mem::size_of::<Value>(),
            value_alignment_bytes: std::mem::align_of::<Value>(),
            local_slot_size_bytes: std::mem::size_of::<LocalSlot>(),
            local_slot_alignment_bytes: std::mem::align_of::<LocalSlot>(),
            frame_size_bytes: std::mem::size_of::<Frame>(),
            frame_alignment_bytes: std::mem::align_of::<Frame>(),
            closure_size_bytes: std::mem::size_of::<Closure>(),
            closure_alignment_bytes: std::mem::align_of::<Closure>(),
            #[cfg(feature = "concurrency")]
            task_size_bytes: std::mem::size_of::<Task>(),
            #[cfg(feature = "concurrency")]
            task_alignment_bytes: std::mem::align_of::<Task>(),
            #[cfg(feature = "concurrency")]
            task_state_size_bytes,
            #[cfg(feature = "concurrency")]
            task_state_alignment_bytes,
            #[cfg(feature = "concurrency")]
            task_execution_size_bytes: std::mem::size_of::<TaskExecution>(),
            #[cfg(feature = "concurrency")]
            task_execution_alignment_bytes: std::mem::align_of::<TaskExecution>(),
            instruction_size_bytes: std::mem::size_of::<crate::bytecode::PackedInstruction>(),
            instruction_alignment_bytes: std::mem::align_of::<crate::Instruction>(),
        }
    }

    #[must_use]
    pub fn with_module_loader(module_loader: ModuleLoader) -> Self {
        let native_resources = module_loader.native_resources();
        let mut vm = Self {
            module_loader: Some(module_loader),
            native_resources,
            ..Self::default()
        };
        vm.install_configuration_builtins();
        vm
    }

    #[doc(hidden)]
    pub fn compile_interactive_forms(
        &self,
        path: &str,
        source: &str,
        state: &InteractiveCompilerState,
    ) -> Result<Vec<InteractiveCompilation>, crate::SourceError> {
        self.module_loader.as_ref().map_or_else(
            || crate::source::compile_interactive_forms(path, source, state),
            |loader| loader.compile_interactive_forms(path, source, state),
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub fn has_module_loader(&self) -> bool {
        self.module_loader.is_some()
    }

    /// Stops this VM and releases clutch-owned runtime state.
    ///
    /// Further execution attempts return a checked runtime error. Hosts sharing
    /// a module loader must shut down all dependent VMs before this call.
    pub fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.shutdown = true;
        if let Some(execution) = self.host_execution.take() {
            let cancellation = self.error(
                RuntimeErrorKind::InvalidCall,
                "VM has shut down".into(),
                None,
            );
            self.release_host_execution(execution, Some(&cancellation));
        }
        self.native_resources.close_all();
        if let Some(loader) = &self.module_loader {
            loader.shutdown();
        }
    }

    pub(crate) fn with_module_bindings(module_loader: &ModuleLoader, names: &[String]) -> Self {
        let vm = Self::with_module_loader(module_loader.clone());
        vm.globals
            .borrow_mut()
            .extend(module_loader.native_globals());
        vm.globals
            .borrow_mut()
            .extend(module_loader.builtin_globals());
        for name in names {
            vm.globals
                .borrow_mut()
                .insert(name.clone(), module_binding(name.as_str()));
        }
        vm
    }

    pub(crate) fn run_module(&mut self, program: &InstalledProgram) -> VmResult<Value> {
        self.module_program = Some(program.program.clone());
        self.install_implicit_builtins(&program.program)?;
        self.bind_foreign_declarations(&program.program)?;
        self.run_installed_execution(program)
    }

    /// Executes top-level code and then the program module's validated `main`.
    ///
    /// Loaded modules use [`Self::run_module`], which intentionally does not
    /// invoke their `main` binding.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when top-level execution or the entrypoint
    /// call fails.
    pub fn run_program(&mut self, program: &Program) -> VmResult<Value> {
        self.reset_metrics();
        let program = self.install_compat_named(program, "main")?;
        self.install_implicit_builtins(&program.program)?;
        self.bind_foreign_declarations(&program.program)?;
        let top_level = self.run_installed_execution(&program)?;
        let Some(entrypoint_specification) = program.program.entrypoint() else {
            return Ok(top_level);
        };
        let identity = program
            .program
            .callable_identity(entrypoint_specification.callable_identity)
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "program entrypoint references missing callable identity".into(),
                    None,
                )
            })?;
        let entrypoint = self
            .globals
            .borrow()
            .get("main")
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "program entrypoint `main` does not exist".into(),
                    None,
                )
            })?
            .resolve()
            .map_err(|message| self.error(RuntimeErrorKind::Name, message, None))?;
        let entrypoint = self.entrypoint_callable(entrypoint, identity)?;
        let arguments = match entrypoint_specification.arguments {
            EntrypointArguments::None => Vec::new(),
            EntrypointArguments::List => vec![Value::List(
                self.configuration(None)?
                    .arguments()
                    .iter()
                    .map(|argument| Value::string(argument.as_str()))
                    .collect::<Vec<_>>()
                    .into(),
            )],
            EntrypointArguments::Map => vec![self.configuration(None)?.argument_map()],
        };
        self.stack.clear();
        self.stack.push(entrypoint);
        self.stack.extend(arguments);
        let count = self.stack.len() - 1;
        self.call(&program.program, count, None, None)?;
        self.run_root_execution()
    }

    #[must_use]
    pub fn module_metadata(&self) -> &[ModuleDeclaration] {
        &self.module_metadata
    }

    #[must_use]
    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name).cloned()
    }

    #[doc(hidden)]
    #[must_use]
    pub fn interactive_environment(&self) -> InteractiveEnvironment {
        InteractiveEnvironment {
            globals: global_environment(),
            host_globals: self.globals.clone(),
            local_bindings: HashSet::new(),
            imported_globals: HashSet::new(),
        }
    }

    #[cfg(not(feature = "concurrency"))]
    #[doc(hidden)]
    pub fn start_named_interactive_execution(
        &mut self,
        program: &Program,
        entry: &str,
        environment: &mut InteractiveEnvironment,
    ) -> VmResult<InteractiveExecution> {
        Self::synchronize_host_interactive_environment(environment);
        let host_globals = std::mem::replace(&mut self.globals, environment.globals.clone());
        let host_imported = std::mem::replace(
            &mut self.imported_globals,
            std::mem::take(&mut environment.imported_globals),
        );
        if let Err(error) = self.start_named(program, entry) {
            environment
                .imported_globals
                .clone_from(&self.imported_globals);
            self.globals = host_globals;
            self.imported_globals = host_imported;
            return Err(error);
        }
        Ok(self.detach_interactive_execution(environment, host_globals, host_imported))
    }

    #[cfg(not(feature = "concurrency"))]
    #[doc(hidden)]
    pub fn run_interactive_execution_until_stalled(
        &mut self,
        execution: &mut InteractiveExecution,
        environment: &mut InteractiveEnvironment,
    ) -> VmProgress {
        let (host_globals, host_imported) =
            self.activate_interactive_execution(execution, environment);
        let progress = self.run_until_stalled();
        *execution = self.detach_interactive_execution(environment, host_globals, host_imported);
        progress
    }

    #[cfg(not(feature = "concurrency"))]
    #[doc(hidden)]
    pub fn cancel_interactive_execution(
        &mut self,
        execution: &mut InteractiveExecution,
        environment: &mut InteractiveEnvironment,
    ) {
        let (host_globals, host_imported) =
            self.activate_interactive_execution(execution, environment);
        let error = self.error(
            RuntimeErrorKind::InvalidCall,
            "interactive session was closed".into(),
            None,
        );
        if let Some(host_execution) = self.host_execution.take() {
            self.release_host_execution(host_execution, Some(&error));
        }
        *execution = self.detach_interactive_execution(environment, host_globals, host_imported);
    }

    #[cfg(not(feature = "concurrency"))]
    fn activate_interactive_execution(
        &mut self,
        execution: &mut InteractiveExecution,
        environment: &mut InteractiveEnvironment,
    ) -> (GlobalEnvironment, HashSet<String>) {
        debug_assert!(self.host_execution.is_none());
        let host_globals = std::mem::replace(&mut self.globals, environment.globals.clone());
        let host_imported = std::mem::replace(
            &mut self.imported_globals,
            std::mem::take(&mut execution.imported_globals),
        );
        self.module_program = execution.module_program.take();
        self.module_metadata = std::mem::take(&mut execution.module_metadata);
        self.stack = std::mem::take(&mut execution.stack);
        self.frames = std::mem::take(&mut execution.frames);
        self.cleanup = std::mem::take(&mut execution.cleanup);
        self.progress = std::mem::replace(&mut execution.progress, Rc::new(ProgressDriver::new()));
        self.current_waiter = execution.current_waiter.take();
        self.suspension = execution.suspension.take();
        self.resume = execution.resume.take();
        self.wait_registration = execution.wait_registration.take();
        self.host_execution = execution.host_execution.take();
        self.active_span = execution.active_span.take();
        (host_globals, host_imported)
    }

    #[cfg(not(feature = "concurrency"))]
    fn detach_interactive_execution(
        &mut self,
        environment: &mut InteractiveEnvironment,
        host_globals: GlobalEnvironment,
        host_imported: HashSet<String>,
    ) -> InteractiveExecution {
        let execution_imported = std::mem::take(&mut self.imported_globals);
        environment.globals.clone_from(&self.globals);
        environment.imported_globals.clone_from(&execution_imported);
        self.globals = host_globals;
        self.imported_globals = host_imported;
        InteractiveExecution {
            module_program: self.module_program.take(),
            imported_globals: execution_imported,
            module_metadata: std::mem::take(&mut self.module_metadata),
            stack: std::mem::take(&mut self.stack),
            frames: std::mem::take(&mut self.frames),
            cleanup: std::mem::take(&mut self.cleanup),
            progress: std::mem::replace(&mut self.progress, Rc::new(ProgressDriver::new())),
            current_waiter: self.current_waiter.take(),
            suspension: self.suspension.take(),
            resume: self.resume.take(),
            wait_registration: self.wait_registration.take(),
            host_execution: self.host_execution.take(),
            active_span: self.active_span.take(),
        }
    }

    #[cfg(feature = "concurrency")]
    #[doc(hidden)]
    pub fn start_named_interactive_task(
        &mut self,
        program: &Program,
        entry: &str,
        environment: &mut InteractiveEnvironment,
    ) -> VmResult<InteractiveTask> {
        self.reset_metrics();
        let task_environment = Self::interactive_overlay(environment);
        let nursery = Rc::new(Nursery::root(
            self.progress.clone(),
            #[cfg(feature = "metrics")]
            self.metrics.clone(),
        ));
        let installed = self.install_compat_named(program, entry)?;
        let program = installed.program;
        let entry = installed.entry;
        let closure = Rc::new(Closure {
            chunk: entry,
            captures: Vec::new(),
            program: Some(program.clone()),
            globals: Some(task_environment.globals.clone()),
            capture_sources: Vec::new(),
        });
        let imported_globals = Rc::new(RefCell::new(task_environment.imported_globals));
        let mut task_vm = self.module_closure_vm(
            program.clone(),
            closure,
            Vec::new(),
            None,
            None,
            ClosureCallOptions {
                direct_task_limit: None,
                direct_task_count: None,
                nursery: nursery.clone(),
            },
        )?;
        task_vm
            .imported_globals
            .clone_from(&imported_globals.borrow());
        let task = Rc::new(Task::pending(
            TaskExecution {
                vm: task_vm,
                settle_nursery: false,
                interactive_imported_globals: Some(imported_globals.clone()),
            },
            None,
            nursery.ready_queue(),
        ));
        nursery.add_task(task.clone());
        Ok(InteractiveTask {
            task,
            nursery,
            globals: task_environment.globals,
            imported_globals,
        })
    }

    #[cfg(feature = "concurrency")]
    #[doc(hidden)]
    pub fn cancel_interactive_task(&self, task: &InteractiveTask) {
        let error = self.error(
            RuntimeErrorKind::InvalidCall,
            "interactive session was closed".into(),
            None,
        );
        task.nursery.cancel_all(&error);
        task.nursery.clear();
    }

    #[cfg(feature = "concurrency")]
    #[doc(hidden)]
    pub fn release_interactive_task(task: &InteractiveTask) {
        task.nursery.clear();
    }

    #[cfg(feature = "concurrency")]
    #[doc(hidden)]
    pub fn run_interactive_task_until_stalled(&mut self, task: &InteractiveTask) -> VmProgress {
        loop {
            if let Some(result) = task.outcome() {
                let cancellation = self.error(
                    RuntimeErrorKind::Thrown,
                    "sibling cancelled due to fail-fast".into(),
                    None,
                );
                if let Some(result) = task.nursery.settle_available(&result, &cancellation) {
                    return match result {
                        Ok(value) => VmProgress::Completed(value),
                        Err(error) => VmProgress::Failed(error),
                    };
                }
            }
            let ingress_progress = self.progress.make_available_progress();
            let task_progress = task.nursery.make_available_progress();
            let timer_progress = task.nursery.wake_due_timers();
            if !(ingress_progress || task_progress || timer_progress) {
                return VmProgress::Stalled;
            }
        }
    }

    fn synchronize_host_interactive_environment(environment: &mut InteractiveEnvironment) {
        let host = environment.host_globals.borrow();
        let mut globals = environment.globals.borrow_mut();
        for (name, value) in host.iter() {
            if !environment.local_bindings.contains(name) {
                globals.insert(name.clone(), value.clone());
            }
        }
    }

    #[doc(hidden)]
    pub fn interactive_overlay(environment: &mut InteractiveEnvironment) -> InteractiveEnvironment {
        Self::synchronize_host_interactive_environment(environment);
        InteractiveEnvironment {
            globals: Rc::new(RefCell::new(environment.globals.borrow().clone())),
            host_globals: environment.host_globals.clone(),
            local_bindings: environment.local_bindings.clone(),
            imported_globals: environment.imported_globals.clone(),
        }
    }

    #[doc(hidden)]
    pub fn commit_interactive_bindings(
        environment: &mut InteractiveEnvironment,
        program: &Program,
    ) {
        environment
            .local_bindings
            .extend(program.bindings().iter().cloned());
    }

    #[cfg(not(feature = "concurrency"))]
    #[doc(hidden)]
    pub fn synchronize_interactive_submission(
        source: &InteractiveEnvironment,
        destination: &mut InteractiveEnvironment,
        program: &Program,
    ) {
        let source_globals = source.globals.borrow();
        let mut destination_globals = destination.globals.borrow_mut();
        for name in program.bindings() {
            if let Some(value) = source_globals.get(name) {
                let value = Self::rebind_slim_interactive_value(
                    value.clone(),
                    &source.globals,
                    &destination.globals,
                );
                let mutable = program
                    .declarations()
                    .iter()
                    .any(|declaration| declaration.mutable && declaration.bindings.contains(name));
                if mutable
                    && !destination_globals
                        .get(name)
                        .is_some_and(|binding| binding.replace_binding(value.clone()))
                {
                    destination_globals.insert(
                        name.clone(),
                        Value::Binding {
                            name: name.clone().into(),
                            cell: binding_cell(value.clone()),
                        },
                    );
                } else if !mutable {
                    destination_globals.insert(name.clone(), value);
                }
            }
        }
        for name in &source.imported_globals {
            if let Some(value) = source_globals.get(name) {
                destination_globals.insert(name.clone(), value.clone());
            }
        }
        destination
            .imported_globals
            .clone_from(&source.imported_globals);
    }

    #[cfg(not(feature = "concurrency"))]
    fn rebind_slim_interactive_value(
        value: Value,
        source: &GlobalEnvironment,
        destination: &GlobalEnvironment,
    ) -> Value {
        match value {
            Value::Closure(closure)
                if closure
                    .globals
                    .as_ref()
                    .is_some_and(|globals| Rc::ptr_eq(globals, source)) =>
            {
                Value::Closure(Rc::new(Closure {
                    chunk: closure.chunk,
                    captures: closure.captures.clone(),
                    program: closure.program.clone(),
                    globals: Some(destination.clone()),
                }))
            }
            Value::Overloads(values) => Value::Overloads(Rc::new(
                values
                    .iter()
                    .cloned()
                    .map(|value| Self::rebind_slim_interactive_value(value, source, destination))
                    .collect(),
            )),
            value => value,
        }
    }

    /// Returns counters for the most recent public VM invocation.
    #[must_use]
    #[cfg(feature = "metrics")]
    pub fn metrics(&self) -> VmMetrics {
        self.metrics.borrow().clone()
    }

    #[must_use]
    pub fn exported_values(&self, program: &Program) -> Value {
        Value::Map(
            Map::new(
                program
                    .exports()
                    .iter()
                    .filter_map(|name| {
                        self.globals
                            .borrow()
                            .get(name)
                            .and_then(|value| value.resolve().ok())
                            .map(|value| (Value::string(name.as_str()), value))
                    })
                    .collect(),
            )
            .into_shared(),
        )
    }

    pub(crate) fn live_exported_values(&self, program: &Program) -> Value {
        Value::Map(
            Map::new(
                program
                    .exports()
                    .iter()
                    .filter_map(|name| {
                        self.globals
                            .borrow()
                            .get(name)
                            .cloned()
                            .map(|value| (Value::string(name.as_str()), value))
                    })
                    .collect(),
            )
            .into_shared(),
        )
    }

    /// Installs one validated native descriptor as a local VM global.
    ///
    /// # Errors
    ///
    /// Returns an error when that binding is already defined.
    pub fn define_native(&mut self, function: NativeFunction) -> Result<(), NativeDescriptorError> {
        let name = function.name().to_string();
        if self.globals.borrow().contains_key(&name) {
            return Err(NativeDescriptorError::new(format!(
                "native binding `{name}` is already defined"
            )));
        }
        let value = Value::Native(function);
        if let Some(module_loader) = &self.module_loader {
            module_loader.define_native(name.clone(), value.clone());
        }
        self.globals.borrow_mut().insert(name, value);
        Ok(())
    }

    /// Registers one native descriptor for a matching source `foreign` declaration.
    ///
    /// The descriptor is visible only through a declaration in its owning module.
    ///
    /// # Errors
    ///
    /// Returns an error when the VM has no module loader or the module-qualified
    /// descriptor name is already registered.
    pub fn define_foreign(
        &mut self,
        function: NativeFunction,
    ) -> Result<(), NativeDescriptorError> {
        let loader = self.module_loader.as_ref().ok_or_else(|| {
            NativeDescriptorError::new("foreign bindings require a module loader")
        })?;
        loader.define_foreign(function)
    }

    /// Registers native descriptors atomically for matching source `foreign`
    /// declarations.
    ///
    /// # Errors
    ///
    /// Returns an error when the VM has no module loader or when any descriptor
    /// conflicts with an existing or sibling descriptor. No descriptor is
    /// registered on failure.
    pub fn define_foreign_batch(
        &mut self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        let loader = self.module_loader.as_ref().ok_or_else(|| {
            NativeDescriptorError::new("foreign bindings require a module loader")
        })?;
        loader.define_foreign_batch(functions)
    }

    /// Registers a host function in the implicitly available foundation module.
    ///
    /// # Errors
    ///
    /// Returns an error when the function does not belong to `slug.builtin`,
    /// or when its foreign descriptor cannot be registered.
    pub fn define_builtin(
        &mut self,
        function: NativeFunction,
    ) -> Result<(), NativeDescriptorError> {
        if function.module_name() != "slug.builtin" {
            return Err(NativeDescriptorError::new(
                "builtin bindings must belong to module slug.builtin",
            ));
        }
        self.define_foreign(function)
    }

    fn bind_foreign_declarations(&mut self, program: &Program) -> VmResult<()> {
        let declared_resource_types = program
            .declarations()
            .iter()
            .filter_map(|declaration| declaration.resource_type.as_deref())
            .collect::<std::collections::HashSet<_>>();
        let Some(loader) = self.module_loader.clone() else {
            return if program
                .declarations()
                .iter()
                .any(|declaration| declaration.foreign)
                || !declared_resource_types.is_empty()
            {
                Err(self.error(
                    RuntimeErrorKind::Module,
                    "foreign declarations and resource types require a module loader".into(),
                    None,
                ))
            } else {
                Ok(())
            };
        };
        let mut registered_resource_types = std::collections::HashSet::new();
        for declaration in program
            .declarations()
            .iter()
            .filter(|declaration| declaration.foreign)
        {
            registered_resource_types.extend(self.bind_foreign_declaration(
                &loader,
                program,
                declaration,
            )?);
        }
        self.validate_resource_type_registrations(
            program,
            &declared_resource_types,
            registered_resource_types,
        )
    }

    fn bind_foreign_declaration(
        &mut self,
        loader: &ModuleLoader,
        program: &Program,
        declaration: &ModuleDeclaration,
    ) -> VmResult<std::collections::HashSet<String>> {
        let mut resource_types = std::collections::HashSet::new();
        for name in &declaration.bindings {
            let function = loader.foreign(program.module_name(), name).ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::Module,
                    format!(
                        "foreign function `{}.{name}` is not registered",
                        program.module_name()
                    ),
                    None,
                )
            })?;
            let (minimum, maximum) = declaration.foreign_arity.ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("foreign declaration `{name}` has no arity metadata"),
                    None,
                )
            })?;
            if !function.matches_declared_arity(minimum, maximum) {
                return Err(self.error(
                    RuntimeErrorKind::Module,
                    format!(
                        "foreign function `{}.{name}` does not accept its declared arity",
                        program.module_name()
                    ),
                    None,
                ));
            }
            resource_types.extend(function.resource_type_names());
            let identity = declaration
                .foreign_callable_identity
                .clone()
                .ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        format!("foreign declaration `{name}` has no callable identity"),
                        None,
                    )
                })?;
            let resource_signature =
                declaration
                    .foreign_resource_signature
                    .clone()
                    .ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("foreign declaration `{name}` has no resource signature"),
                            None,
                        )
                    })?;
            let value = Value::DeclaredNative {
                function,
                callable_identity: identity,
                resource_signature: Box::new(resource_signature),
            };
            self.install_foreign_value(name, &value);
        }
        Ok(resource_types)
    }

    fn install_foreign_value(&mut self, name: &str, value: &Value) {
        let binding = self.globals.borrow().get(name).cloned();
        let value = binding
            .as_ref()
            .and_then(|binding| binding.resolve().ok())
            .and_then(|existing| Self::callable_signature(&existing).map(|_| existing))
            .map_or_else(
                || value.clone(),
                |existing| match existing {
                    Value::Overloads(overloads) => {
                        let mut overloads = overloads.as_ref().clone();
                        overloads.push(value.clone());
                        Value::Overloads(Rc::new(overloads))
                    }
                    existing => Value::Overloads(Rc::new(vec![existing, value.clone()])),
                },
            );
        if !binding.is_some_and(|binding| binding.replace_binding(value.clone())) {
            self.globals.borrow_mut().insert(name.into(), value);
        }
    }

    fn validate_resource_type_registrations(
        &self,
        program: &Program,
        declared: &std::collections::HashSet<&str>,
        registered: std::collections::HashSet<String>,
    ) -> VmResult<()> {
        for name in declared {
            if !registered.contains(*name) {
                return Err(self.error(
                    RuntimeErrorKind::Module,
                    format!(
                        "native module `{}` does not register declared resource type `{name}`",
                        program.module_name()
                    ),
                    None,
                ));
            }
        }
        for name in registered {
            if !declared.contains(name.as_str()) {
                return Err(self.error(
                    RuntimeErrorKind::Module,
                    format!(
                        "native module `{}` registers resource type `{name}` without a matching source declaration",
                        program.module_name()
                    ),
                    None,
                ));
            }
        }
        Ok(())
    }

    fn install_implicit_builtins(&mut self, program: &Program) -> VmResult<()> {
        if program.module_name() == "slug.builtin" {
            return Ok(());
        }
        let Some(loader) = &self.module_loader else {
            return Ok(());
        };
        let mut globals = self.globals.borrow_mut();
        for (name, value) in loader.builtin_globals() {
            globals.entry(name).or_insert(value);
        }
        drop(globals);
        let instance = match loader.initialize(None, "slug.builtin") {
            Ok(instance) => instance,
            Err(crate::ModuleLoadError::NotFound { .. }) => return Ok(()),
            Err(error) => {
                return Err(self.error(RuntimeErrorKind::Module, error.to_string(), None));
            }
        };
        let Value::Map(exports) = instance.live_exports else {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "slug.builtin exports are not a map".into(),
                None,
            ));
        };
        let mut globals = self.globals.borrow_mut();
        for (name, value) in exports.iter() {
            let Value::Str(name) = name else {
                return Err(self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "slug.builtin export name is not a string".into(),
                    None,
                ));
            };
            globals
                .entry(name.to_string())
                .or_insert_with(|| value.clone());
        }
        Ok(())
    }

    fn install_configuration_builtins(&mut self) {
        let mut globals = self.globals.borrow_mut();
        globals.insert("cfg".into(), Value::Builtin(Builtin::Cfg));
        globals.insert("stacktrace".into(), Value::Builtin(Builtin::Stacktrace));
    }

    #[cfg(not(feature = "concurrency"))]
    fn runtime_capability_error(
        &self,
        capability: &str,
        span: Option<&SourceSpan>,
    ) -> RuntimeError {
        self.error_at(
            RuntimeErrorKind::InvalidCall,
            format!("runtime capability `{capability}` is unavailable"),
            span,
        )
    }

    /// Executes a zero-argument entry chunk.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is invalid or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run(&mut self, program: &Program, entry: usize) -> VmResult<Value> {
        self.reset_metrics();
        let program = self.install_compat(program, entry)?;
        self.run_installed_execution(&program)
    }

    /// Starts a zero-argument entry for host-driven execution. Call
    /// [`Self::poll`] or [`Self::run_until_stalled`] to drive it. Neither
    /// method waits for an operating-system event.
    ///
    /// # Errors
    ///
    /// Returns a checked error for invalid bytecode, an invalid entry, a shut
    /// down VM, or an already active host-driven execution.
    pub fn start(&mut self, program: &Program, entry: usize) -> VmResult<()> {
        self.reset_metrics();
        let program = self.install_compat(program, entry)?;
        self.start_installed_execution(&program)
    }

    /// Validates and takes ownership of bytecode for one zero-argument entry.
    ///
    /// The resulting value may be reused by any VM without cloning or
    /// revalidating the program. It retains no VM execution state.
    ///
    /// # Errors
    ///
    /// Returns a checked error when the entry is invalid or the program's
    /// private bytecode is malformed.
    pub fn install(&mut self, program: Program, entry: usize) -> VmResult<InstalledProgram> {
        #[cfg(feature = "metrics")]
        let verification_started = Instant::now();
        program
            .validate(entry)
            .map_err(|message| self.error(RuntimeErrorKind::InvalidBytecode, message, None))?;
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.program_validations += 1;
            metrics.verification_time += verification_started.elapsed();
        }
        Ok(InstalledProgram {
            program: Rc::new(program),
            entry,
        })
    }

    /// Validates and takes ownership of bytecode selected by its chunk name.
    ///
    /// # Errors
    ///
    /// Returns a checked name or bytecode error when the requested entry cannot
    /// be installed.
    pub fn install_named(&mut self, program: Program, entry: &str) -> VmResult<InstalledProgram> {
        let index = program.find_chunk(entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::Name,
                format!("unknown entry `{entry}`"),
                None,
            )
        })?;
        self.install(program, index)
    }

    /// Executes an immutable program previously installed by a VM.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is invalid or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run_installed(&mut self, program: &InstalledProgram) -> VmResult<Value> {
        self.reset_metrics();
        self.run_installed_execution(program)
    }

    /// Starts an already installed entry for host-driven execution.
    ///
    /// # Errors
    ///
    /// Returns a checked error for invalid bytecode, an invalid entry, a shut
    /// down VM, or an already active host-driven execution.
    pub fn start_installed(&mut self, program: &InstalledProgram) -> VmResult<()> {
        self.reset_metrics();
        self.start_installed_execution(program)
    }

    fn run_installed_execution(&mut self, program: &InstalledProgram) -> VmResult<Value> {
        self.start_installed_execution(program)?;
        self.blocking_run()
    }

    fn start_installed_execution(&mut self, installed: &InstalledProgram) -> VmResult<()> {
        if self.host_execution.is_some() {
            return Err(self.error(
                RuntimeErrorKind::InvalidCall,
                "VM already has a host-driven execution".into(),
                None,
            ));
        }
        if self.shutdown {
            return Err(self.error(
                RuntimeErrorKind::InvalidCall,
                "VM has shut down".into(),
                None,
            ));
        }
        let program = &installed.program;
        self.module_program = Some(program.clone());
        let chunk = program.chunk(installed.entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("entry chunk {} does not exist", installed.entry),
                None,
            )
        })?;
        if chunk.arity != 0 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!(
                    "entry function `{}` expects {} arguments",
                    chunk.name, chunk.arity
                ),
                None,
            ));
        }
        if chunk.locals < chunk.arity {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!(
                    "entry function `{}` has {} local slots for {} parameters",
                    chunk.name, chunk.locals, chunk.arity
                ),
                None,
            ));
        }
        self.stack.clear();
        self.frames.clear();
        self.cleanup.clear();
        #[cfg(feature = "concurrency")]
        self.nursery.clear();
        self.progress.clear();
        self.module_metadata = program.declarations().to_vec();
        let locals = frame_locals(Vec::new(), chunk.locals);
        self.record_frame_locals(locals.capacity(), 0);
        #[cfg(feature = "metrics")]
        self.record_frame(chunk.locals);
        self.frames.push(Frame {
            program: program.clone(),
            globals: self.globals.clone(),
            closure: Rc::new(Closure {
                chunk: installed.entry,
                captures: Vec::new(),
                program: None,
                globals: None,
                #[cfg(feature = "concurrency")]
                capture_sources: Vec::new(),
            }),
            call_span: None,
            ip: 0,
            stack_base: 0,
            locals,
            provided: self.frame_provided(Some(vec![false; chunk.arity])),
            scope_depth: 1,
            scopes: Vec::new(),
            cleanup_action: false,
            cleanup_recovers: false,
            cleanup_owner_depth: None,
        });
        let root = RootWaiter::new();
        self.current_waiter = Some(Waiter::root(root.clone()));
        self.host_execution = Some(HostExecution { root, result: None });
        Ok(())
    }

    /// Executes a zero-argument chunk selected by name.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is absent or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run_named(&mut self, program: &Program, entry: &str) -> VmResult<Value> {
        self.reset_metrics();
        let program = self.install_compat_named(program, entry)?;
        self.run_installed_execution(&program)
    }

    /// Starts an entry selected by name for host-driven execution.
    ///
    /// # Errors
    ///
    /// Returns a checked error when the named entry is absent or cannot be
    /// started for host-driven execution.
    pub fn start_named(&mut self, program: &Program, entry: &str) -> VmResult<()> {
        self.reset_metrics();
        let program = self.install_compat_named(program, entry)?;
        self.start_installed_execution(&program)
    }

    /// Executes an entry selected by name from an installed program.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is absent or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run_named_installed(&mut self, program: &InstalledProgram) -> VmResult<Value> {
        self.reset_metrics();
        self.run_installed_execution(program)
    }

    fn reset_metrics(&mut self) {
        self.active_span = None;
        #[cfg(feature = "metrics")]
        self.metrics.borrow_mut().clone_from(&VmMetrics::default());
    }

    /// Performs one host-driven progress round. This method never waits for
    /// external input and never re-enters Slug from a producer callback.
    #[must_use]
    pub fn poll(&mut self) -> VmProgress {
        if self.shutdown {
            return VmProgress::Failed(self.error(
                RuntimeErrorKind::InvalidCall,
                "VM has shut down".into(),
                None,
            ));
        }
        let Some(mut execution) = self.host_execution.take() else {
            #[cfg(feature = "concurrency")]
            {
                return if self.nursery.make_available_progress() {
                    VmProgress::MadeProgress
                } else {
                    VmProgress::Stalled
                };
            }
            #[cfg(not(feature = "concurrency"))]
            return VmProgress::Stalled;
        };
        let mut made_progress = false;

        if execution.result.is_none() {
            if let Some(result) = execution.root.take_resume() {
                if let Some(wait_registration) = self.wait_registration.take() {
                    wait_registration.remove_for_waiter(&Waiter::root(execution.root.clone()));
                }
                self.resume = Some(result);
                made_progress = true;
            }
            match self.execute() {
                ExecutionOutcome::Settled(result) => {
                    execution.result = Some(result);
                    made_progress = true;
                }
                ExecutionOutcome::Suspended => {}
            }
        }

        #[cfg(feature = "concurrency")]
        let settled = execution
            .result
            .as_ref()
            .and_then(|result| self.settle_tasks_available(result));
        #[cfg(not(feature = "concurrency"))]
        let settled = execution.result.clone();
        if let Some(result) = settled {
            self.release_host_execution(execution, None);
            return match result {
                Ok(value) => VmProgress::Completed(value),
                Err(error) => VmProgress::Failed(error),
            };
        }

        let scheduler_progress = {
            #[cfg(feature = "concurrency")]
            {
                self.nursery.make_available_progress()
            }
            #[cfg(not(feature = "concurrency"))]
            {
                false
            }
        };
        if self.progress.make_available_progress() || scheduler_progress {
            made_progress = true;
        }
        self.host_execution = Some(execution);
        if made_progress {
            VmProgress::MadeProgress
        } else {
            VmProgress::Stalled
        }
    }

    /// Drives all immediately available VM work to a local fixed point. It
    /// does not perform an operating-system wait.
    #[must_use]
    pub fn run_until_stalled(&mut self) -> VmProgress {
        loop {
            match self.poll() {
                VmProgress::MadeProgress => {}
                result => return result,
            }
        }
    }

    /// Convenience adapter for command-line and simple embedding hosts. The
    /// core progress API remains non-blocking; this method alone waits for
    /// native wake notifications or scheduled timers.
    ///
    /// # Errors
    ///
    /// Returns the execution error, or a checked blocked-task error when no
    /// runnable work or future progress source remains.
    pub fn blocking_run(&mut self) -> VmResult<Value> {
        loop {
            match self.run_until_stalled() {
                VmProgress::Completed(value) => return Ok(value),
                VmProgress::Failed(error) => return Err(error),
                VmProgress::MadeProgress => unreachable!("run_until_stalled exhausts progress"),
                VmProgress::Stalled
                    if {
                        #[cfg(feature = "concurrency")]
                        {
                            self.nursery.wait_for_progress()
                        }
                        #[cfg(not(feature = "concurrency"))]
                        {
                            self.progress.wait_for_progress()
                        }
                    } => {}
                VmProgress::Stalled => {
                    let result = self
                        .host_execution
                        .as_ref()
                        .and_then(|execution| execution.result.as_ref())
                        .and_then(|result| result.as_ref().err())
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidCall,
                                "task remains blocked with no runnable work".into(),
                                None,
                            )
                        });
                    if let Some(execution) = self.host_execution.take() {
                        self.release_host_execution(execution, Some(&result));
                    }
                    return Err(result);
                }
            }
        }
    }

    /// Discards the local state for one host-driven invocation. Callers retain
    /// the terminal result separately so this routine can serve successful,
    /// failed, blocked, and lifecycle-cancelled executions alike.
    fn release_host_execution(
        &mut self,
        execution: HostExecution,
        cancellation: Option<&RuntimeError>,
    ) {
        #[cfg(not(feature = "concurrency"))]
        let _ = cancellation;
        if let Some(wait_registration) = self.wait_registration.take() {
            wait_registration.remove_for_waiter(&Waiter::root(execution.root));
        }
        self.suspension = None;
        self.resume = None;
        self.current_waiter = None;
        self.stack.clear();
        self.frames.clear();
        self.cleanup.clear();
        self.progress.clear();
        #[cfg(feature = "concurrency")]
        if let Some(error) = cancellation {
            self.nursery.cancel_all(error);
            self.nursery.clear();
        }
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    fn install_compat(&mut self, program: &Program, entry: usize) -> VmResult<InstalledProgram> {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.program_clones += 1;
            metrics.program_clone_bytes += program.layout_metrics().instruction_bytes;
        }
        self.install(program.clone(), entry)
    }

    fn install_compat_named(
        &mut self,
        program: &Program,
        entry: &str,
    ) -> VmResult<InstalledProgram> {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.program_clones += 1;
            metrics.program_clone_bytes += program.layout_metrics().instruction_bytes;
        }
        self.install_named(program.clone(), entry)
    }

    #[allow(clippy::too_many_lines)]
    fn execute(&mut self) -> ExecutionOutcome {
        if let Some(result) = self.resume.take() {
            self.suspension = None;
            match result {
                Ok(value) => {
                    if let Some(slot) = self.stack.last_mut() {
                        *slot = value;
                    }
                }
                Err(error) => {
                    self.begin_error(error);
                    match self.drive_cleanup() {
                        Ok(Some(value)) => return ExecutionOutcome::Settled(Ok(value)),
                        Ok(None) => {}
                        Err(error) => return ExecutionOutcome::Settled(Err(error)),
                    }
                }
            }
        }
        loop {
            match self.execute_raw() {
                Ok(ExecutionOutcome::Settled(result)) => return ExecutionOutcome::Settled(result),
                Ok(ExecutionOutcome::Suspended) => return ExecutionOutcome::Suspended,
                Err(error) if self.frames.is_empty() => {
                    return ExecutionOutcome::Settled(Err(error));
                }
                Err(error) => {
                    self.begin_error(error);
                    match self.drive_cleanup() {
                        Ok(Some(value)) => return ExecutionOutcome::Settled(Ok(value)),
                        Ok(None) => {}
                        Err(error) => return ExecutionOutcome::Settled(Err(error)),
                    }
                }
            }
        }
    }

    fn run_root_execution(&mut self) -> VmResult<Value> {
        let root = RootWaiter::new();
        self.current_waiter = Some(Waiter::root(root.clone()));
        loop {
            match self.execute() {
                ExecutionOutcome::Settled(result) => {
                    #[cfg(feature = "concurrency")]
                    return self.settle_tasks(&result);
                    #[cfg(not(feature = "concurrency"))]
                    return result;
                }
                ExecutionOutcome::Suspended => loop {
                    if let Some(result) = root.take_resume() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::root(root.clone()));
                        }
                        self.resume = Some(result);
                        break;
                    }
                    if !self.make_progress() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::root(root.clone()));
                        }
                        let blocked = self.error(
                            RuntimeErrorKind::InvalidCall,
                            "task remains blocked with no runnable work".into(),
                            None,
                        );
                        #[cfg(feature = "concurrency")]
                        return self.settle_tasks(&Err(blocked));
                        #[cfg(not(feature = "concurrency"))]
                        return Err(blocked);
                    }
                },
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn execute_raw(&mut self) -> VmResult<ExecutionOutcome> {
        loop {
            if self.suspension.is_some() {
                return Ok(ExecutionOutcome::Suspended);
            }
            let frame_program = self.active_program()?;
            self.globals = self
                .frames
                .last()
                .expect("active frame was checked")
                .globals
                .clone();
            let instruction = self.next_instruction(&frame_program)?;
            self.active_span = instruction.span;
            let outcome = if let Some(outcome) =
                self.execute_packed_hot_op(&frame_program, &instruction)?
            {
                outcome
            } else {
                let instruction = Program::unpack_instruction(&instruction).map_err(|message| {
                    self.error(RuntimeErrorKind::InvalidBytecode, message, None)
                })?;
                self.execute_borrowed_span_op(&frame_program, &instruction.op, None)?
            };
            if let BorrowedSpanOpOutcome::Settled(value) = outcome {
                return Ok(ExecutionOutcome::Settled(Ok(value)));
            }
        }
    }

    /// Dispatches the common packed instructions without reconstructing a
    /// builder-facing `Op`. Less frequent instructions retain the checked
    /// fallback while this representation transition is measured.
    #[allow(clippy::too_many_lines)]
    fn execute_packed_hot_op(
        &mut self,
        program: &Program,
        instruction: &PackedInstruction,
    ) -> VmResult<Option<BorrowedSpanOpOutcome>> {
        let operand = instruction.a as usize;
        match instruction.opcode {
            PackedOpcode::Constant => {
                let chunk = self.current_chunk(program)?;
                let value = match chunk.constants.get(operand) {
                    Some(crate::Constant::Value(value)) => value.clone(),
                    Some(crate::Constant::Function(function)) => Value::Closure(Rc::new(Closure {
                        chunk: *function,
                        captures: Vec::new(),
                        program: Some(self.active_program()?),
                        globals: Some(self.globals.clone()),
                        #[cfg(feature = "concurrency")]
                        capture_sources: Vec::new(),
                    })),
                    None => {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("constant {operand} does not exist"),
                            None,
                        ));
                    }
                };
                self.stack.push(value);
            }
            PackedOpcode::Nil => self.stack.push(Value::Nil),
            PackedOpcode::True => self.stack.push(Value::Bool(true)),
            PackedOpcode::False => self.stack.push(Value::Bool(false)),
            PackedOpcode::Pop => {
                self.pop_at(None)?;
            }
            PackedOpcode::Duplicate => self.stack.push(self.peek_at(None)?.clone()),
            PackedOpcode::GetLocal => self.stack.push(self.local_value(operand, None)?),
            PackedOpcode::SetLocal => {
                let value = self.pop_at(None)?;
                self.set_local_at(operand, value, None)?;
            }
            PackedOpcode::GetGlobal => {
                let name = program
                    .global_name(crate::GlobalNameId::new(instruction.a))
                    .expect("validated global name metadata");
                let value = self
                    .globals
                    .borrow()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            None,
                        )
                    })?
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, None))?;
                self.stack.push(value);
            }
            PackedOpcode::List => {
                let values = self.pop_values_at(operand, None)?;
                #[cfg(feature = "metrics")]
                self.record_collection_construction(values.len());
                self.stack
                    .push(Value::List(List::from_values(values).into_shared()));
            }
            PackedOpcode::Map => {
                let values = self.pop_values_at(operand.saturating_mul(2), None)?;
                let mut entries = Vec::with_capacity(operand);
                for pair in values.chunks_exact(2) {
                    if !is_map_key(&pair[0]) {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("{} cannot be used as a map key", pair[0].type_name()),
                            None,
                        ));
                    }
                    entries.push((pair[0].clone(), pair[1].clone()));
                }
                #[cfg(feature = "metrics")]
                self.record_collection_construction(entries.len());
                self.stack.push(Value::Map(Map::new(entries).into_shared()));
            }
            PackedOpcode::GetIndex => {
                let (collection, index) = self.pop_pair_at(None)?;
                self.stack.push(
                    index_value(
                        collection,
                        &index,
                        #[cfg(feature = "metrics")]
                        &self.metrics,
                    )
                    .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, None))?,
                );
            }
            PackedOpcode::Add => {
                let (left, right) = self.pop_pair_at(None)?;
                #[cfg(feature = "metrics")]
                match (&left, &right) {
                    (Value::List(left), Value::List(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    (Value::Map(left), Value::Map(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    (Value::Bytes(left), Value::Bytes(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    _ => {}
                }
                self.stack.push(
                    add(left, right)
                        .map_err(|(kind, message)| self.error_at(kind, message, None))?,
                );
            }
            PackedOpcode::Subtract => {
                let (left, right) = self.pop_pair_at(None)?;
                #[cfg(feature = "metrics")]
                if let Value::Map(entries) = &left {
                    self.record_collection_update(entries.len(), Rc::strong_count(entries) == 1);
                }
                self.stack.push(
                    subtract(left, right)
                        .map_err(|(kind, message)| self.error_at(kind, message, None))?,
                );
            }
            PackedOpcode::Multiply => self.binary_at(None, multiply)?,
            PackedOpcode::Divide => self.binary_at(None, divide)?,
            PackedOpcode::Modulo => self.binary_at(None, modulo)?,
            PackedOpcode::Equal => {
                let (left, right) = self.pop_pair_at(None)?;
                self.stack.push(Value::Bool(left == right));
            }
            PackedOpcode::Greater => self.compare_at(None, std::cmp::Ordering::Greater)?,
            PackedOpcode::Less => self.compare_at(None, std::cmp::Ordering::Less)?,
            PackedOpcode::Jump => self.jump_at(operand, None)?,
            PackedOpcode::JumpIfFalse => {
                if !self.peek_at(None)?.is_truthy() {
                    self.jump_at(operand, None)?;
                }
            }
            PackedOpcode::CallPositional => self.call_positional_at(program, operand, None)?,
            PackedOpcode::RecurPositional => self.recur_positional_at(program, operand, None)?,
            PackedOpcode::Return => {
                let value = self.pop_at(None)?;
                if let Some(value) = self.begin_return(value)? {
                    return Ok(Some(BorrowedSpanOpOutcome::Settled(value)));
                }
            }
            _ => return Ok(None),
        }
        Ok(Some(BorrowedSpanOpOutcome::Continue))
    }

    #[allow(clippy::too_many_lines)]
    fn execute_borrowed_span_op(
        &mut self,
        program: &Program,
        op: &Op,
        span: Option<&SourceSpan>,
    ) -> VmResult<BorrowedSpanOpOutcome> {
        match op {
            Op::Constant(index) => {
                let chunk = self.current_chunk(program)?;
                let value = match chunk.constants.get(*index) {
                    Some(crate::Constant::Value(value)) => value.clone(),
                    Some(crate::Constant::Function(function)) => Value::Closure(Rc::new(Closure {
                        chunk: *function,
                        captures: Vec::new(),
                        program: Some(self.active_program()?),
                        globals: Some(self.globals.clone()),
                        #[cfg(feature = "concurrency")]
                        capture_sources: Vec::new(),
                    })),
                    None => {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("constant {index} does not exist"),
                            span,
                        ));
                    }
                };
                self.stack.push(value);
            }
            Op::Interpolate(parts) => {
                let values = self.pop_values_at(parts.len().saturating_sub(1), span)?;
                let mut output = String::new();
                for (index, text) in parts.iter().enumerate() {
                    output.push_str(text);
                    if let Some(value) = values.get(index) {
                        output.push_str(&value.to_string());
                    }
                }
                self.stack.push(Value::string(output));
            }
            Op::InterpolatePooled(id) => {
                let parts = program.interpolation(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "interpolation metadata does not exist".into(),
                        span,
                    )
                })?;
                let values = self.pop_values_at(parts.len().saturating_sub(1), span)?;
                let mut output = String::new();
                for (index, text) in parts.iter().enumerate() {
                    output.push_str(text);
                    if let Some(value) = values.get(index) {
                        output.push_str(&value.to_string());
                    }
                }
                self.stack.push(Value::string(output));
            }
            Op::GetCapture(slot) => {
                let value = self
                    .frames
                    .last()
                    .and_then(|frame| frame.closure.captures.get(*slot))
                    .map(|cell| cell.borrow().clone())
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("capture {slot} does not exist"),
                            span,
                        )
                    })?;
                self.stack.push(value);
            }
            Op::SetCapture(slot) => {
                let value = self.pop_at(span)?;
                let capture = self
                    .frames
                    .last()
                    .and_then(|frame| frame.closure.captures.get(*slot))
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("capture {slot} does not exist"),
                            span,
                        )
                    })?;
                *capture.borrow_mut() = value;
            }
            Op::NotImplemented => {
                return Err(self.error_at(
                    RuntimeErrorKind::NotImplemented,
                    "not implemented".into(),
                    span,
                ));
            }
            Op::Nil => self.stack.push(Value::Nil),
            Op::True => self.stack.push(Value::Bool(true)),
            Op::False => self.stack.push(Value::Bool(false)),
            Op::Pop => {
                self.pop_at(span)?;
            }
            Op::Duplicate => self.stack.push(self.peek_at(span)?.clone()),
            Op::GetLocal(slot) => self.stack.push(self.local_value(*slot, span)?),
            Op::SetLocal(slot) => {
                let value = self.pop_at(span)?;
                self.set_local_at(*slot, value, span)?;
            }
            Op::GetGlobal(name) => {
                let value = self
                    .globals
                    .borrow()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            span,
                        )
                    })?
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
                self.stack.push(value);
            }
            Op::DefineGlobal(name) => {
                let value = self.pop_unresolved_at(span)?;
                if self.imported_globals.remove(name) {
                    self.warning(format!(
                        "local binding `{name}` shadows an imported binding"
                    ));
                }
                if !self
                    .globals
                    .borrow()
                    .get(name)
                    .is_some_and(|binding| binding.replace_binding(value.clone()))
                {
                    self.globals.borrow_mut().insert(name.clone(), value);
                }
            }
            Op::CombineOverloads => {
                let existing = self.pop_unresolved_at(span)?;
                let new = self.pop_unresolved_at(span)?;
                let mut overloads = match existing {
                    Value::Overloads(overloads) => overloads.as_ref().clone(),
                    value if Self::callable_signature(&value).is_some() => vec![value],
                    _ => {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            "overload combination requires callable values".into(),
                            span,
                        ));
                    }
                };
                match new {
                    Value::Overloads(values) => overloads.extend(values.iter().cloned()),
                    value if Self::callable_signature(&value).is_some() => overloads.push(value),
                    _ => {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            "overload combination requires callable values".into(),
                            span,
                        ));
                    }
                }
                self.stack.push(Value::Overloads(Rc::new(overloads)));
            }
            Op::DefineMapGlobals => {
                let value = self.pop_unresolved_at(span)?;
                let Value::Map(entries) = value else {
                    return Err(self.error_at(
                        RuntimeErrorKind::Type,
                        format!("{{*}} binding expects a map, got {}", value.type_name()),
                        span,
                    ));
                };
                for (key, value) in entries.iter() {
                    let Value::Str(name) = key else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            "{*} binding requires string map keys".into(),
                            span,
                        ));
                    };
                    let name = name.to_string();
                    let existing = self.globals.borrow().get(&name).cloned();
                    let Some(existing) = existing else {
                        self.globals
                            .borrow_mut()
                            .insert(name.clone(), value.clone());
                        self.imported_globals.insert(name);
                        continue;
                    };
                    if existing.is_uninitialized_binding() {
                        existing.replace_binding(value.clone());
                        self.imported_globals.insert(name);
                    } else {
                        self.warning(format!(
                            "imported binding `{name}` is shadowed by a local binding"
                        ));
                    }
                }
            }
            Op::RecordModuleTag {
                declaration,
                tag,
                arguments,
            } => {
                let arguments = self.pop_values_at(*arguments, span)?;
                if self
                    .module_metadata
                    .get(*declaration)
                    .is_none_or(|declaration| declaration.tags.get(*tag).is_none())
                {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "module tag metadata does not exist".into(),
                        span,
                    ));
                }
                self.module_metadata[*declaration].tags[*tag].arguments = arguments;
            }
            Op::SetGlobal(name) => {
                if !self.globals.borrow().contains_key(name) {
                    return Err(self.error_at(
                        RuntimeErrorKind::Name,
                        format!("unknown name `{name}`"),
                        span,
                    ));
                }
                let value = self.pop_at(span)?;
                if !self
                    .globals
                    .borrow()
                    .get(name)
                    .is_some_and(|binding| binding.replace_binding(value.clone()))
                {
                    self.globals.borrow_mut().insert(name.clone(), value);
                }
            }
            Op::MakeClosure { chunk, captures } => {
                program.chunk(*chunk).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        format!("function chunk {chunk} does not exist"),
                        span,
                    )
                })?;
                #[cfg(feature = "concurrency")]
                let capture_sources = captures.clone();
                let captures = captures
                    .iter()
                    .map(|capture| match capture {
                        Capture::Local(slot) => self.promote_local_at(*slot, span),
                        Capture::Capture(slot) => self
                            .frames
                            .last()
                            .and_then(|frame| frame.closure.captures.get(*slot))
                            .cloned()
                            .ok_or_else(|| {
                                self.error_at(
                                    RuntimeErrorKind::InvalidBytecode,
                                    format!("capture {slot} does not exist"),
                                    span,
                                )
                            }),
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                self.stack.push(Value::Closure(Rc::new(Closure {
                    chunk: *chunk,
                    captures,
                    program: Some(self.active_program()?),
                    globals: Some(self.globals.clone()),
                    #[cfg(feature = "concurrency")]
                    capture_sources,
                })));
            }
            Op::Add => {
                let (left, right) = self.pop_pair_at(span)?;
                #[cfg(feature = "metrics")]
                match (&left, &right) {
                    (Value::List(left), Value::List(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    (Value::Map(left), Value::Map(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    (Value::Bytes(left), Value::Bytes(right)) => {
                        self.record_collection_update(
                            left.len() + right.len(),
                            Rc::strong_count(left) == 1,
                        );
                    }
                    _ => {}
                }
                self.stack.push(
                    add(left, right)
                        .map_err(|(kind, message)| self.error_at(kind, message, span))?,
                );
            }
            Op::Subtract => {
                let (left, right) = self.pop_pair_at(span)?;
                #[cfg(feature = "metrics")]
                if let Value::Map(entries) = &left {
                    self.record_collection_update(entries.len(), Rc::strong_count(entries) == 1);
                }
                self.stack.push(
                    subtract(left, right)
                        .map_err(|(kind, message)| self.error_at(kind, message, span))?,
                );
            }
            Op::Multiply => self.binary_at(span, multiply)?,
            Op::Divide => self.binary_at(span, divide)?,
            Op::Modulo => self.binary_at(span, modulo)?,
            Op::BitAnd => {
                self.binary_at(span, |left, right| bitwise(left, right, |a, b| a & b))?;
            }
            Op::BitOr => {
                self.binary_at(span, |left, right| bitwise(left, right, |a, b| a | b))?;
            }
            Op::BitXor => {
                self.binary_at(span, |left, right| bitwise(left, right, |a, b| a ^ b))?;
            }
            Op::ShiftLeft => {
                self.binary_at(span, |left, right| shift(left, right, i64::checked_shl))?;
            }
            Op::ShiftRight => {
                self.binary_at(span, |left, right| shift(left, right, i64::checked_shr))?;
            }
            Op::ListAppend => {
                let (list, value) = self.pop_pair_at(span)?;
                #[cfg(feature = "metrics")]
                match &list {
                    Value::List(values) => {
                        self.record_collection_update(values.len(), Rc::strong_count(values) == 1);
                    }
                    Value::Bytes(values) => {
                        self.record_collection_update(values.len(), Rc::strong_count(values) == 1);
                    }
                    _ => {}
                }
                self.stack.push(
                    list_append(list, value)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::ListPrepend => {
                let (value, list) = self.pop_pair_at(span)?;
                #[cfg(feature = "metrics")]
                match &list {
                    Value::List(values) => {
                        self.record_collection_update(values.len(), Rc::strong_count(values) == 1);
                    }
                    Value::Bytes(values) => {
                        self.record_collection_update(values.len(), Rc::strong_count(values) == 1);
                    }
                    _ => {}
                }
                self.stack.push(
                    list_prepend(value, list)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::List(count) => {
                let values = self.pop_values_at(*count, span)?;
                #[cfg(feature = "metrics")]
                self.record_collection_construction(values.len());
                self.stack
                    .push(Value::List(List::from_values(values).into_shared()));
            }
            Op::ListSpread(spreads) => self.list_spread_at(spreads, span)?,
            Op::ListSpreadPooled(id) => {
                let spreads = program.list_spread(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "list spread metadata does not exist".into(),
                        span,
                    )
                })?;
                self.list_spread_at(spreads, span)?;
            }
            Op::Map(count) => {
                let values = self.pop_values_at(count.saturating_mul(2), span)?;
                let mut entries = Vec::with_capacity(*count);
                for pair in values.chunks_exact(2) {
                    if !is_map_key(&pair[0]) {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("{} cannot be used as a map key", pair[0].type_name()),
                            span,
                        ));
                    }
                    entries.push((pair[0].clone(), pair[1].clone()));
                }
                #[cfg(feature = "metrics")]
                self.record_collection_construction(entries.len());
                self.stack.push(Value::Map(Map::new(entries).into_shared()));
            }
            Op::StructSchema(fields) => {
                let default_count = fields.iter().filter(|field| field.has_default).count();
                let defaults = self.pop_values_at(default_count, span)?;
                let mut defaults = defaults.into_iter();
                let mut names = Vec::with_capacity(fields.len());
                let mut schema_fields = Vec::with_capacity(fields.len());
                for field in fields {
                    if names.contains(&field.name) {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("duplicate struct schema field '{}'", field.name),
                            span,
                        ));
                    }
                    names.push(field.name.clone());
                    schema_fields.push(crate::StructField {
                        name: field.name.clone().into(),
                        default: field.has_default.then(|| {
                            defaults
                                .next()
                                .expect("default count was derived from field metadata")
                        }),
                    });
                }
                self.stack
                    .push(Value::StructSchema(Rc::new(crate::StructSchema {
                        fields: schema_fields,
                    })));
            }
            Op::Struct(fields) => {
                let values = self.pop_values_at(fields.len(), span)?;
                let schema = self.pop_at(span)?;
                #[cfg(feature = "metrics")]
                self.record_collection_construction(values.len());
                self.stack.push(
                    construct_struct(schema, fields, &values)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::StructCopy(fields) => {
                let replacements = self.pop_values_at(fields.len(), span)?;
                let value = self.pop_at(span)?;
                #[cfg(feature = "metrics")]
                match &value {
                    Value::Map(value) => {
                        self.record_collection_update(value.len(), Rc::strong_count(value) == 1);
                    }
                    Value::Struct(value) => {
                        self.record_collection_update(
                            value.values.len(),
                            Rc::strong_count(value) == 1,
                        );
                    }
                    _ => {}
                }
                self.stack.push(
                    copy_value(value, fields, &replacements)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::GetSlice {
                has_start,
                has_end,
                has_step,
            } => {
                let count =
                    usize::from(*has_start) + usize::from(*has_end) + usize::from(*has_step);
                let mut values = self.pop_values_at(count + 1, span)?.into_iter();
                let collection = values
                    .next()
                    .expect("slice operation includes a collection");
                #[cfg(feature = "metrics")]
                self.record_collection_slice();
                let start = has_start.then(|| values.next().expect("slice start is present"));
                let end = has_end.then(|| values.next().expect("slice end is present"));
                let step = has_step.then(|| values.next().expect("slice step is present"));
                self.stack.push(
                    slice_value(collection, start.as_ref(), end.as_ref(), step.as_ref())
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::GetIndex => {
                let (collection, index) = self.pop_pair_at(span)?;
                self.stack.push(
                    index_value(
                        collection,
                        &index,
                        #[cfg(feature = "metrics")]
                        &self.metrics,
                    )
                    .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::Negate => {
                let value = self.pop_at(span)?;
                self.stack.push(
                    negate(value)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::Not => {
                let value = self.pop_at(span)?;
                self.stack.push(Value::Bool(!value.is_truthy()));
            }
            Op::BitNot => {
                let value = self.pop_at(span)?;
                self.stack.push(
                    bit_not(&value)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::Equal => {
                let (left, right) = self.pop_pair_at(span)?;
                self.stack.push(Value::Bool(left == right));
            }
            Op::Greater => self.compare_at(span, std::cmp::Ordering::Greater)?,
            Op::Less => self.compare_at(span, std::cmp::Ordering::Less)?,
            Op::GuardGreater => self.guard_compare_at(span, std::cmp::Ordering::Greater)?,
            Op::GuardLess => self.guard_compare_at(span, std::cmp::Ordering::Less)?,
            Op::Jump(target) => self.jump_at(*target, span)?,
            Op::JumpIfFalse(target) => {
                if !self.peek_at(span)?.is_truthy() {
                    self.jump_at(*target, span)?;
                }
            }
            Op::JumpIfProvided { slot, target } => {
                if self
                    .frames
                    .last()
                    .is_some_and(|frame| frame.provided.is_provided(*slot))
                {
                    self.jump_at(*target, span)?;
                }
            }
            Op::Recur(kinds) => self.recur_at(program, kinds, span)?,
            Op::RecurPooled(id) => {
                let kinds = program.call_arguments(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                self.recur_at(program, kinds, span)?;
            }
            Op::RecurPositional(count) => self.recur_positional_at(program, *count, span)?,
            Op::Call(count) => self.call_at(program, *count, None, span)?,
            Op::CallPositional(count) => self.call_positional_at(program, *count, span)?,
            Op::CallSpread(kinds) => self.call_spread_at(program, kinds, None, span)?,
            Op::CallSpreadPooled(id) => {
                let kinds = program.call_arguments(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                self.call_spread_at(program, kinds, None, span)?;
            }
            Op::CallSelected { kinds, identity } => {
                let identity = program.callable_identity(*identity).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected callable identity does not exist".into(),
                        span,
                    )
                })?;
                self.call_spread_at(program, kinds, Some(identity), span)?;
            }
            Op::CallSelectedPooled(id) => {
                let (kinds, identity) = program.selected_call(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected call metadata does not exist".into(),
                        span,
                    )
                })?;
                let kinds = program.call_arguments(kinds).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                let identity = program.callable_identity(identity).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected callable identity does not exist".into(),
                        span,
                    )
                })?;
                self.call_spread_at(program, kinds, Some(identity), span)?;
            }
            Op::PipelineCall(kinds) => self.pipeline_call_at(program, kinds, None, span)?,
            Op::PipelineCallPooled(id) => {
                let kinds = program.call_arguments(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                self.pipeline_call_at(program, kinds, None, span)?;
            }
            Op::PipelineCallSelected { kinds, identity } => {
                let identity = program.callable_identity(*identity).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected callable identity does not exist".into(),
                        span,
                    )
                })?;
                self.pipeline_call_at(program, kinds, Some(identity), span)?;
            }
            Op::PipelineCallSelectedPooled(id) => {
                let (kinds, identity) = program.selected_call(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected call metadata does not exist".into(),
                        span,
                    )
                })?;
                let kinds = program.call_arguments(kinds).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                let identity = program.callable_identity(identity).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected callable identity does not exist".into(),
                        span,
                    )
                })?;
                self.pipeline_call_at(program, kinds, Some(identity), span)?;
            }
            Op::Import(kinds) => self.import_at(kinds, span)?,
            Op::ImportPooled(id) => {
                let kinds = program.call_arguments(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "call metadata does not exist".into(),
                        span,
                    )
                })?;
                self.import_at(kinds, span)?;
            }
            Op::Select(cases) => {
                self.select_at(cases, span)?;
            }
            Op::SelectPooled(id) => {
                let cases = program.select_cases(*id).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "select metadata does not exist".into(),
                        span,
                    )
                })?;
                self.select_at(cases, span)?;
            }
            Op::SelectApply => self.select_apply_at(program, span)?,
            Op::Spawn => {
                #[cfg(feature = "concurrency")]
                self.spawn_task_at(program, span)?;
                #[cfg(not(feature = "concurrency"))]
                return Err(self.runtime_capability_error("spawn", span));
            }
            Op::Nursery { has_limit } => {
                #[cfg(feature = "concurrency")]
                self.run_nursery_at(program, *has_limit, span)?;
                #[cfg(not(feature = "concurrency"))]
                {
                    let _ = has_limit;
                    return Err(self.runtime_capability_error("nursery", span));
                }
            }
            Op::EnterScope => {
                if self.frames.is_empty() {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active call frame".into(),
                        span,
                    ));
                }
                if self
                    .frames
                    .last()
                    .is_some_and(|frame| frame.scope_depth == u32::MAX)
                {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "scope nesting is too deep".into(),
                        span,
                    ));
                }
                let frame = self.frames.last_mut().expect("frame was checked");
                frame.scope_depth += 1;
                if !frame.scopes.is_empty() {
                    frame.scopes.push(Vec::new());
                }
            }
            Op::LeaveScope => {
                if self.frames.is_empty() {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active call frame".into(),
                        span,
                    ));
                }
                let frame = self.frames.last_mut().expect("frame was checked");
                if frame.scope_depth <= 1 {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active scope".into(),
                        span,
                    ));
                }
                frame.scope_depth -= 1;
                let actions = frame.scopes.pop().unwrap_or_default();
                if self.frames.last().is_some_and(|frame| frame.cleanup_action) {
                    self.cleanup.push(Cleanup::Resume);
                }
                self.cleanup.push(Cleanup::Actions {
                    actions,
                    success: true,
                    frame_depth: self.frames.len() - 1,
                });
                if let Some(value) = self.drive_cleanup()? {
                    return Ok(BorrowedSpanOpOutcome::Settled(value));
                }
            }
            Op::Defer { mode } => {
                let action = self.pop_at(span)?;
                if !matches!(
                    action,
                    Value::Closure(_)
                        | Value::Native(_)
                        | Value::DeclaredNative { .. }
                        | Value::Builtin(_)
                ) {
                    return Err(self.error_at(
                        RuntimeErrorKind::Type,
                        "defer expects a callable action".into(),
                        span,
                    ));
                }
                let Some(frame) = self.frames.last_mut() else {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active call frame".into(),
                        span,
                    ));
                };
                if frame.scope_depth == 0 {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active scope".into(),
                        span,
                    ));
                }
                let materialized_depth = frame.scopes.is_empty().then_some(frame.scope_depth);
                if let Some(depth) = materialized_depth {
                    frame.scopes.resize_with(depth as usize, Vec::new);
                }
                let scope = frame.scopes.last_mut().expect("scope depth was checked");
                scope.push(Deferred {
                    action,
                    mode: *mode,
                });
                if let Some(depth) = materialized_depth {
                    self.record_defer_scope_stack(depth as usize);
                }
            }
            Op::TryMatch {
                pattern,
                bindings,
                operands,
            } => {
                let operands = self.pop_values_at(*operands, span)?;
                let value = self.pop_at(span)?;
                let mut values = Vec::new();
                let matched = matches_pattern(pattern, &value, &operands, &mut values)
                    .map_err(|(kind, message)| self.error_at(kind, message, span))?;
                if matched && values.len() != *bindings {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "match pattern binding count is invalid".into(),
                        span,
                    ));
                }
                if matched {
                    self.stack.extend(values);
                } else {
                    self.stack.extend((0..*bindings).map(|_| Value::Nil));
                }
                self.stack.push(Value::Bool(matched));
            }
            Op::MatchFailure => {
                return Err(self.error_at(
                    RuntimeErrorKind::Match,
                    "destructuring pattern did not match".into(),
                    span,
                ));
            }
            Op::Throw => {
                let value = self.pop_at(span)?;
                return Err(self.thrown(value, self.owned_span(span)));
            }
            Op::Return => {
                let value = self.pop_at(span)?;
                if let Some(value) = self.begin_return(value)? {
                    return Ok(BorrowedSpanOpOutcome::Settled(value));
                }
            }
            Op::GetGlobalPooled(id) => {
                let name = program
                    .global_name(*id)
                    .expect("validated global name metadata");
                let value = self
                    .globals
                    .borrow()
                    .get(name)
                    .cloned()
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            span,
                        )
                    })?
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
                self.stack.push(value);
            }
            Op::DefineGlobalPooled(id) => {
                let name = program
                    .global_name(*id)
                    .expect("validated global name metadata");
                let value = self.pop_unresolved_at(span)?;
                if self.imported_globals.remove(name) {
                    self.warning(format!(
                        "local binding `{name}` shadows an imported binding"
                    ));
                }
                if !self
                    .globals
                    .borrow()
                    .get(name)
                    .is_some_and(|binding| binding.replace_binding(value.clone()))
                {
                    self.globals.borrow_mut().insert(name.into(), value);
                }
            }
            Op::SetGlobalPooled(id) => {
                let name = program
                    .global_name(*id)
                    .expect("validated global name metadata");
                if !self.globals.borrow().contains_key(name) {
                    return Err(self.error_at(
                        RuntimeErrorKind::Name,
                        format!("unknown name `{name}`"),
                        span,
                    ));
                }
                let value = self.pop_at(span)?;
                if !self
                    .globals
                    .borrow()
                    .get(name)
                    .is_some_and(|binding| binding.replace_binding(value.clone()))
                {
                    self.globals.borrow_mut().insert(name.into(), value);
                }
            }
            Op::MakeClosurePooled { chunk, captures } => {
                program.chunk(*chunk).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        format!("function chunk {chunk} does not exist"),
                        span,
                    )
                })?;
                let capture_sources = program
                    .capture_list(*captures)
                    .expect("validated capture metadata");
                let captures = capture_sources
                    .iter()
                    .map(|capture| match capture {
                        Capture::Local(slot) => self.promote_local_at(*slot, span),
                        Capture::Capture(slot) => self
                            .frames
                            .last()
                            .and_then(|frame| frame.closure.captures.get(*slot))
                            .cloned()
                            .ok_or_else(|| {
                                self.error_at(
                                    RuntimeErrorKind::InvalidBytecode,
                                    format!("capture {slot} does not exist"),
                                    span,
                                )
                            }),
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                self.stack.push(Value::Closure(Rc::new(Closure {
                    chunk: *chunk,
                    captures,
                    program: Some(self.active_program()?),
                    globals: Some(self.globals.clone()),
                    #[cfg(feature = "concurrency")]
                    capture_sources: capture_sources.to_vec(),
                })));
            }
            Op::StructSchemaPooled(id) => {
                let fields = program
                    .schema_fields(*id)
                    .expect("validated schema field metadata");
                let default_count = fields.iter().filter(|field| field.has_default).count();
                let defaults = self.pop_values_at(default_count, span)?;
                let mut defaults = defaults.into_iter();
                let mut names = Vec::with_capacity(fields.len());
                let mut schema_fields = Vec::with_capacity(fields.len());
                for field in fields {
                    if names.contains(&field.name) {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("duplicate struct schema field '{}'", field.name),
                            span,
                        ));
                    }
                    names.push(field.name.clone());
                    schema_fields.push(crate::StructField {
                        name: field.name.clone().into(),
                        default: field.has_default.then(|| {
                            defaults
                                .next()
                                .expect("default count was derived from field metadata")
                        }),
                    });
                }
                self.stack
                    .push(Value::StructSchema(Rc::new(crate::StructSchema {
                        fields: schema_fields,
                    })));
            }
            Op::StructPooled(id) => {
                let fields = program
                    .struct_fields(*id)
                    .expect("validated struct field metadata");
                let values = self.pop_values_at(fields.len(), span)?;
                let schema = self.pop_at(span)?;
                #[cfg(feature = "metrics")]
                self.record_collection_construction(values.len());
                self.stack.push(
                    construct_struct(schema, fields, &values)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::StructCopyPooled(id) => {
                let fields = program
                    .struct_fields(*id)
                    .expect("validated struct field metadata");
                let replacements = self.pop_values_at(fields.len(), span)?;
                let value = self.pop_at(span)?;
                #[cfg(feature = "metrics")]
                match &value {
                    Value::Map(value) => {
                        self.record_collection_update(value.len(), Rc::strong_count(value) == 1);
                    }
                    Value::Struct(value) => {
                        self.record_collection_update(
                            value.values.len(),
                            Rc::strong_count(value) == 1,
                        );
                    }
                    _ => {}
                }
                self.stack.push(
                    copy_value(value, fields, &replacements)
                        .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?,
                );
            }
            Op::TryMatchPooled {
                pattern,
                bindings,
                operands,
            } => {
                let pattern = program
                    .match_pattern(*pattern)
                    .expect("validated match pattern metadata");
                let operands = self.pop_values_at(*operands, span)?;
                let value = self.pop_at(span)?;
                let mut values = Vec::new();
                let matched = matches_pattern(pattern, &value, &operands, &mut values)
                    .map_err(|(kind, message)| self.error_at(kind, message, span))?;
                if matched && values.len() != *bindings {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "match pattern binding count is invalid".into(),
                        span,
                    ));
                }
                if matched {
                    self.stack.extend(values);
                } else {
                    self.stack.extend((0..*bindings).map(|_| Value::Nil));
                }
                self.stack.push(Value::Bool(matched));
            }
        }
        Ok(BorrowedSpanOpOutcome::Continue)
    }

    fn next_instruction(&mut self, program: &Program) -> VmResult<PackedInstruction> {
        let (chunk_index, ip) = self
            .frames
            .last()
            .map(|frame| (frame.closure.chunk, frame.ip))
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    None,
                )
            })?;
        let chunk = program.chunk(chunk_index).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "active chunk does not exist".into(),
                None,
            )
        })?;
        let instruction = chunk.code.get(ip).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("function `{}` ended without Return", chunk.name),
                None,
            )
        })?;
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.instructions_executed += 1;
        }
        self.frames.last_mut().expect("active frame was checked").ip += 1;
        Ok(*instruction)
    }

    fn current_chunk<'a>(
        &self,
        program: &'a Program,
    ) -> VmResult<&'a crate::bytecode::CompiledChunk> {
        let chunk = self
            .frames
            .last()
            .map(|frame| frame.closure.chunk)
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    None,
                )
            })?;
        program.chunk(chunk).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "active chunk does not exist".into(),
                None,
            )
        })
    }

    fn active_program(&self) -> VmResult<Rc<Program>> {
        self.frames
            .last()
            .map(|frame| frame.program.clone())
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    None,
                )
            })
    }

    #[allow(clippy::too_many_lines)]
    fn call(
        &mut self,
        program: &Program,
        count: usize,
        provided: Option<Vec<bool>>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let required = count.checked_add(1).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span.clone(),
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span.clone(),
            )
        })?;
        let callee = self.stack[base]
            .resolve()
            .map_err(|message| self.error(RuntimeErrorKind::Name, message, span.clone()))?;
        match callee {
            Value::Closure(closure) => {
                let frame_program = closure
                    .program
                    .clone()
                    .or_else(|| self.module_program.clone())
                    .unwrap_or_else(|| Rc::new(program.clone()));
                let chunk = frame_program.chunk(closure.chunk).ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        "closure references missing chunk".into(),
                        span.clone(),
                    )
                })?;
                if chunk.arity != count {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!(
                            "`{}` expects {} arguments, got {count}",
                            chunk.name, chunk.arity
                        ),
                        span,
                    ));
                }
                if chunk.locals < chunk.arity {
                    return Err(self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        format!(
                            "function `{}` has {} local slots for {} parameters",
                            chunk.name, chunk.locals, chunk.arity
                        ),
                        span,
                    ));
                }
                self.record_closure_argument_vector();
                let locals = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let argument_count = locals.len();
                let locals = frame_locals(locals, chunk.locals);
                self.record_frame_locals(locals.capacity(), argument_count);
                #[cfg(feature = "metrics")]
                self.record_frame(chunk.locals);
                self.frames.push(Frame {
                    program: frame_program.clone(),
                    globals: closure
                        .globals
                        .clone()
                        .unwrap_or_else(|| self.globals.clone()),
                    closure,
                    call_span: span.map(|span| CallSpan::Owned(Box::new(span))),
                    ip: 0,
                    stack_base: base,
                    locals,
                    provided: self.frame_provided(provided),
                    scope_depth: 1,
                    scopes: Vec::new(),
                    cleanup_action: false,
                    cleanup_recovers: false,
                    cleanup_owner_depth: None,
                });
            }
            Value::Native(function) => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result = self.invoke_native(&function, &arguments, None, span.as_ref())?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::DeclaredNative {
                function,
                resource_signature,
                ..
            } => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result = self.invoke_native(
                    &function,
                    &arguments,
                    Some(&resource_signature),
                    span.as_ref(),
                )?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::Builtin(builtin) => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result = self.call_builtin(builtin, program, &arguments, span.clone())?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::Overloads(overloads) => {
                let positional = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let (callee, arguments, provided) = self.bind_overload_arguments(
                    program,
                    &overloads,
                    &positional,
                    &[],
                    span.clone(),
                )?;
                self.stack.truncate(base);
                self.stack.push(callee);
                self.stack.extend(arguments);
                let count = self.stack.len() - base - 1;
                return self.call(program, count, Some(provided), span);
            }
            value => {
                return Err(self.error(
                    RuntimeErrorKind::InvalidCall,
                    format!("cannot call {}", value.type_name()),
                    span,
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn call_at(
        &mut self,
        program: &Program,
        count: usize,
        provided: Option<Vec<bool>>,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let required = count.checked_add(1).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span,
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span,
            )
        })?;
        let callee = self.stack[base]
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
        match callee {
            Value::Closure(closure) => {
                let frame_program = closure
                    .program
                    .clone()
                    .or_else(|| self.module_program.clone())
                    .unwrap_or_else(|| Rc::new(program.clone()));
                let chunk = frame_program.chunk(closure.chunk).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "closure references missing chunk".into(),
                        span,
                    )
                })?;
                if chunk.arity != count {
                    return Err(self.error_at(
                        RuntimeErrorKind::Arity,
                        format!(
                            "`{}` expects {} arguments, got {count}",
                            chunk.name, chunk.arity
                        ),
                        span,
                    ));
                }
                if chunk.locals < chunk.arity {
                    return Err(self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        format!(
                            "function `{}` has {} local slots for {} parameters",
                            chunk.name, chunk.locals, chunk.arity
                        ),
                        span,
                    ));
                }
                self.record_closure_argument_vector();
                let locals = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value
                            .resolve()
                            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let argument_count = locals.len();
                let locals = frame_locals(locals, chunk.locals);
                self.record_frame_locals(locals.capacity(), argument_count);
                #[cfg(feature = "metrics")]
                self.record_frame(chunk.locals);
                self.frames.push(Frame {
                    program: frame_program.clone(),
                    globals: closure
                        .globals
                        .clone()
                        .unwrap_or_else(|| self.globals.clone()),
                    closure,
                    call_span: self.frame_call_span(span),
                    ip: 0,
                    stack_base: base,
                    locals,
                    provided: self.frame_provided(provided),
                    scope_depth: 1,
                    scopes: Vec::new(),
                    cleanup_action: false,
                    cleanup_recovers: false,
                    cleanup_owner_depth: None,
                });
            }
            Value::Native(function) => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value
                            .resolve()
                            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result = self.invoke_native(&function, &arguments, None, span)?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::DeclaredNative {
                function,
                resource_signature,
                ..
            } => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value
                            .resolve()
                            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result =
                    self.invoke_native(&function, &arguments, Some(&resource_signature), span)?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::Builtin(builtin) => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value
                            .resolve()
                            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result =
                    self.call_builtin(builtin, program, &arguments, self.owned_span(span))?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            Value::Overloads(overloads) => {
                let positional = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value
                            .resolve()
                            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let (callee, arguments, provided) =
                    self.bind_overload_arguments_at(program, &overloads, &positional, &[], span)?;
                self.stack.truncate(base);
                self.stack.push(callee);
                self.stack.extend(arguments);
                let count = self.stack.len() - base - 1;
                return self.call_at(program, count, Some(provided), span);
            }
            value => {
                return Err(self.error_at(
                    RuntimeErrorKind::InvalidCall,
                    format!("cannot call {}", value.type_name()),
                    span,
                ));
            }
        }
        Ok(())
    }

    #[cfg(feature = "concurrency")]
    #[allow(clippy::needless_pass_by_value)]
    fn module_closure_vm(
        &self,
        program: Rc<Program>,
        closure: Rc<Closure>,
        arguments: Vec<Value>,
        provided: Option<Vec<bool>>,
        span: Option<SourceSpan>,
        options: ClosureCallOptions,
    ) -> VmResult<Vm> {
        let chunk = program.chunk(closure.chunk).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "closure references missing chunk".into(),
                span.clone(),
            )
        })?;
        if chunk.arity != arguments.len() {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!(
                    "`{}` expects {} arguments, got {}",
                    chunk.name,
                    chunk.arity,
                    arguments.len()
                ),
                span,
            ));
        }
        let mut vm = Self {
            module_loader: self.module_loader.clone(),
            module_program: Some(program.clone()),
            globals: closure
                .globals
                .clone()
                .unwrap_or_else(|| self.globals.clone()),
            imported_globals: HashSet::new(),
            module_metadata: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            cleanup: Vec::new(),
            progress: self.progress.clone(),
            #[cfg(feature = "concurrency")]
            nursery: options.nursery,
            #[cfg(feature = "concurrency")]
            direct_task_limit: options.direct_task_limit,
            #[cfg(feature = "concurrency")]
            direct_task_count: options.direct_task_count,
            native_resources: self.native_resources.clone(),
            current_waiter: None,
            suspension: None,
            resume: None,
            wait_registration: None,
            host_execution: None,
            active_span: None,
            shutdown: false,
            #[cfg(feature = "metrics")]
            metrics: self.metrics.clone(),
        };
        let argument_count = arguments.len();
        let locals = frame_locals(arguments, chunk.locals);
        vm.record_frame_locals(locals.capacity(), argument_count);
        #[cfg(feature = "metrics")]
        vm.record_frame(chunk.locals);
        vm.frames.push(Frame {
            program: program.clone(),
            globals: vm.globals.clone(),
            closure,
            call_span: span.map(|span| CallSpan::Owned(Box::new(span))),
            ip: 0,
            stack_base: 0,
            locals,
            provided: self.frame_provided(provided),
            scope_depth: 1,
            scopes: Vec::new(),
            cleanup_action: false,
            cleanup_recovers: false,
            cleanup_owner_depth: None,
        });
        Ok(vm)
    }

    #[cfg(feature = "concurrency")]
    fn spawn_task_at(&mut self, _program: &Program, span: Option<&SourceSpan>) -> VmResult<()> {
        let closure = self.pop_at(span)?;
        let Value::Closure(closure) = closure else {
            return Err(self.error_at(
                RuntimeErrorKind::Type,
                "spawn expects a function or block".into(),
                span,
            ));
        };
        let admission = if let Some(limit) = self.direct_task_limit {
            let count = self.direct_task_count.as_ref().ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "limited nursery is missing its task counter".into(),
                    span,
                )
            })?;
            Some(TaskAdmission {
                limit,
                count: count.clone(),
            })
        } else {
            None
        };
        let captures = closure
            .captures
            .iter()
            .zip(&closure.capture_sources)
            .map(|(cell, source)| match source {
                Capture::Local(_) => binding_cell(cell.borrow().clone()),
                Capture::Capture(_) => cell.clone(),
            })
            .collect();
        let closure = Rc::new(Closure {
            chunk: closure.chunk,
            captures,
            program: closure.program.clone(),
            globals: closure.globals.clone(),
            capture_sources: closure.capture_sources.clone(),
        });
        let task_program = closure.program.clone().unwrap_or(self.active_program()?);
        let vm = self.module_closure_vm(
            task_program.clone(),
            closure,
            Vec::new(),
            None,
            self.owned_span(span),
            ClosureCallOptions {
                direct_task_limit: None,
                direct_task_count: None,
                nursery: self.nursery.clone(),
            },
        )?;
        let execution = TaskExecution {
            vm,
            settle_nursery: false,
            interactive_imported_globals: None,
        };
        let task = Rc::new(Task::pending(
            execution,
            admission,
            self.nursery.ready_queue(),
        ));
        self.nursery.add_task(task.clone());
        self.stack.push(Value::Task(task));
        Ok(())
    }

    fn make_progress(&self) -> bool {
        self.progress.make_available_progress() || {
            #[cfg(feature = "concurrency")]
            {
                self.nursery.make_progress()
            }
            #[cfg(not(feature = "concurrency"))]
            {
                self.progress.wait_for_progress()
            }
        }
    }

    #[cfg(feature = "concurrency")]
    fn settle_tasks(&self, result: &VmResult<Value>) -> VmResult<Value> {
        let cancellation = self.error(
            RuntimeErrorKind::Thrown,
            "sibling cancelled due to fail-fast".into(),
            None,
        );
        let blocked = self.error(
            RuntimeErrorKind::InvalidCall,
            "task remains blocked with no runnable work".into(),
            None,
        );
        self.nursery.settle(result, &cancellation, &blocked)
    }

    #[cfg(feature = "concurrency")]
    fn settle_tasks_available(&self, result: &VmResult<Value>) -> Option<VmResult<Value>> {
        let cancellation = self.error(
            RuntimeErrorKind::Thrown,
            "sibling cancelled due to fail-fast".into(),
            None,
        );
        self.nursery.settle_available(result, &cancellation)
    }

    #[cfg(feature = "concurrency")]
    fn run_nursery_at(
        &mut self,
        _program: &Program,
        has_limit: bool,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let closure = self.pop_at(span)?;
        let limit = has_limit.then(|| self.pop_at(span)).transpose()?;
        let limit = if let Some(limit) = limit {
            let Value::Int(limit) = limit else {
                return Err(self.error_at(
                    RuntimeErrorKind::Type,
                    "nursery limit expects an integer".into(),
                    span,
                ));
            };
            if limit < 0 {
                return Err(self.error_at(
                    RuntimeErrorKind::Type,
                    "nursery limit must not be negative".into(),
                    span,
                ));
            }
            if limit == 0 {
                return Err(self.error_at(
                    RuntimeErrorKind::Type,
                    "nursery limit must be positive".into(),
                    span,
                ));
            }
            Some(usize::try_from(limit).map_err(|_| {
                self.error_at(
                    RuntimeErrorKind::Type,
                    "nursery limit is too large".into(),
                    span,
                )
            })?)
        } else {
            None
        };
        let Value::Closure(closure) = closure else {
            return Err(self.error_at(
                RuntimeErrorKind::Type,
                "nursery expects a function or block".into(),
                span,
            ));
        };
        let nursery = Rc::new(Nursery::explicit(
            self.progress.clone(),
            #[cfg(feature = "metrics")]
            self.metrics.clone(),
        ));
        let task_program = closure.program.clone().unwrap_or(self.active_program()?);
        let vm = self.module_closure_vm(
            task_program.clone(),
            closure,
            Vec::new(),
            None,
            self.owned_span(span),
            ClosureCallOptions {
                direct_task_limit: limit,
                direct_task_count: limit.map(|_| Rc::new(Cell::new(0))),
                nursery: nursery.clone(),
            },
        )?;
        let execution = TaskExecution {
            vm,
            settle_nursery: true,
            interactive_imported_globals: None,
        };
        let body = Rc::new(Task::pending(execution, None, nursery.ready_queue()));
        nursery.enqueue(body.clone());
        nursery.run_task(&body);
        if body.is_pending() {
            let blocked = self.error_at(
                RuntimeErrorKind::InvalidCall,
                "task remains blocked with no runnable work".into(),
                span,
            );
            nursery.cancel_all(&blocked);
            body.cancel(&blocked);
            return Err(blocked);
        }
        let value = body.outcome().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "nursery body settled without an outcome".into(),
                span,
            )
        })??;
        self.stack.push(value);
        Ok(())
    }

    fn warning(&self, message: String) {
        if let Some(loader) = &self.module_loader {
            loader.warn(message);
        }
    }

    fn import_at(&mut self, kinds: &[CallArgumentKind], span: Option<&SourceSpan>) -> VmResult<()> {
        let values = self.pop_values_at(kinds.len(), span)?;
        let (names, named_arguments) = self.expand_call_arguments_at(values, kinds, span)?;
        if !named_arguments.is_empty() {
            return Err(self.error_at(
                RuntimeErrorKind::Arity,
                "import does not accept named arguments".into(),
                span,
            ));
        }
        if names.is_empty() {
            return Err(self.error_at(
                RuntimeErrorKind::Arity,
                "import expects at least one module name".into(),
                span,
            ));
        }
        let loader = self.module_loader.clone().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::Module,
                "module loader is not configured".into(),
                span,
            )
        })?;
        let importer = span
            .or_else(|| self.active_span())
            .map(|span| Path::new(span.path.as_ref()));
        let mut exports = Vec::new();
        for name in names {
            let Value::Str(name) = name else {
                return Err(self.error_at(
                    RuntimeErrorKind::Type,
                    format!(
                        "import expects string module names, got {}",
                        name.type_name()
                    ),
                    span,
                ));
            };
            let instance = loader.initialize(importer, &name).map_err(|error| {
                self.error_at(RuntimeErrorKind::Module, error.to_string(), span)
            })?;
            let Value::Map(module_exports) = instance.live_exports else {
                return Err(self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "module exports are not a map".into(),
                    span,
                ));
            };
            for (key, value) in module_exports.iter() {
                let Some(index) = exports.iter().position(|(existing, _)| existing == key) else {
                    exports.push((key.clone(), value.clone()));
                    continue;
                };
                let existing = exports[index].1.clone();
                if let (Some(existing_signatures), Some(incoming_signature)) = (
                    Self::callable_signatures(&existing),
                    Self::callable_signature(value),
                ) {
                    if existing_signatures
                        .iter()
                        .any(|signature| signature == &incoming_signature)
                    {
                        if let Value::Str(name) = key {
                            self.warning(format!(
                                "imported callable `{name}` with a duplicate signature was ignored because an earlier module provided it"
                            ));
                        }
                    } else {
                        let mut overloads = match existing {
                            Value::Overloads(overloads) => overloads.as_ref().clone(),
                            value => vec![value],
                        };
                        overloads.push(value.clone());
                        exports[index].1 = Value::Overloads(Rc::new(overloads));
                    }
                } else if let Value::Str(name) = key {
                    self.warning(format!(
                        "imported binding `{name}` was ignored because an earlier module provided it"
                    ));
                }
            }
        }
        self.stack.push(Value::Map(Map::new(exports).into_shared()));
        Ok(())
    }

    fn callable_signature(value: &Value) -> Option<CallableRuntimeSignature> {
        match value.resolve().ok()? {
            Value::Closure(closure) => {
                let program = closure.program.as_deref()?;
                program
                    .chunk(closure.chunk)
                    .map(|chunk| CallableRuntimeSignature {
                        identity: chunk
                            .callable_identity
                            .and_then(|identity| program.callable_identity(identity))
                            .cloned(),
                        shape: chunk
                            .parameters
                            .iter()
                            .map(|parameter| (parameter.has_default, parameter.variadic))
                            .collect(),
                    })
            }
            Value::DeclaredNative {
                callable_identity, ..
            } => Some(CallableRuntimeSignature {
                identity: Some(callable_identity),
                shape: Vec::new(),
            }),
            _ => None,
        }
    }

    fn callable_signatures(value: &Value) -> Option<Vec<CallableRuntimeSignature>> {
        match value {
            Value::Overloads(overloads) => overloads.iter().map(Self::callable_signature).collect(),
            value => Self::callable_signature(value).map(|signature| vec![signature]),
        }
    }

    fn entrypoint_callable(&self, value: Value, identity: &CallableIdentity) -> VmResult<Value> {
        match value {
            Value::Overloads(overloads) => overloads
                .iter()
                .find(|candidate| {
                    Self::callable_signature(candidate)
                        .and_then(|signature| signature.identity)
                        .as_ref()
                        == Some(identity)
                })
                .cloned()
                .ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        "selected program entrypoint is no longer present in the live `main` binding"
                            .into(),
                        None,
                    )
                }),
            value
                if Self::callable_signature(&value)
                    .and_then(|signature| signature.identity)
                    .as_ref()
                    == Some(identity) =>
            {
                Ok(value)
            }
            _ => Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "selected program entrypoint is no longer present in the live `main` binding".into(),
                None,
            )),
        }
    }

    fn list_spread_at(&mut self, spreads: &[bool], span: Option<&SourceSpan>) -> VmResult<()> {
        let values = self.pop_values_at(spreads.len(), span)?;
        let mut result = Vec::new();
        for (value, spread) in values.into_iter().zip(spreads) {
            if *spread {
                let Value::List(values) = value else {
                    return Err(self.error_at(
                        RuntimeErrorKind::Type,
                        "list spread expects a list".into(),
                        span,
                    ));
                };
                result.extend(values.iter().cloned());
            } else {
                result.push(value);
            }
        }
        self.stack
            .push(Value::List(List::from_values(result).into_shared()));
        Ok(())
    }

    fn call_spread_at(
        &mut self,
        program: &Program,
        kinds: &[CallArgumentKind],
        selected: Option<&CallableIdentity>,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        if kinds
            .iter()
            .all(|kind| matches!(kind, CallArgumentKind::Positional))
            && self.call_positional_closure_at(program, kinds.len(), selected, span)?
        {
            #[cfg(feature = "metrics")]
            self.record_exact_positional_closure_call();
            return Ok(());
        }
        #[cfg(feature = "metrics")]
        self.record_generic_call_argument_binding();
        let required = kinds.len().checked_add(1).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span,
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span,
            )
        })?;
        let callee = self.stack[base]
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
        let values = self.stack.split_off(base + 1);
        self.stack.truncate(base);
        let (positional, named) = self.expand_call_arguments_at(values, kinds, span)?;
        let (callee, arguments, provided) = if let Value::Overloads(overloads) = &callee {
            if let Some(selected) = selected {
                self.bind_selected_overload_arguments_at(
                    program,
                    overloads,
                    selected,
                    &positional,
                    &named,
                    span,
                )?
            } else {
                self.bind_overload_arguments_at(program, overloads, &positional, &named, span)?
            }
        } else {
            if let Some(selected) = selected
                && Self::callable_signature(&callee)
                    .and_then(|signature| signature.identity)
                    .as_ref()
                    != Some(selected)
            {
                return Err(self.error_at(
                    RuntimeErrorKind::InvalidCall,
                    "selected callable signature is no longer present in the live binding".into(),
                    span,
                ));
            }
            let (arguments, provided) =
                self.bind_call_arguments_at(program, &callee, positional, named, span)?;
            (callee, arguments, provided)
        };
        self.stack.push(callee);
        self.stack.extend(arguments);
        let count = self.stack.len() - base - 1;
        self.call_at(program, count, Some(provided), span)
    }

    /// Handles a source call whose compiler-provided argument shape is exactly
    /// positional, retaining the generic binder only for closures that need it.
    fn call_positional_at(
        &mut self,
        program: &Program,
        count: usize,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        if self.call_positional_closure_at(program, count, None, span)? {
            #[cfg(feature = "metrics")]
            self.record_exact_positional_closure_call();
            return Ok(());
        }
        #[cfg(feature = "metrics")]
        self.record_generic_call_argument_binding();
        let required = count.checked_add(1).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span,
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span,
            )
        })?;
        let callee = self.stack[base]
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
        let positional = self.stack.split_off(base + 1);
        self.stack.truncate(base);
        let positional = positional
            .into_iter()
            .map(|value| {
                value
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
            })
            .collect::<VmResult<Vec<_>>>()?;
        let (callee, arguments, provided) = if let Value::Overloads(overloads) = &callee {
            self.bind_overload_arguments_at(program, overloads, &positional, &[], span)?
        } else {
            let (arguments, provided) =
                self.bind_call_arguments_at(program, &callee, positional, Vec::new(), span)?;
            (callee, arguments, provided)
        };
        self.stack.push(callee);
        self.stack.extend(arguments);
        let count = self.stack.len() - base - 1;
        self.call_at(program, count, Some(provided), span)
    }

    /// Starts an exact positional closure call without constructing either the
    /// generic argument-binding intermediates or a temporary argument vector.
    ///
    /// A selected call still verifies the identity against the current live
    /// callee value before this path can run.
    fn call_positional_closure_at(
        &mut self,
        program: &Program,
        count: usize,
        selected: Option<&CallableIdentity>,
        span: Option<&SourceSpan>,
    ) -> VmResult<bool> {
        let required = count.checked_add(1).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span,
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span,
            )
        })?;
        let callee = self.stack[base]
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
        let callee = match callee {
            Value::Overloads(overloads) => selected.and_then(|selected| {
                overloads.iter().find_map(|candidate| {
                    let candidate = candidate.resolve().ok()?;
                    (Self::callable_signature(&candidate)
                        .and_then(|signature| signature.identity)
                        .as_ref()
                        == Some(selected))
                    .then_some(candidate)
                })
            }),
            candidate => match selected {
                None => Some(candidate),
                Some(selected) => (Self::callable_signature(&candidate)
                    .and_then(|signature| signature.identity)
                    .as_ref()
                    == Some(selected))
                .then_some(candidate),
            },
        };
        let Some(Value::Closure(closure)) = callee else {
            return Ok(false);
        };
        let frame_program = closure
            .program
            .clone()
            .or_else(|| self.module_program.clone())
            .unwrap_or_else(|| Rc::new(program.clone()));
        let local_count = {
            let chunk = frame_program.chunk(closure.chunk).ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "closure references missing chunk".into(),
                    span,
                )
            })?;
            if chunk.arity != count || !chunk.exact_positional_parameters {
                return Ok(false);
            }
            chunk.locals
        };
        let mut locals = Vec::with_capacity(local_count);
        for value in &self.stack[base + 1..] {
            let value = value
                .resolve()
                .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
            locals.push(LocalSlot::Direct(value));
        }
        locals.resize_with(local_count, || LocalSlot::Direct(Value::Nil));
        self.record_frame_locals(locals.capacity(), count);
        self.record_exact_positional_stack_local_initialization(count);
        #[cfg(feature = "metrics")]
        self.record_frame(local_count);
        self.frames.push(Frame {
            program: frame_program,
            globals: closure
                .globals
                .clone()
                .unwrap_or_else(|| self.globals.clone()),
            closure,
            call_span: self.frame_call_span(span),
            ip: 0,
            stack_base: base,
            locals,
            provided: ProvidedArguments::All,
            scope_depth: 1,
            scopes: Vec::new(),
            cleanup_action: false,
            cleanup_recovers: false,
            cleanup_owner_depth: None,
        });
        Ok(true)
    }

    fn pipeline_call_at(
        &mut self,
        program: &Program,
        kinds: &[CallArgumentKind],
        selected: Option<&CallableIdentity>,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let required = kinds.len().checked_add(2).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline argument count is too large".into(),
                span,
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span,
            )
        })?;
        let arguments = self.stack.split_off(base + 2);
        let callee = self.stack.pop().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span,
            )
        })?;
        let value = self.stack.pop().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span,
            )
        })?;
        self.stack.push(callee);
        self.stack.push(value);
        self.stack.extend(arguments);
        let mut all_kinds = Vec::with_capacity(kinds.len() + 1);
        all_kinds.push(CallArgumentKind::Positional);
        all_kinds.extend_from_slice(kinds);
        self.call_spread_at(program, &all_kinds, selected, span)
    }

    fn expand_call_arguments_at(
        &self,
        values: Vec<Value>,
        kinds: &[CallArgumentKind],
        span: Option<&SourceSpan>,
    ) -> VmResult<ExpandedCallArguments> {
        let mut positional = Vec::new();
        let mut named = Vec::new();
        for (value, kind) in values.into_iter().zip(kinds) {
            let value = value
                .resolve()
                .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
            match kind {
                CallArgumentKind::Positional => positional.push(value),
                CallArgumentKind::Spread => {
                    let Value::List(values) = value else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            "call spread expects a list".into(),
                            span,
                        ));
                    };
                    positional.extend(values.iter().cloned());
                }
                CallArgumentKind::Named(name) => named.push((name.clone(), value)),
            }
        }
        Ok((positional, named))
    }

    fn call_builtin(
        &mut self,
        builtin: Builtin,
        program: &Program,
        arguments: &[Value],
        span: Option<SourceSpan>,
    ) -> VmResult<Value> {
        match builtin {
            Builtin::Cfg => {
                let configuration = self.configuration(span.clone())?;
                if arguments.len() != 2 {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!("`cfg` expects 2 arguments, got {}", arguments.len()),
                        span,
                    ));
                }
                let Value::Str(key) = &arguments[0] else {
                    return Err(self.error(
                        RuntimeErrorKind::Type,
                        format!("cfg key expects str, got {}", arguments[0].type_name()),
                        span,
                    ));
                };
                let key = if key.contains('.') || program.module_name().is_empty() {
                    key.to_string()
                } else {
                    format!("{}.{}", program.module_name(), key)
                };
                Ok(configuration.resolve(&key, &arguments[1]))
            }
            Builtin::Stacktrace => {
                if arguments.len() != 1 {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!("`stacktrace` expects 1 argument, got {}", arguments.len()),
                        span,
                    ));
                }
                let active = self.active_error().ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidCall,
                        "`stacktrace` is only valid while handling an active error".into(),
                        span.clone(),
                    )
                })?;
                if arguments[0] != Self::error_value(active.clone()) {
                    return Err(self.error(
                        RuntimeErrorKind::InvalidCall,
                        "`stacktrace` expects the active error".into(),
                        span,
                    ));
                }
                Ok(Value::string(render_stacktrace(&active)))
            }
        }
    }

    fn configuration(&self, span: Option<SourceSpan>) -> VmResult<&crate::Configuration> {
        self.module_loader
            .as_ref()
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::Module,
                    "configuration service is not configured".into(),
                    span,
                )
            })
            .map(ModuleLoader::configuration)
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_lines)]
    fn select_at(&mut self, cases: &[SelectCase], span: Option<&SourceSpan>) -> VmResult<()> {
        #[cfg(not(feature = "concurrency"))]
        if cases
            .iter()
            .any(|case| matches!(case, SelectCase::After { .. } | SelectCase::Await { .. }))
        {
            return Err(self.runtime_capability_error("select timer or task-await", span));
        }
        if cases.is_empty() {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidCall,
                "select requires at least one case".into(),
                span,
            ));
        }
        let mut values = Vec::with_capacity(cases.len());
        for case in cases.iter().rev() {
            let has_handler = match case {
                SelectCase::Receive { has_handler }
                | SelectCase::Send { has_handler }
                | SelectCase::After { has_handler }
                | SelectCase::Await { has_handler }
                | SelectCase::Default { has_handler } => *has_handler,
            };
            let handler = has_handler.then(|| self.pop_at(span)).transpose()?;
            let value = match case {
                SelectCase::Receive { .. } => {
                    let value = self.pop_at(span)?;
                    let Value::Channel(channel) = value else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("select recv expects chan, got {}", value.type_name()),
                            span,
                        ));
                    };
                    RuntimeSelectCase::Receive { channel, handler }
                }
                SelectCase::Send { .. } => {
                    let value = self.pop_at(span)?;
                    let channel = self.pop_at(span)?;
                    let Value::Channel(channel) = channel else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("select send expects chan, got {}", channel.type_name()),
                            span,
                        ));
                    };
                    if matches!(value, Value::Nil) {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            "send cannot send nil".into(),
                            span,
                        ));
                    }
                    RuntimeSelectCase::Send {
                        channel,
                        value,
                        handler,
                    }
                }
                #[cfg(feature = "concurrency")]
                SelectCase::After { .. } => {
                    let duration = self.pop_at(span)?;
                    let Value::Int(milliseconds) = duration else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("select after expects num, got {}", duration.type_name()),
                            span,
                        ));
                    };
                    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
                        self.error_at(
                            RuntimeErrorKind::Type,
                            "select after must not be negative or too large".into(),
                            span,
                        )
                    })?;
                    RuntimeSelectCase::After {
                        deadline: Instant::now()
                            .checked_add(Duration::from_millis(milliseconds))
                            .ok_or_else(|| {
                                self.error_at(
                                    RuntimeErrorKind::Type,
                                    "select after is too large".into(),
                                    span,
                                )
                            })?,
                        handler,
                    }
                }
                #[cfg(not(feature = "concurrency"))]
                SelectCase::After { .. } => {
                    return Err(self.runtime_capability_error("select timer", span));
                }
                #[cfg(feature = "concurrency")]
                SelectCase::Await { .. } => {
                    let value = self.pop_at(span)?;
                    let Value::Task(task) = value else {
                        return Err(self.error_at(
                            RuntimeErrorKind::Type,
                            format!("select await expects task, got {}", value.type_name()),
                            span,
                        ));
                    };
                    RuntimeSelectCase::Await { task, handler }
                }
                #[cfg(not(feature = "concurrency"))]
                SelectCase::Await { .. } => {
                    return Err(self.runtime_capability_error("select task-await", span));
                }
                SelectCase::Default { .. } => RuntimeSelectCase::Default { handler },
            };
            values.push(value);
        }
        values.reverse();
        let mut default = None;
        for case in &values {
            match case {
                RuntimeSelectCase::Receive { channel, handler } => {
                    self.progress.track_native_channel(channel);
                    if let ChannelReceive::Ready(value) = channel.try_receive() {
                        self.push_select_result(value, handler.clone());
                        return Ok(());
                    }
                }
                RuntimeSelectCase::Send {
                    channel,
                    value,
                    handler,
                } => {
                    self.progress.track_native_channel(channel);
                    match channel.try_send(value.clone()) {
                        ChannelSend::Ready => {
                            self.push_select_result(Value::Nil, handler.clone());
                            return Ok(());
                        }
                        ChannelSend::Closed => {
                            return Err(self.error_at(
                                RuntimeErrorKind::InvalidCall,
                                "send on a closed channel".into(),
                                span,
                            ));
                        }
                        ChannelSend::Pending => {}
                    }
                }
                #[cfg(feature = "concurrency")]
                RuntimeSelectCase::Await { task, handler } => {
                    if task.is_running() {
                        return Err(self.error_at(
                            RuntimeErrorKind::InvalidCall,
                            "task cannot await itself while it is running".into(),
                            span,
                        ));
                    }
                    if let Some(outcome) = task.outcome() {
                        task.observe();
                        self.push_select_result(outcome?, handler.clone());
                        return Ok(());
                    }
                }
                #[cfg(feature = "concurrency")]
                RuntimeSelectCase::After { deadline, handler } => {
                    if *deadline <= Instant::now() {
                        self.push_select_result(Value::Nil, handler.clone());
                        return Ok(());
                    }
                }
                RuntimeSelectCase::Default { handler } => default = Some(handler.clone()),
            }
        }
        if let Some(handler) = default {
            self.push_select_result(Value::Nil, handler);
            return Ok(());
        }
        let base = self.current_waiter_at(span)?;
        let select_state = WaitSet::select_state(base.clone());
        let mut registrations = Vec::new();
        for case in values {
            match case {
                RuntimeSelectCase::Receive { channel, handler } => {
                    channel.park_receiver(Waiter::select(
                        select_state.clone(),
                        SelectWake::Value { handler },
                    ));
                    registrations.push(WaitRegistration::ChannelReceive(channel));
                }
                RuntimeSelectCase::Send {
                    channel,
                    value,
                    handler,
                } => {
                    let waiter =
                        Waiter::select(select_state.clone(), SelectWake::Value { handler });
                    waiter.set_closed_send_error(self.error_at(
                        RuntimeErrorKind::InvalidCall,
                        "send on a closed channel".into(),
                        span,
                    ));
                    channel.park_sender(waiter, value);
                    registrations.push(WaitRegistration::ChannelSend(channel));
                }
                #[cfg(feature = "concurrency")]
                RuntimeSelectCase::Await { task, handler } => {
                    task.wait_for(Waiter::select(
                        select_state.clone(),
                        SelectWake::TaskAwait {
                            handler,
                            observer: task.observer(),
                        },
                    ));
                    registrations.push(WaitRegistration::TaskAwait(task));
                }
                #[cfg(feature = "concurrency")]
                RuntimeSelectCase::After { deadline, handler } => {
                    let timers = self.nursery.timer_service();
                    let timer = timers.borrow_mut().register(
                        deadline,
                        Waiter::select(select_state.clone(), SelectWake::Value { handler }),
                    );
                    registrations.push(WaitRegistration::Timer(timers::TimerRegistration::new(
                        timers,
                        timer,
                        #[cfg(feature = "metrics")]
                        self.metrics.clone(),
                    )));
                }
                RuntimeSelectCase::Default { .. } => {}
            }
        }
        let registrations = WaitSet::many(
            registrations,
            #[cfg(feature = "metrics")]
            self.metrics.clone(),
        );
        WaitSet::set_select_registrations(&select_state, registrations.clone());
        self.wait_registration = Some(registrations);
        self.stack.push(Value::Nil);
        self.suspension = Some(Suspension::Select {
            #[cfg(feature = "concurrency")]
            span: self.owned_span(span),
        });
        Ok(())
    }

    fn push_select_result(&mut self, value: Value, handler: Option<Value>) {
        self.stack.push(Value::List(
            vec![value, handler.unwrap_or(Value::Nil)].into(),
        ));
    }

    fn select_apply_at(&mut self, program: &Program, span: Option<&SourceSpan>) -> VmResult<()> {
        let selected = self.pop_at(span)?;
        let Value::List(values) = selected else {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "select result is invalid".into(),
                span,
            ));
        };
        if values.len() != 2 {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "select result has invalid arity".into(),
                span,
            ));
        }
        let value = values[0].clone();
        let handler = values[1].clone();
        if matches!(handler, Value::Nil) {
            self.stack.push(value);
        } else {
            self.stack.push(handler);
            self.stack.push(value);
            self.call_at(program, 1, None, span)?;
        }
        Ok(())
    }

    fn current_waiter_at(&self, span: Option<&SourceSpan>) -> VmResult<Waiter> {
        self.current_waiter.clone().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidCall,
                "blocking operations require scheduler-owned execution".into(),
                span,
            )
        })
    }

    pub(super) fn invoke_native(
        &mut self,
        function: &NativeFunction,
        arguments: &[Value],
        resource_signature: Option<&crate::source::environment::ForeignResourceSignature>,
        span: Option<&SourceSpan>,
    ) -> VmResult<Value> {
        if let Some(signature) = resource_signature {
            self.validate_foreign_resource_arguments(function, signature, arguments, span)?;
        }
        match function.invoke(arguments) {
            NativeInvocation::Result(value, resources) => {
                self.native_resources.register(resources);
                if let Some(signature) = resource_signature {
                    self.validate_foreign_resource_result(function, signature, &value, span)?;
                }
                if let Value::Channel(channel) = &value
                    && channel.has_native_producer()
                {
                    self.progress.track_native_channel(channel);
                }
                Ok(value)
            }
            NativeInvocation::Error(error, resources) => {
                self.native_resources.register(resources);
                let (code, message, data) = error.into_parts();
                let mut error = self.error_at(
                    RuntimeErrorKind::Native,
                    format!("native `{}`: {message}", function.qualified_name()),
                    span,
                );
                error.native = Some(Box::new(NativeErrorDetails { code, data }));
                Err(error)
            }
            NativeInvocation::ContractViolation(message) => {
                Err(self.error_at(RuntimeErrorKind::NativeContract, message, span))
            }
        }
    }

    fn validate_foreign_resource_arguments(
        &self,
        function: &NativeFunction,
        signature: &crate::source::environment::ForeignResourceSignature,
        arguments: &[Value],
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        for (index, value) in arguments.iter().enumerate() {
            let Some(expected) = signature.parameter_identity(index) else {
                continue;
            };
            self.validate_foreign_resource_value(function, expected, value, "argument", span)?;
        }
        Ok(())
    }

    fn validate_foreign_resource_result(
        &self,
        function: &NativeFunction,
        signature: &crate::source::environment::ForeignResourceSignature,
        value: &Value,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let Some(expected) = signature.result_identity() else {
            return Ok(());
        };
        self.validate_foreign_resource_value(function, expected, value, "result", span)
    }

    fn validate_foreign_resource_value(
        &self,
        function: &NativeFunction,
        expected: (Option<&str>, &str),
        value: &Value,
        position: &str,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let expected_module = expected.0.unwrap_or_else(|| function.module_name());
        let Value::NativeResource(resource) = value else {
            return Err(self.error_at(
                RuntimeErrorKind::NativeContract,
                format!(
                    "native `{}` returned a non-resource {position} where `{}.{}` is declared",
                    function.qualified_name(),
                    expected_module,
                    expected.1
                ),
                span,
            ));
        };
        if resource.has_type_in_scope(function.resource_scope_id(), expected_module, expected.1) {
            return Ok(());
        }
        Err(self.error_at(
            RuntimeErrorKind::NativeContract,
            format!(
                "native `{}` returned the wrong resource type for its {position}; expected `{}.{}`",
                function.qualified_name(),
                expected_module,
                expected.1
            ),
            span,
        ))
    }

    fn bind_call_arguments(
        &self,
        program: &Program,
        callee: &Value,
        mut positional: Vec<Value>,
        named: Vec<(String, Value)>,
        span: Option<SourceSpan>,
    ) -> VmResult<(Vec<Value>, Vec<bool>)> {
        let Value::Closure(closure) = callee else {
            if named.is_empty() {
                return Ok((positional.clone(), vec![true; positional.len()]));
            }
            return Err(self.error(
                RuntimeErrorKind::Arity,
                "native functions do not accept named arguments".into(),
                span,
            ));
        };
        let closure_program = closure.program.as_deref().unwrap_or(program);
        let chunk = closure_program.chunk(closure.chunk).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "closure references missing chunk".into(),
                span.clone(),
            )
        })?;
        if chunk.parameters.is_empty() {
            if chunk.arity != 0 {
                return Ok((positional.clone(), vec![true; positional.len()]));
            }
            if positional.is_empty() && named.is_empty() {
                return Ok((Vec::new(), Vec::new()));
            }
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`{}` expects no arguments", chunk.name),
                span,
            ));
        }
        let variadic = chunk
            .parameters
            .last()
            .filter(|parameter| parameter.variadic);
        let fixed = chunk.parameters.len() - usize::from(variadic.is_some());
        if positional.len() > chunk.parameters.len() && variadic.is_none() {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`{}` received too many positional arguments", chunk.name),
                span,
            ));
        }
        let rest = positional.split_off(fixed.min(positional.len()));
        let mut bound = positional.into_iter().map(Some).collect::<Vec<_>>();
        bound.resize_with(chunk.parameters.len(), || None);
        for (name, value) in named {
            let slot = chunk
                .parameters
                .iter()
                .position(|parameter| parameter.name == name)
                .ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::Name,
                        format!("unknown parameter `{name}`"),
                        span.clone(),
                    )
                })?;
            if bound[slot].is_some() {
                return Err(self.error(
                    RuntimeErrorKind::Arity,
                    format!("parameter `{name}` was assigned more than once"),
                    span,
                ));
            }
            if chunk.parameters[slot].variadic && !matches!(value, Value::List(_)) {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    format!("variadic parameter `{name}` expects a list"),
                    span,
                ));
            }
            bound[slot] = Some(value);
        }
        if variadic.is_some() && bound[fixed].is_none() {
            bound[fixed] = Some(Value::List(List::from_values(rest).into_shared()));
        }
        let provided = bound.iter().map(Option::is_some).collect::<Vec<_>>();
        let values = bound
            .into_iter()
            .enumerate()
            .map(|(slot, value)| {
                value
                    .or_else(|| chunk.parameters[slot].has_default.then_some(Value::Nil))
                    .ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::Arity,
                            format!(
                                "missing required parameter `{}`",
                                chunk.parameters[slot].name
                            ),
                            span.clone(),
                        )
                    })
            })
            .collect::<VmResult<Vec<_>>>()?;
        Ok((values, provided))
    }

    fn bind_call_arguments_at(
        &self,
        program: &Program,
        callee: &Value,
        mut positional: Vec<Value>,
        named: Vec<(String, Value)>,
        span: Option<&SourceSpan>,
    ) -> VmResult<(Vec<Value>, Vec<bool>)> {
        let Value::Closure(closure) = callee else {
            if named.is_empty() {
                return Ok((positional.clone(), vec![true; positional.len()]));
            }
            return Err(self.error_at(
                RuntimeErrorKind::Arity,
                "native functions do not accept named arguments".into(),
                span,
            ));
        };
        let closure_program = closure.program.as_deref().unwrap_or(program);
        let chunk = closure_program.chunk(closure.chunk).ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "closure references missing chunk".into(),
                span,
            )
        })?;
        if chunk.parameters.is_empty() {
            if chunk.arity != 0 {
                return Ok((positional.clone(), vec![true; positional.len()]));
            }
            if positional.is_empty() && named.is_empty() {
                return Ok((Vec::new(), Vec::new()));
            }
            return Err(self.error_at(
                RuntimeErrorKind::Arity,
                format!("`{}` expects no arguments", chunk.name),
                span,
            ));
        }
        let variadic = chunk
            .parameters
            .last()
            .filter(|parameter| parameter.variadic);
        let fixed = chunk.parameters.len() - usize::from(variadic.is_some());
        if positional.len() > chunk.parameters.len() && variadic.is_none() {
            return Err(self.error_at(
                RuntimeErrorKind::Arity,
                format!("`{}` received too many positional arguments", chunk.name),
                span,
            ));
        }
        let rest = positional.split_off(fixed.min(positional.len()));
        let mut bound = positional.into_iter().map(Some).collect::<Vec<_>>();
        bound.resize_with(chunk.parameters.len(), || None);
        for (name, value) in named {
            let slot = chunk
                .parameters
                .iter()
                .position(|parameter| parameter.name == name)
                .ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::Name,
                        format!("unknown parameter `{name}`"),
                        span,
                    )
                })?;
            if bound[slot].is_some() {
                return Err(self.error_at(
                    RuntimeErrorKind::Arity,
                    format!("parameter `{name}` was assigned more than once"),
                    span,
                ));
            }
            if chunk.parameters[slot].variadic && !matches!(value, Value::List(_)) {
                return Err(self.error_at(
                    RuntimeErrorKind::Type,
                    format!("variadic parameter `{name}` expects a list"),
                    span,
                ));
            }
            bound[slot] = Some(value);
        }
        if variadic.is_some() && bound[fixed].is_none() {
            bound[fixed] = Some(Value::List(List::from_values(rest).into_shared()));
        }
        let provided = bound.iter().map(Option::is_some).collect::<Vec<_>>();
        let values = bound
            .into_iter()
            .enumerate()
            .map(|(slot, value)| {
                value
                    .or_else(|| chunk.parameters[slot].has_default.then_some(Value::Nil))
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::Arity,
                            format!(
                                "missing required parameter `{}`",
                                chunk.parameters[slot].name
                            ),
                            span,
                        )
                    })
            })
            .collect::<VmResult<Vec<_>>>()?;
        Ok((values, provided))
    }

    fn bind_overload_arguments(
        &self,
        program: &Program,
        overloads: &[Value],
        positional: &[Value],
        named: &[NamedArgument],
        span: Option<SourceSpan>,
    ) -> VmResult<(Value, Vec<Value>, Vec<bool>)> {
        let mut error = None;
        for callee in overloads {
            let callee = callee
                .resolve()
                .map_err(|message| self.error(RuntimeErrorKind::Name, message, span.clone()))?;
            match self.bind_call_arguments(
                program,
                &callee,
                positional.to_vec(),
                named.to_vec(),
                span.clone(),
            ) {
                Ok((arguments, provided)) => return Ok((callee, arguments, provided)),
                Err(next) => error = Some(next),
            }
        }
        Err(error.unwrap_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidCall,
                "overload set is empty".into(),
                span,
            )
        }))
    }

    fn bind_overload_arguments_at(
        &self,
        program: &Program,
        overloads: &[Value],
        positional: &[Value],
        named: &[NamedArgument],
        span: Option<&SourceSpan>,
    ) -> VmResult<(Value, Vec<Value>, Vec<bool>)> {
        let mut error = None;
        for callee in overloads {
            let callee = callee
                .resolve()
                .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
            match self.bind_call_arguments_at(
                program,
                &callee,
                positional.to_vec(),
                named.to_vec(),
                span,
            ) {
                Ok((arguments, provided)) => return Ok((callee, arguments, provided)),
                Err(next) => error = Some(next),
            }
        }
        Err(error.unwrap_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidCall,
                "overload set is empty".into(),
                span,
            )
        }))
    }

    fn bind_selected_overload_arguments_at(
        &self,
        program: &Program,
        overloads: &[Value],
        selected: &CallableIdentity,
        positional: &[Value],
        named: &[NamedArgument],
        span: Option<&SourceSpan>,
    ) -> VmResult<(Value, Vec<Value>, Vec<bool>)> {
        for callee in overloads {
            let callee = callee
                .resolve()
                .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))?;
            let identity =
                Self::callable_signature(&callee).and_then(|signature| signature.identity);
            if identity.as_ref() != Some(selected) {
                continue;
            }
            let (arguments, provided) = self.bind_call_arguments_at(
                program,
                &callee,
                positional.to_vec(),
                named.to_vec(),
                span,
            )?;
            return Ok((callee, arguments, provided));
        }
        Err(self.error_at(
            RuntimeErrorKind::InvalidCall,
            "selected callable signature is no longer present in the live binding".into(),
            span,
        ))
    }

    #[cfg(feature = "metrics")]
    fn record_frame(&self, _local_count: usize) {
        let mut metrics = self.metrics.borrow_mut();
        metrics.frames_created += 1;
    }

    #[cfg(feature = "metrics")]
    fn record_local_cell(&self) {
        self.metrics.borrow_mut().local_binding_cells_created += 1;
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    pub(super) fn record_frame_locals(&self, capacity: usize, arguments: usize) {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.frame_local_vectors_created += 1;
            metrics.frame_local_capacity_total += capacity;
            metrics.argument_values_copied_to_locals += arguments;
        }
        #[cfg(not(feature = "metrics"))]
        let _ = (capacity, arguments);
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    pub(super) fn record_local_argument_writes(&self, arguments: usize) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().argument_values_copied_to_locals += arguments;
        }
        #[cfg(not(feature = "metrics"))]
        let _ = arguments;
    }

    fn frame_provided(&self, provided: Option<Vec<bool>>) -> ProvidedArguments {
        let Some(provided) = provided else {
            return ProvidedArguments::All;
        };
        self.record_provided_argument_bitmap(provided.capacity());
        ProvidedArguments::Bitmap(provided)
    }

    pub(super) fn provided_bitmap(&self, provided: Vec<bool>) -> ProvidedArguments {
        self.record_provided_argument_bitmap(provided.capacity());
        ProvidedArguments::Bitmap(provided)
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    fn record_provided_argument_bitmap(&self, capacity: usize) {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.provided_argument_bitmaps_created += 1;
            metrics.provided_argument_bitmap_capacity_total += capacity;
        }
        #[cfg(not(feature = "metrics"))]
        let _ = capacity;
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    fn record_defer_scope_stack(&self, entries: usize) {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.defer_scope_stacks_created += 1;
            metrics.defer_scope_entries_materialized += entries;
        }
        #[cfg(not(feature = "metrics"))]
        let _ = entries;
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    fn record_closure_argument_vector(&self) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().closure_argument_vectors_created += 1;
        }
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    fn record_exact_positional_stack_local_initialization(&self, arguments: usize) {
        #[cfg(feature = "metrics")]
        {
            self.metrics
                .borrow_mut()
                .exact_positional_stack_local_initializations += arguments;
        }
        #[cfg(not(feature = "metrics"))]
        let _ = arguments;
    }

    #[cfg_attr(not(feature = "metrics"), allow(clippy::unused_self))]
    pub(super) fn record_recur_local_vector(&self, reused: bool) {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            if reused {
                metrics.recur_local_vectors_reused += 1;
            } else {
                metrics.recur_local_vectors_replaced += 1;
            }
        }
        #[cfg(not(feature = "metrics"))]
        let _ = reused;
    }

    #[cfg(feature = "metrics")]
    pub(super) fn record_exact_positional_recur_restart(&self) {
        self.metrics.borrow_mut().exact_positional_recur_restarts += 1;
    }

    #[cfg(feature = "metrics")]
    pub(super) fn record_generic_recur_argument_binding(&self) {
        self.metrics.borrow_mut().generic_recur_argument_bindings += 1;
    }

    #[cfg(feature = "metrics")]
    fn record_exact_positional_closure_call(&self) {
        self.metrics.borrow_mut().exact_positional_closure_calls += 1;
    }

    #[cfg(feature = "metrics")]
    fn record_generic_call_argument_binding(&self) {
        self.metrics.borrow_mut().generic_call_argument_bindings += 1;
    }

    #[cfg(feature = "metrics")]
    fn record_collection_construction(&self, elements: usize) {
        let mut metrics = self.metrics.borrow_mut();
        metrics.collection_constructions += 1;
        metrics.collection_elements_constructed += elements;
    }

    #[cfg(feature = "metrics")]
    fn record_collection_update(&self, copied: usize, unique_owner: bool) {
        let mut metrics = self.metrics.borrow_mut();
        metrics.collection_updates += 1;
        if unique_owner {
            metrics.collection_unique_owner_updates += 1;
        } else {
            metrics.collection_elements_copied += copied;
            metrics.collection_shared_owner_updates += 1;
        }
    }

    #[cfg(feature = "metrics")]
    fn record_collection_slice(&self) {
        self.metrics.borrow_mut().collection_slices += 1;
    }

    fn error_at(
        &self,
        kind: RuntimeErrorKind,
        message: String,
        span: Option<&SourceSpan>,
    ) -> RuntimeError {
        self.error(kind, message, self.owned_span(span))
    }

    fn owned_span(&self, span: Option<&SourceSpan>) -> Option<SourceSpan> {
        let span = span.or_else(|| self.active_span());
        #[cfg(feature = "metrics")]
        if span.is_some() {
            let mut metrics = self.metrics.borrow_mut();
            metrics.source_span_clones += 1;
        }
        span.cloned()
    }

    /// Retains a compact caller instruction reference whenever this call is
    /// entered by the dispatch loop. The caller frame owns the corresponding
    /// program until this frame returns, so diagnostics can resolve it lazily.
    fn frame_call_span(&self, span: Option<&SourceSpan>) -> Option<CallSpan> {
        match span {
            Some(span) => Some(CallSpan::Owned(Box::new(span.clone()))),
            None => self.active_span.map(CallSpan::Instruction),
        }
    }

    fn active_span(&self) -> Option<&SourceSpan> {
        let span = self
            .active_span
            .and_then(|span| self.frames.last()?.program.span(span));
        #[cfg(feature = "metrics")]
        if span.is_some() {
            self.metrics.borrow_mut().source_span_lookups += 1;
        }
        span
    }

    fn jump_at(&mut self, target: usize, span: Option<&SourceSpan>) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        self.frames.last_mut().expect("active frame was checked").ip = target;
        Ok(())
    }
}
