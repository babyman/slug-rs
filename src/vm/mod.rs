use std::{
    cell::{Cell, RefCell},
    cmp::Ordering,
    collections::HashSet,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::source::environment::CallableIdentity;
use crate::{
    CallArgumentKind, Capture, ModuleDeclaration, ModuleLoader, NativeDescriptorError,
    NativeFunction, Program, SourceSpan, Task, Value,
    bytecode::{Op, SelectCase},
    native::{NativeInvocation, NativeResourceRegistry, native_resource_registry},
    value::{
        BindingCell, Builtin, Channel, ChannelReceive, ChannelSend, Closure, GlobalEnvironment,
        RootWaiter, SelectWake, TaskAdmission, WaitRegistration, WaitSet, Waiter, binding_cell,
        global_environment, module_binding,
    },
};

mod cleanup;
mod error;
mod operations;
mod scheduler;

use cleanup::{Cleanup, Deferred};
pub use error::{CallFrame, NativeErrorDetails, RuntimeError, RuntimeErrorKind};
use operations::{
    add, bit_not, bitwise, construct_struct, copy_struct, divide, index_value, is_map_key,
    list_append, list_prepend, matches_pattern, modulo, multiply, negate, numbers, shift,
    slice_value, subtract,
};
use scheduler::Nursery;

pub type VmResult<T> = Result<T, RuntimeError>;

type NamedArgument = (String, Value);
type ExpandedCallArguments = (Vec<Value>, Vec<NamedArgument>);

/// Execution counters for one public VM invocation.
///
/// The counters describe private representation costs, not source semantics.
/// They provide a measurement seam while bytecode changes, but are not a
/// profiling or compatibility API.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VmMetrics {
    /// Instructions fetched by the dispatch loop, including spawned tasks.
    pub instructions_executed: usize,
    /// Whole instructions cloned while fetching them for dispatch.
    pub instruction_clones: usize,
    /// Source spans cloned because an instruction takes the owned-span path.
    pub source_span_clones: usize,
    /// Frames allocated for the root invocation, calls, and spawned tasks.
    pub frames_created: usize,
    /// Frame-local binding cells allocated by the current representation.
    pub local_binding_cells_created: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CallableRuntimeSignature {
    identity: Option<CallableIdentity>,
    shape: Vec<(bool, bool)>,
}

#[derive(Clone)]
struct Frame {
    closure: Rc<Closure>,
    function: String,
    call_span: Option<SourceSpan>,
    ip: usize,
    stack_base: usize,
    locals: Vec<BindingCell>,
    provided: Vec<bool>,
    scopes: Vec<Vec<Deferred>>,
    cleanup_action: bool,
    cleanup_recovers: bool,
}

struct ClosureCallOptions {
    direct_task_limit: Option<usize>,
    direct_task_count: Option<Rc<Cell<usize>>>,
    nursery: Rc<Nursery>,
    settle_nursery: bool,
}

/// The independently owned interpreter state for a spawned task.
///
/// It currently runs to settlement, but keeping the VM intact makes future
/// blocking operations able to return it to the scheduler without rebuilding
/// frames, locals, or the operand stack.
pub(crate) struct TaskExecution {
    vm: Vm,
    program: Rc<Program>,
    settle_nursery: bool,
}

enum TaskRunOutcome {
    Settled(VmResult<Value>),
    Suspended(Box<TaskExecution>),
}

enum ExecutionOutcome {
    Settled(VmResult<Value>),
    Suspended,
}

enum BorrowedSpanOpOutcome {
    NotHandled,
    Continue,
    Settled(Value),
}

#[derive(Clone)]
enum Suspension {
    Select(Option<SourceSpan>),
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
    After {
        deadline: Instant,
        handler: Option<Value>,
    },
    Await {
        task: Rc<Task>,
        handler: Option<Value>,
    },
    Default {
        handler: Option<Value>,
    },
}

impl TaskExecution {
    fn run(mut self) -> TaskRunOutcome {
        match self.vm.execute(&self.program) {
            ExecutionOutcome::Suspended => TaskRunOutcome::Suspended(Box::new(self)),
            ExecutionOutcome::Settled(result) => {
                let result = if self.settle_nursery {
                    self.vm.settle_tasks(result)
                } else {
                    result
                };
                TaskRunOutcome::Settled(result)
            }
        }
    }

    pub(crate) fn set_current_task(&mut self, task: &Rc<Task>) {
        self.vm.current_waiter = Some(Waiter::Task(task.clone()));
    }

    pub(crate) fn resume(&mut self, result: VmResult<Value>) {
        self.vm.resume = Some(result);
    }

    pub(crate) fn reject_closed_send(&mut self) {
        let span = match &self.vm.suspension {
            Some(Suspension::Select(span)) => span.clone(),
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
    nursery: Rc<Nursery>,
    direct_task_limit: Option<usize>,
    direct_task_count: Option<Rc<Cell<usize>>>,
    native_resources: NativeResourceRegistry,
    current_waiter: Option<Waiter>,
    suspension: Option<Suspension>,
    resume: Option<VmResult<Value>>,
    wait_registration: Option<WaitSet>,
    metrics: Rc<RefCell<VmMetrics>>,
}

impl Default for Vm {
    fn default() -> Self {
        Self {
            module_loader: None,
            module_program: None,
            globals: global_environment(),
            imported_globals: HashSet::new(),
            module_metadata: Vec::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            cleanup: Vec::new(),
            nursery: Rc::new(Nursery::root()),
            direct_task_limit: None,
            direct_task_count: None,
            native_resources: native_resource_registry(),
            current_waiter: None,
            suspension: None,
            resume: None,
            wait_registration: None,
            metrics: Rc::new(RefCell::new(VmMetrics::default())),
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

    pub(crate) fn run_module(&mut self, program: &Rc<Program>) -> VmResult<Value> {
        self.module_program = Some(program.clone());
        self.install_implicit_builtins(program)?;
        self.bind_foreign_declarations(program)?;
        self.run_named(program, "main")
    }

    /// Executes top-level code and then the program module's zero-argument `main`.
    ///
    /// Loaded modules use [`Self::run_module`], which intentionally does not
    /// invoke their `main` binding.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when top-level execution or the entrypoint
    /// call fails.
    pub fn run_program(&mut self, program: &Program) -> VmResult<Value> {
        self.module_program = Some(Rc::new(program.clone()));
        self.install_implicit_builtins(program)?;
        self.bind_foreign_declarations(program)?;
        let top_level = self.run_named(program, "main")?;
        if !program.has_entrypoint() {
            return Ok(top_level);
        }
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
        self.stack.clear();
        self.stack.push(entrypoint);
        self.call(program, 0, None, None)?;
        self.run_root_execution(program)
    }

    #[must_use]
    pub fn module_metadata(&self) -> &[ModuleDeclaration] {
        &self.module_metadata
    }

    #[must_use]
    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name).cloned()
    }

    /// Returns counters for the most recent public VM invocation.
    #[must_use]
    pub fn metrics(&self) -> VmMetrics {
        self.metrics.borrow().clone()
    }

    #[must_use]
    pub fn exported_values(&self, program: &Program) -> Value {
        Value::Map(Rc::new(
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
        ))
    }

    pub(crate) fn live_exported_values(&self, program: &Program) -> Value {
        Value::Map(Rc::new(
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
        ))
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
        let Some(loader) = &self.module_loader else {
            return if program
                .declarations()
                .iter()
                .any(|declaration| declaration.foreign)
            {
                Err(self.error(
                    RuntimeErrorKind::Module,
                    "foreign declarations require a module loader".into(),
                    None,
                ))
            } else {
                Ok(())
            };
        };
        for declaration in program
            .declarations()
            .iter()
            .filter(|declaration| declaration.foreign)
        {
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
                let value = Value::DeclaredNative {
                    function,
                    callable_identity: identity,
                };
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
                    self.globals.borrow_mut().insert(name.clone(), value);
                }
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
        self.globals.borrow_mut().extend(loader.builtin_globals());
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
        self.globals
            .borrow_mut()
            .insert("cfg".into(), Value::Builtin(Builtin::Cfg));
        self.globals
            .borrow_mut()
            .insert("argv".into(), Value::Builtin(Builtin::Argv));
        self.globals
            .borrow_mut()
            .insert("argm".into(), Value::Builtin(Builtin::Argm));
    }

    /// Executes a zero-argument entry chunk.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is invalid or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run(&mut self, program: &Program, entry: usize) -> VmResult<Value> {
        self.metrics.borrow_mut().clone_from(&VmMetrics::default());
        let chunk = program.chunk(entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("entry chunk {entry} does not exist"),
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
        self.nursery.clear();
        self.module_metadata = program.declarations().to_vec();
        self.record_frame(chunk.locals);
        self.frames.push(Frame {
            closure: Rc::new(Closure {
                chunk: entry,
                captures: Vec::new(),
                program: None,
                globals: None,
                capture_sources: Vec::new(),
            }),
            function: chunk.name.clone(),
            call_span: None,
            ip: 0,
            stack_base: 0,
            locals: (0..chunk.locals)
                .map(|_| binding_cell(Value::Nil))
                .collect(),
            provided: vec![false; chunk.arity],
            scopes: vec![Vec::new()],
            cleanup_action: false,
            cleanup_recovers: false,
        });
        self.run_root_execution(program)
    }

    /// Executes a zero-argument chunk selected by name.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is absent or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run_named(&mut self, program: &Program, entry: &str) -> VmResult<Value> {
        let index = program.find_chunk(entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::Name,
                format!("unknown entry `{entry}`"),
                None,
            )
        })?;
        self.run(program, index)
    }

    #[allow(clippy::too_many_lines)]
    fn execute(&mut self, program: &Program) -> ExecutionOutcome {
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
                    match self.drive_cleanup(program) {
                        Ok(Some(value)) => return ExecutionOutcome::Settled(Ok(value)),
                        Ok(None) => {}
                        Err(error) => return ExecutionOutcome::Settled(Err(error)),
                    }
                }
            }
        }
        loop {
            match self.execute_raw(program) {
                Ok(ExecutionOutcome::Settled(result)) => return ExecutionOutcome::Settled(result),
                Ok(ExecutionOutcome::Suspended) => return ExecutionOutcome::Suspended,
                Err(error) if self.frames.is_empty() => {
                    return ExecutionOutcome::Settled(Err(error));
                }
                Err(error) => {
                    self.begin_error(error);
                    match self.drive_cleanup(program) {
                        Ok(Some(value)) => return ExecutionOutcome::Settled(Ok(value)),
                        Ok(None) => {}
                        Err(error) => return ExecutionOutcome::Settled(Err(error)),
                    }
                }
            }
        }
    }

    fn run_root_execution(&mut self, program: &Program) -> VmResult<Value> {
        let root = RootWaiter::new();
        self.current_waiter = Some(Waiter::Root(root.clone()));
        loop {
            match self.execute(program) {
                ExecutionOutcome::Settled(result) => return self.settle_tasks(result),
                ExecutionOutcome::Suspended => loop {
                    if let Some(result) = root.take_resume() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::Root(root.clone()));
                        }
                        self.resume = Some(result);
                        break;
                    }
                    if !self.make_progress() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::Root(root.clone()));
                        }
                        let blocked = self.error(
                            RuntimeErrorKind::InvalidCall,
                            "task remains blocked with no runnable work".into(),
                            None,
                        );
                        return self.settle_tasks(Err(blocked));
                    }
                },
            }
        }
    }

    fn run_nested_execution(&mut self, program: &Program) -> VmResult<Value> {
        let root = RootWaiter::new();
        self.current_waiter = Some(Waiter::Root(root.clone()));
        loop {
            match self.execute(program) {
                ExecutionOutcome::Settled(result) => return result,
                ExecutionOutcome::Suspended => loop {
                    if let Some(result) = root.take_resume() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::Root(root.clone()));
                        }
                        self.resume = Some(result);
                        break;
                    }
                    if !self.make_progress() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::Root(root.clone()));
                        }
                        return Err(self.error(
                            RuntimeErrorKind::InvalidCall,
                            "task remains blocked with no runnable work".into(),
                            None,
                        ));
                    }
                },
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn execute_raw(&mut self, program: &Program) -> VmResult<ExecutionOutcome> {
        loop {
            if self.suspension.is_some() {
                return Ok(ExecutionOutcome::Suspended);
            }
            let instruction = self.next_instruction(program)?;
            match self.execute_borrowed_span_op(
                program,
                &instruction.op,
                instruction.span.as_ref(),
            )? {
                BorrowedSpanOpOutcome::Continue => continue,
                BorrowedSpanOpOutcome::Settled(value) => {
                    return Ok(ExecutionOutcome::Settled(Ok(value)));
                }
                BorrowedSpanOpOutcome::NotHandled => {}
            }
            if instruction.span.is_some() {
                self.metrics.borrow_mut().source_span_clones += 1;
            }
            let span = instruction.span.clone();
            match &instruction.op {
                Op::Constant(index) => {
                    let chunk = self.current_chunk(program)?;
                    let value = match chunk.constants.get(*index) {
                        Some(crate::Constant::Value(value)) => value.clone(),
                        Some(crate::Constant::Function(function)) => {
                            Value::Closure(Rc::new(Closure {
                                chunk: *function,
                                captures: Vec::new(),
                                program: self.module_program.clone(),
                                globals: self.module_program.as_ref().map(|_| self.globals.clone()),
                                capture_sources: Vec::new(),
                            }))
                        }
                        None => {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("constant {index} does not exist"),
                                span,
                            ));
                        }
                    };
                    self.stack.push(value);
                }
                Op::Interpolate(parts) => {
                    let values = self.pop_values(parts.len().saturating_sub(1), span.clone())?;
                    let mut output = String::new();
                    for (index, text) in parts.iter().enumerate() {
                        output.push_str(text);
                        if let Some(value) = values.get(index) {
                            output.push_str(&value.to_string());
                        }
                    }
                    self.stack.push(Value::string(output));
                }
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
                Op::Pop => {
                    self.pop(span)?;
                }
                Op::Duplicate => self.stack.push(self.peek(span)?.clone()),
                Op::GetLocal(slot) => self.stack.push(self.local(*slot, span)?.borrow().clone()),
                Op::SetLocal(slot) => {
                    let value = self.pop(span.clone())?;
                    self.set_local(*slot, value, span)?;
                }
                Op::GetCapture(slot) => {
                    let value = self
                        .frames
                        .last()
                        .and_then(|frame| frame.closure.captures.get(*slot))
                        .map(|cell| cell.borrow().clone())
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("capture {slot} does not exist"),
                                span.clone(),
                            )
                        })?;
                    self.stack.push(value);
                }
                Op::SetCapture(slot) => {
                    let value = self.pop(span.clone())?;
                    let capture = self
                        .frames
                        .last()
                        .and_then(|frame| frame.closure.captures.get(*slot))
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("capture {slot} does not exist"),
                                span.clone(),
                            )
                        })?;
                    *capture.borrow_mut() = value;
                }
                Op::GetGlobal(name) => {
                    let value = self
                        .globals
                        .borrow()
                        .get(name)
                        .cloned()
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::Name,
                                format!("unknown name `{name}`"),
                                span.clone(),
                            )
                        })?
                        .resolve()
                        .map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })?;
                    self.stack.push(value);
                }
                Op::NotImplemented => {
                    return Err(self.error(
                        RuntimeErrorKind::NotImplemented,
                        "not implemented".into(),
                        span,
                    ));
                }
                Op::DefineGlobal(name) => {
                    let value = self.pop_unresolved(span.clone())?;
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
                    let existing = self.pop_unresolved(span.clone())?;
                    let new = self.pop_unresolved(span.clone())?;
                    let mut overloads = match existing {
                        Value::Overloads(overloads) => overloads.as_ref().clone(),
                        value if Self::callable_signature(&value).is_some() => vec![value],
                        _ => {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                "overload combination requires callable values".into(),
                                span,
                            ));
                        }
                    };
                    match new {
                        Value::Overloads(values) => overloads.extend(values.iter().cloned()),
                        value if Self::callable_signature(&value).is_some() => {
                            overloads.push(value);
                        }
                        _ => {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                "overload combination requires callable values".into(),
                                span,
                            ));
                        }
                    }
                    self.stack.push(Value::Overloads(Rc::new(overloads)));
                }
                Op::DefineMapGlobals => {
                    let value = self.pop_unresolved(span.clone())?;
                    let Value::Map(entries) = value else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            format!("{{*}} binding expects a map, got {}", value.type_name()),
                            span,
                        ));
                    };
                    for (key, value) in entries.iter() {
                        let Value::Str(name) = key else {
                            return Err(self.error(
                                RuntimeErrorKind::Type,
                                "{*} binding requires string map keys".into(),
                                span.clone(),
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
                    let arguments = self.pop_values(*arguments, span.clone())?;
                    if self
                        .module_metadata
                        .get(*declaration)
                        .is_none_or(|declaration| declaration.tags.get(*tag).is_none())
                    {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "module tag metadata does not exist".into(),
                            span,
                        ));
                    }
                    self.module_metadata[*declaration].tags[*tag].arguments = arguments;
                }
                Op::SetGlobal(name) => {
                    if !self.globals.borrow().contains_key(name) {
                        return Err(self.error(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            span,
                        ));
                    }
                    let value = self.pop(None)?;
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
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("function chunk {chunk} does not exist"),
                            span.clone(),
                        )
                    })?;
                    let capture_sources = captures.clone();
                    let captures = captures
                        .iter()
                        .map(|capture| match capture {
                            Capture::Local(slot) => self.local(*slot, span.clone()),
                            Capture::Capture(slot) => self
                                .frames
                                .last()
                                .and_then(|frame| frame.closure.captures.get(*slot))
                                .cloned()
                                .ok_or_else(|| {
                                    self.error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        format!("capture {slot} does not exist"),
                                        span.clone(),
                                    )
                                }),
                        })
                        .collect::<VmResult<Vec<_>>>()?;
                    self.stack.push(Value::Closure(Rc::new(Closure {
                        chunk: *chunk,
                        captures,
                        program: self.module_program.clone(),
                        globals: self.module_program.as_ref().map(|_| self.globals.clone()),
                        capture_sources,
                    })));
                }
                Op::Add => self.binary(span, add)?,
                Op::Subtract => self.binary(span, subtract)?,
                Op::Multiply => self.binary(span, multiply)?,
                Op::Divide => self.binary(span, divide)?,
                Op::Modulo => self.binary(span, modulo)?,
                Op::BitAnd => {
                    self.binary(span, |left, right| bitwise(left, right, |a, b| a & b))?;
                }
                Op::BitOr => self.binary(span, |left, right| bitwise(left, right, |a, b| a | b))?,
                Op::BitXor => {
                    self.binary(span, |left, right| bitwise(left, right, |a, b| a ^ b))?;
                }
                Op::ShiftLeft => {
                    self.binary(span, |left, right| shift(left, right, i64::checked_shl))?;
                }
                Op::ShiftRight => {
                    self.binary(span, |left, right| shift(left, right, i64::checked_shr))?;
                }
                Op::ListAppend => self.binary(span, |list, value| {
                    list_append(list, value).map_err(|message| (RuntimeErrorKind::Type, message))
                })?,
                Op::ListPrepend => self.binary(span, |value, list| {
                    list_prepend(value, list).map_err(|message| (RuntimeErrorKind::Type, message))
                })?,
                Op::List(count) => {
                    let values = self.pop_values(*count, span.clone())?;
                    self.stack.push(Value::List(Rc::new(values)));
                }
                Op::ListSpread(spreads) => self.list_spread(spreads.clone(), span)?,
                Op::Map(count) => {
                    let values = self.pop_values(count.saturating_mul(2), span.clone())?;
                    let mut entries = Vec::with_capacity(*count);
                    for pair in values.chunks_exact(2) {
                        if !is_map_key(&pair[0]) {
                            return Err(self.error(
                                RuntimeErrorKind::Type,
                                format!("{} cannot be used as a map key", pair[0].type_name()),
                                span,
                            ));
                        }
                        entries.push((pair[0].clone(), pair[1].clone()));
                    }
                    self.stack.push(Value::Map(Rc::new(entries)));
                }
                Op::StructSchema(fields) => {
                    let default_count = fields.iter().filter(|field| field.has_default).count();
                    let defaults = self.pop_values(default_count, span.clone())?;
                    let mut defaults = defaults.into_iter();
                    let mut names = Vec::with_capacity(fields.len());
                    let mut schema_fields = Vec::with_capacity(fields.len());
                    for field in fields {
                        if names.contains(&field.name) {
                            return Err(self.error(
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
                    let values = self.pop_values(fields.len(), span.clone())?;
                    let schema = self.pop(span.clone())?;
                    self.stack.push(
                        construct_struct(schema, fields, &values)
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::StructCopy(fields) => {
                    let replacements = self.pop_values(fields.len(), span.clone())?;
                    let value = self.pop(span.clone())?;
                    self.stack.push(
                        copy_struct(value, fields, &replacements)
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::GetIndex => {
                    let (collection, index) = self.pop_pair(span.clone())?;
                    self.stack
                        .push(index_value(collection, &index).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::GetSlice {
                    has_start,
                    has_end,
                    has_step,
                } => {
                    let count =
                        usize::from(*has_start) + usize::from(*has_end) + usize::from(*has_step);
                    let mut values = self.pop_values(count + 1, span.clone())?.into_iter();
                    let collection = values
                        .next()
                        .expect("slice operation includes a collection");
                    let start = has_start.then(|| values.next().expect("slice start is present"));
                    let end = has_end.then(|| values.next().expect("slice end is present"));
                    let step = has_step.then(|| values.next().expect("slice step is present"));
                    self.stack.push(
                        slice_value(collection, start.as_ref(), end.as_ref(), step.as_ref())
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::Negate => {
                    let value = self.pop(span.clone())?;
                    self.stack
                        .push(negate(value).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::Not => {
                    let value = self.pop(span)?;
                    self.stack.push(Value::Bool(!value.is_truthy()));
                }
                Op::BitNot => {
                    let value = self.pop(span.clone())?;
                    self.stack
                        .push(bit_not(&value).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::Equal => {
                    let (left, right) = self.pop_pair(span.clone())?;
                    self.stack.push(Value::Bool(left == right));
                }
                Op::Greater => self.compare(span, Ordering::Greater)?,
                Op::Less => self.compare(span, Ordering::Less)?,
                Op::Jump(target) => self.jump(*target, span)?,
                Op::JumpIfFalse(target) => {
                    if !self.peek(span.clone())?.is_truthy() {
                        self.jump(*target, span)?;
                    }
                }
                Op::JumpIfProvided { slot, target } => {
                    if self
                        .frames
                        .last()
                        .and_then(|frame| frame.provided.get(*slot))
                        .copied()
                        == Some(true)
                    {
                        self.jump(*target, span)?;
                    }
                }
                Op::Call(count) => self.call(program, *count, None, span)?,
                Op::CallSpread(kinds) => self.call_spread(program, kinds.clone(), None, span)?,
                Op::CallSelected { kinds, identity } => {
                    let identity = program.callable_identity(*identity).ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "selected callable identity does not exist".into(),
                            span.clone(),
                        )
                    })?;
                    self.call_spread(program, kinds.clone(), Some(identity), span)?;
                }
                Op::PipelineCall(kinds) => {
                    self.pipeline_call(program, kinds.clone(), None, span)?;
                }
                Op::PipelineCallSelected { kinds, identity } => {
                    let identity = program.callable_identity(*identity).ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "selected callable identity does not exist".into(),
                            span.clone(),
                        )
                    })?;
                    self.pipeline_call(program, kinds.clone(), Some(identity), span)?;
                }
                Op::Import(kinds) => self.import(kinds.clone(), span)?,
                Op::Spawn => self.spawn_task(program, span)?,
                Op::Nursery { has_limit } => self.run_nursery(program, *has_limit, span)?,
                Op::Select(cases) => self.select(cases, span)?,
                Op::SelectApply => self.select_apply(program, span)?,
                Op::TryMatch {
                    pattern,
                    bindings,
                    operands,
                } => {
                    let operands = self.pop_values(*operands, span.clone())?;
                    let value = self.pop(span.clone())?;
                    let mut values = Vec::new();
                    let matched = matches_pattern(pattern, &value, &operands, &mut values)
                        .map_err(|(kind, message)| self.error(kind, message, span.clone()))?;
                    if matched && values.len() != *bindings {
                        return Err(self.error(
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
                    return Err(self.error(
                        RuntimeErrorKind::Match,
                        "destructuring pattern did not match".into(),
                        span,
                    ));
                }
                Op::Throw => {
                    let value = self.pop(span.clone())?;
                    return Err(self.thrown(value, span));
                }
                Op::EnterScope => self.current_scopes(span)?.push(Vec::new()),
                Op::LeaveScope => {
                    let actions = self.current_scopes(span.clone())?.pop().ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active scope".into(),
                            span.clone(),
                        )
                    })?;
                    if self.frames.last().is_some_and(|frame| frame.cleanup_action) {
                        self.cleanup.push(Cleanup::Resume);
                    }
                    self.cleanup.push(Cleanup::Actions {
                        actions,
                        success: true,
                        frame_depth: self.frames.len() - 1,
                    });
                    if let Some(value) = self.drive_cleanup(program)? {
                        return Ok(ExecutionOutcome::Settled(Ok(value)));
                    }
                }
                Op::Defer { mode } => {
                    let action = self.pop(span.clone())?;
                    if !matches!(
                        action,
                        Value::Closure(_)
                            | Value::Native(_)
                            | Value::DeclaredNative { .. }
                            | Value::Builtin(_)
                    ) {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            "defer expects a callable action".into(),
                            span,
                        ));
                    }
                    let Some(frame) = self.frames.last_mut() else {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active call frame".into(),
                            span,
                        ));
                    };
                    let Some(scope) = frame.scopes.last_mut() else {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active scope".into(),
                            None,
                        ));
                    };
                    scope.push(Deferred {
                        action,
                        mode: *mode,
                    });
                }
                Op::Recur(kinds) => self.recur(program, kinds.clone(), span)?,
                Op::Return => {
                    let value = self.pop(span.clone())?;
                    if let Some(value) = self.begin_return(program, value)? {
                        return Ok(ExecutionOutcome::Settled(Ok(value)));
                    }
                }
            }
        }
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
                        program: self.module_program.clone(),
                        globals: self.module_program.as_ref().map(|_| self.globals.clone()),
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
            Op::Nil => self.stack.push(Value::Nil),
            Op::True => self.stack.push(Value::Bool(true)),
            Op::False => self.stack.push(Value::Bool(false)),
            Op::Pop => {
                self.pop_at(span)?;
            }
            Op::Duplicate => self.stack.push(self.peek_at(span)?.clone()),
            Op::GetLocal(slot) => self
                .stack
                .push(self.local_at(*slot, span)?.borrow().clone()),
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
            Op::MakeClosure { chunk, captures } => {
                program.chunk(*chunk).ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        format!("function chunk {chunk} does not exist"),
                        span,
                    )
                })?;
                let capture_sources = captures.clone();
                let captures = captures
                    .iter()
                    .map(|capture| match capture {
                        Capture::Local(slot) => self.local_at(*slot, span),
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
                    program: self.module_program.clone(),
                    globals: self.module_program.as_ref().map(|_| self.globals.clone()),
                    capture_sources,
                })));
            }
            Op::Add => self.binary_at(span, add)?,
            Op::Subtract => self.binary_at(span, subtract)?,
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
            Op::ListAppend => self.binary_at(span, |list, value| {
                list_append(list, value).map_err(|message| (RuntimeErrorKind::Type, message))
            })?,
            Op::ListPrepend => self.binary_at(span, |value, list| {
                list_prepend(value, list).map_err(|message| (RuntimeErrorKind::Type, message))
            })?,
            Op::List(count) => {
                let values = self.pop_values_at(*count, span)?;
                self.stack.push(Value::List(Rc::new(values)));
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
                self.stack.push(Value::Map(Rc::new(entries)));
            }
            Op::GetIndex => {
                let (collection, index) = self.pop_pair_at(span)?;
                self.stack.push(
                    index_value(collection, &index)
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
            Op::Greater => self.compare_at(span, Ordering::Greater)?,
            Op::Less => self.compare_at(span, Ordering::Less)?,
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
                    .and_then(|frame| frame.provided.get(*slot))
                    .copied()
                    == Some(true)
                {
                    self.jump_at(*target, span)?;
                }
            }
            Op::Recur(kinds) => self.recur_at(program, kinds, span)?,
            Op::Call(count) => self.call(program, *count, None, self.owned_span(span))?,
            Op::EnterScope => self.current_scopes_at(span)?.push(Vec::new()),
            Op::LeaveScope => {
                let actions = self.current_scopes_at(span)?.pop().ok_or_else(|| {
                    self.error_at(
                        RuntimeErrorKind::InvalidBytecode,
                        "no active scope".into(),
                        span,
                    )
                })?;
                if self.frames.last().is_some_and(|frame| frame.cleanup_action) {
                    self.cleanup.push(Cleanup::Resume);
                }
                self.cleanup.push(Cleanup::Actions {
                    actions,
                    success: true,
                    frame_depth: self.frames.len() - 1,
                });
                if let Some(value) = self.drive_cleanup(program)? {
                    return Ok(BorrowedSpanOpOutcome::Settled(value));
                }
            }
            Op::Return => {
                let value = self.pop_at(span)?;
                if let Some(value) = self.begin_return(program, value)? {
                    return Ok(BorrowedSpanOpOutcome::Settled(value));
                }
            }
            _ => return Ok(BorrowedSpanOpOutcome::NotHandled),
        }
        Ok(BorrowedSpanOpOutcome::Continue)
    }

    fn next_instruction<'program>(
        &mut self,
        program: &'program Program,
    ) -> VmResult<&'program crate::Instruction> {
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
        let mut metrics = self.metrics.borrow_mut();
        metrics.instructions_executed += 1;
        drop(metrics);
        self.frames.last_mut().expect("active frame was checked").ip += 1;
        Ok(instruction)
    }

    fn current_chunk<'a>(&self, program: &'a Program) -> VmResult<&'a crate::Chunk> {
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
                if let Some(module_program) = &closure.program
                    && self
                        .module_program
                        .as_ref()
                        .is_none_or(|current| !Rc::ptr_eq(current, module_program))
                {
                    let arguments = self.stack[base + 1..]
                        .iter()
                        .map(|value| {
                            value.resolve().map_err(|message| {
                                self.error(RuntimeErrorKind::Name, message, span.clone())
                            })
                        })
                        .collect::<VmResult<Vec<_>>>()?;
                    let result = self.call_module_closure(
                        module_program,
                        closure.clone(),
                        arguments,
                        provided,
                        span.clone(),
                        ClosureCallOptions {
                            direct_task_limit: None,
                            direct_task_count: None,
                            nursery: self.nursery.clone(),
                            settle_nursery: false,
                        },
                    )?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                    return Ok(());
                }
                let chunk = program.chunk(closure.chunk).ok_or_else(|| {
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
                let mut locals = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map(binding_cell).map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                locals.resize_with(chunk.locals, || binding_cell(Value::Nil));
                self.record_frame(chunk.locals);
                self.frames.push(Frame {
                    closure,
                    function: chunk.name.clone(),
                    call_span: span,
                    ip: 0,
                    stack_base: base,
                    locals,
                    provided: provided.unwrap_or_else(|| vec![true; chunk.arity]),
                    scopes: vec![Vec::new()],
                    cleanup_action: false,
                    cleanup_recovers: false,
                });
            }
            Value::Native(function) | Value::DeclaredNative { function, .. } => {
                let arguments = self.stack[base + 1..]
                    .iter()
                    .map(|value| {
                        value.resolve().map_err(|message| {
                            self.error(RuntimeErrorKind::Name, message, span.clone())
                        })
                    })
                    .collect::<VmResult<Vec<_>>>()?;
                let result = self.invoke_native(&function, &arguments, span)?;
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

    fn module_closure_execution(
        &self,
        program: Rc<Program>,
        closure: Rc<Closure>,
        arguments: Vec<Value>,
        provided: Option<Vec<bool>>,
        span: Option<SourceSpan>,
        options: ClosureCallOptions,
    ) -> VmResult<TaskExecution> {
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
            nursery: options.nursery,
            direct_task_limit: options.direct_task_limit,
            direct_task_count: options.direct_task_count,
            native_resources: self.native_resources.clone(),
            current_waiter: None,
            suspension: None,
            resume: None,
            wait_registration: None,
            metrics: self.metrics.clone(),
        };
        let mut locals = arguments.into_iter().map(binding_cell).collect::<Vec<_>>();
        locals.resize_with(chunk.locals, || binding_cell(Value::Nil));
        vm.record_frame(chunk.locals);
        vm.frames.push(Frame {
            closure,
            function: chunk.name.clone(),
            call_span: span,
            ip: 0,
            stack_base: 0,
            locals,
            provided: provided.unwrap_or_else(|| vec![true; chunk.arity]),
            scopes: vec![Vec::new()],
            cleanup_action: false,
            cleanup_recovers: false,
        });
        Ok(TaskExecution {
            vm,
            program,
            settle_nursery: options.settle_nursery,
        })
    }

    fn call_module_closure(
        &self,
        program: &Rc<Program>,
        closure: Rc<Closure>,
        arguments: Vec<Value>,
        provided: Option<Vec<bool>>,
        span: Option<SourceSpan>,
        options: ClosureCallOptions,
    ) -> VmResult<Value> {
        let mut execution = self.module_closure_execution(
            program.clone(),
            closure,
            arguments,
            provided,
            span,
            options,
        )?;
        execution.vm.run_nested_execution(&execution.program)
    }

    fn spawn_task(&mut self, program: &Program, span: Option<SourceSpan>) -> VmResult<()> {
        let closure = self.pop(span.clone())?;
        let Value::Closure(closure) = closure else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                "spawn expects a function or block".into(),
                span,
            ));
        };
        let admission = if let Some(limit) = self.direct_task_limit {
            let count = self.direct_task_count.as_ref().ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "limited nursery is missing its task counter".into(),
                    span.clone(),
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
        let execution = self.module_closure_execution(
            Rc::new(program.clone()),
            closure,
            Vec::new(),
            None,
            span.clone(),
            ClosureCallOptions {
                direct_task_limit: None,
                direct_task_count: None,
                nursery: self.nursery.clone(),
                settle_nursery: false,
            },
        )?;
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
        self.nursery.make_progress()
    }

    fn settle_tasks(&self, result: VmResult<Value>) -> VmResult<Value> {
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

    fn run_nursery(
        &mut self,
        program: &Program,
        has_limit: bool,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let closure = self.pop(span.clone())?;
        let limit = has_limit.then(|| self.pop(span.clone())).transpose()?;
        let limit = if let Some(limit) = limit {
            let Value::Int(limit) = limit else {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    "nursery limit expects an integer".into(),
                    span,
                ));
            };
            if limit < 0 {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    "nursery limit must not be negative".into(),
                    span,
                ));
            }
            if limit == 0 {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    "nursery limit must be positive".into(),
                    span,
                ));
            }
            Some(usize::try_from(limit).map_err(|_| {
                self.error(
                    RuntimeErrorKind::Type,
                    "nursery limit is too large".into(),
                    span.clone(),
                )
            })?)
        } else {
            None
        };
        let Value::Closure(closure) = closure else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                "nursery expects a function or block".into(),
                span,
            ));
        };
        let nursery = Rc::new(Nursery::explicit());
        let execution = self.module_closure_execution(
            Rc::new(program.clone()),
            closure,
            Vec::new(),
            None,
            span.clone(),
            ClosureCallOptions {
                direct_task_limit: limit,
                direct_task_count: limit.map(|_| Rc::new(Cell::new(0))),
                nursery: nursery.clone(),
                settle_nursery: true,
            },
        )?;
        let body = Rc::new(Task::pending(execution, None, nursery.ready_queue()));
        nursery.enqueue(body.clone());
        nursery.run_task(&body);
        if body.is_pending() {
            let blocked = self.error(
                RuntimeErrorKind::InvalidCall,
                "task remains blocked with no runnable work".into(),
                span,
            );
            nursery.cancel_all(&blocked);
            body.cancel(&blocked);
            return Err(blocked);
        }
        let value = body.outcome().ok_or_else(|| {
            self.error(
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

    fn import(&mut self, kinds: Vec<CallArgumentKind>, span: Option<SourceSpan>) -> VmResult<()> {
        let values = self.pop_values(kinds.len(), span.clone())?;
        let (names, named_arguments) = self.expand_call_arguments(values, kinds, span.clone())?;
        if !named_arguments.is_empty() {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                "import does not accept named arguments".into(),
                span,
            ));
        }
        if names.is_empty() {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                "import expects at least one module name".into(),
                span,
            ));
        }
        let loader = self.module_loader.clone().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::Module,
                "module loader is not configured".into(),
                span.clone(),
            )
        })?;
        let importer = span.as_ref().map(|span| Path::new(&span.path));
        let mut exports = Vec::new();
        for name in names {
            let Value::Str(name) = name else {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    format!(
                        "import expects string module names, got {}",
                        name.type_name()
                    ),
                    span,
                ));
            };
            let instance = loader.initialize(importer, &name).map_err(|error| {
                self.error(RuntimeErrorKind::Module, error.to_string(), span.clone())
            })?;
            let Value::Map(module_exports) = instance.live_exports else {
                return Err(self.error(
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
        self.stack.push(Value::Map(Rc::new(exports)));
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

    fn list_spread(&mut self, spreads: Vec<bool>, span: Option<SourceSpan>) -> VmResult<()> {
        let values = self.pop_values(spreads.len(), span.clone())?;
        let mut result = Vec::new();
        for (value, spread) in values.into_iter().zip(spreads) {
            if spread {
                let Value::List(values) = value else {
                    return Err(self.error(
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
        self.stack.push(Value::List(Rc::new(result)));
        Ok(())
    }

    fn call_spread(
        &mut self,
        program: &Program,
        kinds: Vec<CallArgumentKind>,
        selected: Option<&CallableIdentity>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let required = kinds.len().checked_add(1).ok_or_else(|| {
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
        let values = self.stack.split_off(base + 1);
        self.stack.truncate(base);
        let (positional, named) = self.expand_call_arguments(values, kinds, span.clone())?;
        let (callee, arguments, provided) = if let Value::Overloads(overloads) = &callee {
            if let Some(selected) = selected {
                self.bind_selected_overload_arguments(
                    program,
                    overloads,
                    selected,
                    &positional,
                    &named,
                    span.clone(),
                )?
            } else {
                self.bind_overload_arguments(program, overloads, &positional, &named, span.clone())?
            }
        } else {
            if let Some(selected) = selected
                && Self::callable_signature(&callee)
                    .and_then(|signature| signature.identity)
                    .as_ref()
                    != Some(selected)
            {
                return Err(self.error(
                    RuntimeErrorKind::InvalidCall,
                    "selected callable signature is no longer present in the live binding".into(),
                    span,
                ));
            }
            let (arguments, provided) =
                self.bind_call_arguments(program, &callee, positional, named, span.clone())?;
            (callee, arguments, provided)
        };
        self.stack.push(callee);
        self.stack.extend(arguments);
        let count = self.stack.len() - base - 1;
        self.call(program, count, Some(provided), span)
    }

    fn pipeline_call(
        &mut self,
        program: &Program,
        kinds: Vec<CallArgumentKind>,
        selected: Option<&CallableIdentity>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let required = kinds.len().checked_add(2).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline argument count is too large".into(),
                span.clone(),
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        let arguments = self.stack.split_off(base + 2);
        let callee = self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        let value = self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        self.stack.push(callee);
        self.stack.push(value);
        self.stack.extend(arguments);
        let mut kinds = kinds;
        kinds.insert(0, CallArgumentKind::Positional);
        self.call_spread(program, kinds, selected, span)
    }

    fn expand_call_arguments(
        &self,
        values: Vec<Value>,
        kinds: Vec<CallArgumentKind>,
        span: Option<SourceSpan>,
    ) -> VmResult<ExpandedCallArguments> {
        let mut positional = Vec::new();
        let mut named = Vec::new();
        for (value, kind) in values.into_iter().zip(kinds) {
            let value = value
                .resolve()
                .map_err(|message| self.error(RuntimeErrorKind::Name, message, span.clone()))?;
            match kind {
                CallArgumentKind::Positional => positional.push(value),
                CallArgumentKind::Spread => {
                    let Value::List(values) = value else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            "call spread expects a list".into(),
                            span,
                        ));
                    };
                    positional.extend(values.iter().cloned());
                }
                CallArgumentKind::Named(name) => named.push((name, value)),
            }
        }
        Ok((positional, named))
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
            Builtin::Argv => {
                let configuration = self.configuration(span.clone())?;
                if !arguments.is_empty() {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!("`argv` expects no arguments, got {}", arguments.len()),
                        span,
                    ));
                }
                Ok(Value::List(
                    configuration
                        .arguments()
                        .iter()
                        .map(|argument| Value::string(argument.as_str()))
                        .collect::<Vec<_>>()
                        .into(),
                ))
            }
            Builtin::Argm => {
                let configuration = self.configuration(span.clone())?;
                if !arguments.is_empty() {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!("`argm` expects no arguments, got {}", arguments.len()),
                        span,
                    ));
                }
                Ok(configuration.argument_map())
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
    fn select(&mut self, cases: &[SelectCase], span: Option<SourceSpan>) -> VmResult<()> {
        if cases.is_empty() {
            return Err(self.error(
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
            let handler = has_handler.then(|| self.pop(span.clone())).transpose()?;
            let value = match case {
                SelectCase::Receive { .. } => {
                    let value = self.pop(span.clone())?;
                    let Value::Channel(channel) = value else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            format!("select recv expects chan, got {}", value.type_name()),
                            span,
                        ));
                    };
                    RuntimeSelectCase::Receive { channel, handler }
                }
                SelectCase::Send { .. } => {
                    let value = self.pop(span.clone())?;
                    let channel = self.pop(span.clone())?;
                    let Value::Channel(channel) = channel else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            format!("select send expects chan, got {}", channel.type_name()),
                            span,
                        ));
                    };
                    if matches!(value, Value::Nil) {
                        return Err(self.error(
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
                SelectCase::After { .. } => {
                    let duration = self.pop(span.clone())?;
                    let Value::Int(milliseconds) = duration else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            format!("select after expects num, got {}", duration.type_name()),
                            span,
                        ));
                    };
                    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
                        self.error(
                            RuntimeErrorKind::Type,
                            "select after must not be negative or too large".into(),
                            span.clone(),
                        )
                    })?;
                    RuntimeSelectCase::After {
                        deadline: Instant::now()
                            .checked_add(Duration::from_millis(milliseconds))
                            .ok_or_else(|| {
                                self.error(
                                    RuntimeErrorKind::Type,
                                    "select after is too large".into(),
                                    span.clone(),
                                )
                            })?,
                        handler,
                    }
                }
                SelectCase::Await { .. } => {
                    let value = self.pop(span.clone())?;
                    let Value::Task(task) = value else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            format!("select await expects task, got {}", value.type_name()),
                            span,
                        ));
                    };
                    RuntimeSelectCase::Await { task, handler }
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
                    self.nursery.track_native_channel(channel);
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
                    self.nursery.track_native_channel(channel);
                    match channel.try_send(value.clone()) {
                        ChannelSend::Ready => {
                            self.push_select_result(Value::Nil, handler.clone());
                            return Ok(());
                        }
                        ChannelSend::Closed => {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidCall,
                                "send on a closed channel".into(),
                                span,
                            ));
                        }
                        ChannelSend::Pending => {}
                    }
                }
                RuntimeSelectCase::Await { task, handler } => {
                    if task.is_running() {
                        return Err(self.error(
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

        let base = self.current_waiter(span.clone())?;
        let select_state = WaitSet::select_state(base.clone());
        let mut registrations = Vec::new();
        for case in values {
            match case {
                RuntimeSelectCase::Receive { channel, handler } => {
                    let waiter = Waiter::Select {
                        state: select_state.clone(),
                        wake: SelectWake::Value { handler },
                    };
                    channel.park_receiver(waiter);
                    registrations.push(WaitRegistration::ChannelReceive(channel));
                }
                RuntimeSelectCase::Send {
                    channel,
                    value,
                    handler,
                } => {
                    let waiter = Waiter::Select {
                        state: select_state.clone(),
                        wake: SelectWake::Value { handler },
                    };
                    waiter.set_closed_send_error(self.error(
                        RuntimeErrorKind::InvalidCall,
                        "send on a closed channel".into(),
                        span.clone(),
                    ));
                    channel.park_sender(waiter, value);
                    registrations.push(WaitRegistration::ChannelSend(channel));
                }
                RuntimeSelectCase::Await { task, handler } => {
                    task.wait_for(Waiter::Select {
                        state: select_state.clone(),
                        wake: SelectWake::TaskAwait {
                            handler,
                            observer: task.observer(),
                        },
                    });
                    registrations.push(WaitRegistration::TaskAwait(task));
                }
                RuntimeSelectCase::After { deadline, handler } => {
                    self.nursery.timer_service().borrow_mut().register(
                        deadline,
                        Waiter::Select {
                            state: select_state.clone(),
                            wake: SelectWake::Value { handler },
                        },
                    );
                    registrations.push(WaitRegistration::Timer(self.nursery.timer_service()));
                }
                RuntimeSelectCase::Default { .. } => {}
            }
        }
        let registrations = WaitSet::many(registrations);
        WaitSet::set_select_registrations(&select_state, registrations.clone());
        self.wait_registration = Some(registrations);
        self.stack.push(Value::Nil);
        self.suspension = Some(Suspension::Select(span));
        Ok(())
    }

    fn push_select_result(&mut self, value: Value, handler: Option<Value>) {
        self.stack.push(Value::List(
            vec![value, handler.unwrap_or(Value::Nil)].into(),
        ));
    }

    fn select_apply(&mut self, program: &Program, span: Option<SourceSpan>) -> VmResult<()> {
        let selected = self.pop(span.clone())?;
        let Value::List(values) = selected else {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "select result is invalid".into(),
                span,
            ));
        };
        if values.len() != 2 {
            return Err(self.error(
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
            self.call(program, 1, None, span)?;
        }
        Ok(())
    }

    fn current_waiter(&self, span: Option<SourceSpan>) -> VmResult<Waiter> {
        self.current_waiter.clone().ok_or_else(|| {
            self.error(
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
        span: Option<SourceSpan>,
    ) -> VmResult<Value> {
        match function.invoke(arguments) {
            NativeInvocation::Result(value, resources) => {
                self.native_resources.register(resources);
                if let Value::Channel(channel) = &value
                    && channel.has_native_producer()
                {
                    self.nursery.track_native_channel(channel);
                }
                Ok(value)
            }
            NativeInvocation::Error(error, resources) => {
                self.native_resources.register(resources);
                let (code, message, data) = error.into_parts();
                let mut error = self.error(
                    RuntimeErrorKind::Native,
                    format!("native `{}`: {message}", function.qualified_name()),
                    span,
                );
                error.native = Some(Box::new(NativeErrorDetails { code, data }));
                Err(error)
            }
            NativeInvocation::ContractViolation(message) => {
                Err(self.error(RuntimeErrorKind::NativeContract, message, span))
            }
        }
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
            bound[fixed] = Some(Value::List(Rc::new(rest)));
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
            bound[fixed] = Some(Value::List(Rc::new(rest)));
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

    fn bind_selected_overload_arguments(
        &self,
        program: &Program,
        overloads: &[Value],
        selected: &CallableIdentity,
        positional: &[Value],
        named: &[NamedArgument],
        span: Option<SourceSpan>,
    ) -> VmResult<(Value, Vec<Value>, Vec<bool>)> {
        for callee in overloads {
            let callee = callee
                .resolve()
                .map_err(|message| self.error(RuntimeErrorKind::Name, message, span.clone()))?;
            let identity =
                Self::callable_signature(&callee).and_then(|signature| signature.identity);
            if identity.as_ref() != Some(selected) {
                continue;
            }
            let (arguments, provided) = self.bind_call_arguments(
                program,
                &callee,
                positional.to_vec(),
                named.to_vec(),
                span.clone(),
            )?;
            return Ok((callee, arguments, provided));
        }
        Err(self.error(
            RuntimeErrorKind::InvalidCall,
            "selected callable signature is no longer present in the live binding".into(),
            span,
        ))
    }

    fn binary(
        &mut self,
        span: Option<SourceSpan>,
        operation: fn(Value, Value) -> Result<Value, (RuntimeErrorKind, String)>,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair(span.clone())?;
        self.stack.push(
            operation(left, right).map_err(|(kind, message)| self.error(kind, message, span))?,
        );
        Ok(())
    }
    fn compare(&mut self, span: Option<SourceSpan>, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair(span.clone())?;
        let result = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            left.cmp(right) == expected
        } else {
            let (left, right) = numbers(left, right)
                .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?;
            left.partial_cmp(&right)
                .is_some_and(|ordering| ordering == expected)
        };
        self.stack.push(Value::Bool(result));
        Ok(())
    }
    fn pop_pair(&mut self, span: Option<SourceSpan>) -> VmResult<(Value, Value)> {
        let right = self.pop(span.clone())?;
        let left = self.pop(span)?;
        Ok((left, right))
    }
    fn pop_values(&mut self, count: usize, span: Option<SourceSpan>) -> VmResult<Vec<Value>> {
        if self.stack.len() < count {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            ));
        }
        self.stack
            .split_off(self.stack.len() - count)
            .into_iter()
            .map(|value| {
                value
                    .resolve()
                    .map_err(|message| self.error(RuntimeErrorKind::Name, message, span.clone()))
            })
            .collect()
    }
    fn pop(&mut self, span: Option<SourceSpan>) -> VmResult<Value> {
        self.pop_unresolved(span.clone())?
            .resolve()
            .map_err(|message| self.error(RuntimeErrorKind::Name, message, span))
    }
    fn pop_unresolved(&mut self, span: Option<SourceSpan>) -> VmResult<Value> {
        self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }
    fn peek(&self, span: Option<SourceSpan>) -> VmResult<&Value> {
        self.stack.last().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }
    fn local(&self, slot: usize, span: Option<SourceSpan>) -> VmResult<BindingCell> {
        self.frames
            .last()
            .and_then(|frame| frame.locals.get(slot))
            .cloned()
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("local {slot} does not exist"),
                    span,
                )
            })
    }
    fn set_local(&mut self, slot: usize, value: Value, span: Option<SourceSpan>) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
                None,
            ));
        }
        *self
            .frames
            .last_mut()
            .expect("active frame was checked")
            .locals[slot]
            .borrow_mut() = value;
        Ok(())
    }
    fn jump(&mut self, target: usize, span: Option<SourceSpan>) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        self.frames.last_mut().expect("active frame was checked").ip = target;
        Ok(())
    }

    fn record_frame(&self, local_count: usize) {
        let mut metrics = self.metrics.borrow_mut();
        metrics.frames_created += 1;
        metrics.local_binding_cells_created += local_count;
    }

    fn error_at(
        &self,
        kind: RuntimeErrorKind,
        message: String,
        span: Option<&SourceSpan>,
    ) -> RuntimeError {
        self.error(kind, message, span.cloned())
    }

    fn owned_span(&self, span: Option<&SourceSpan>) -> Option<SourceSpan> {
        if span.is_some() {
            self.metrics.borrow_mut().source_span_clones += 1;
        }
        span.cloned()
    }

    fn pop_at(&mut self, span: Option<&SourceSpan>) -> VmResult<Value> {
        self.pop_unresolved_at(span)?
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
    }

    fn pop_unresolved_at(&mut self, span: Option<&SourceSpan>) -> VmResult<Value> {
        self.stack.pop().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }

    fn pop_values_at(&mut self, count: usize, span: Option<&SourceSpan>) -> VmResult<Vec<Value>> {
        if self.stack.len() < count {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            ));
        }
        self.stack
            .split_off(self.stack.len() - count)
            .into_iter()
            .map(|value| {
                value
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
            })
            .collect()
    }

    fn peek_at(&self, span: Option<&SourceSpan>) -> VmResult<&Value> {
        self.stack.last().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }

    fn local_at(&self, slot: usize, span: Option<&SourceSpan>) -> VmResult<BindingCell> {
        self.frames
            .last()
            .and_then(|frame| frame.locals.get(slot))
            .cloned()
            .ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("local {slot} does not exist"),
                    span,
                )
            })
    }

    fn set_local_at(
        &mut self,
        slot: usize,
        value: Value,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
                span,
            ));
        }
        *self
            .frames
            .last_mut()
            .expect("active frame was checked")
            .locals[slot]
            .borrow_mut() = value;
        Ok(())
    }

    fn pop_pair_at(&mut self, span: Option<&SourceSpan>) -> VmResult<(Value, Value)> {
        let right = self.pop_at(span)?;
        let left = self.pop_at(span)?;
        Ok((left, right))
    }

    fn binary_at(
        &mut self,
        span: Option<&SourceSpan>,
        operation: fn(Value, Value) -> Result<Value, (RuntimeErrorKind, String)>,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair_at(span)?;
        self.stack.push(
            operation(left, right).map_err(|(kind, message)| self.error_at(kind, message, span))?,
        );
        Ok(())
    }

    fn compare_at(&mut self, span: Option<&SourceSpan>, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair_at(span)?;
        let result = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            left.cmp(right) == expected
        } else {
            let (left, right) = numbers(left, right)
                .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?;
            left.partial_cmp(&right)
                .is_some_and(|ordering| ordering == expected)
        };
        self.stack.push(Value::Bool(result));
        Ok(())
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
