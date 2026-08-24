use std::{
    cell::{Cell, RefCell},
    cmp::Ordering,
    collections::{HashSet, VecDeque},
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::{
    CallArgumentKind, Capture, ModuleDeclaration, ModuleLoader, NativeDescriptorError,
    NativeFunction, Program, SourceSpan, Task, Value,
    bytecode::{Op, SelectCase},
    native::{NativeInvocation, NativeResourceRegistry, native_resource_registry},
    value::{
        BindingCell, Builtin, Channel, Closure, GlobalEnvironment, RootWaiter, TaskAdmission,
        TimerService, WaitRegistration, WaitSet, Waiter, binding_cell, global_environment,
        module_binding,
    },
};

mod cleanup;
mod error;
mod operations;

use cleanup::{Cleanup, Deferred};
pub use error::{CallFrame, NativeErrorDetails, RuntimeError, RuntimeErrorKind};
use operations::{
    add, bit_not, bitwise, construct_struct, copy_struct, divide, index_value, is_map_key,
    list_append, list_prepend, matches_pattern, modulo, multiply, negate, numbers, shift,
    slice_value, subtract,
};

pub type VmResult<T> = Result<T, RuntimeError>;

type NamedArgument = (String, Value);
type ExpandedCallArguments = (Vec<Value>, Vec<NamedArgument>);

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

struct Nursery {
    tasks: RefCell<Vec<Rc<Task>>>,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    fail_fast: bool,
    timers: Rc<RefCell<TimerService>>,
}

impl Nursery {
    fn root() -> Self {
        Self {
            tasks: RefCell::new(Vec::new()),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            fail_fast: false,
            timers: Rc::new(RefCell::new(TimerService::new())),
        }
    }

    fn explicit() -> Self {
        Self {
            tasks: RefCell::new(Vec::new()),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            fail_fast: true,
            timers: Rc::new(RefCell::new(TimerService::new())),
        }
    }
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

#[derive(Clone)]
enum Suspension {
    Await,
    Receive,
    Send(Option<SourceSpan>),
    Select,
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
            Some(Suspension::Send(span)) => span.clone(),
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
        for name in names {
            vm.globals
                .borrow_mut()
                .insert(name.clone(), module_binding(name.as_str()));
        }
        vm
    }

    pub(crate) fn run_module(&mut self, program: &Rc<Program>) -> VmResult<Value> {
        self.module_program = Some(program.clone());
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
    /// The descriptor retains its module-qualified identity. This adapter uses
    /// only its local binding name until `foreign` declarations gain a
    /// module-qualified registry.
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
        self.globals
            .borrow_mut()
            .insert("await".into(), Value::Builtin(Builtin::Await));
        self.globals
            .borrow_mut()
            .insert("channel".into(), Value::Builtin(Builtin::Channel));
        self.globals
            .borrow_mut()
            .insert("send".into(), Value::Builtin(Builtin::Send));
        self.globals
            .borrow_mut()
            .insert("recv".into(), Value::Builtin(Builtin::Recv));
        self.globals
            .borrow_mut()
            .insert("close".into(), Value::Builtin(Builtin::Close));
    }

    /// Executes a zero-argument entry chunk.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is invalid or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run(&mut self, program: &Program, entry: usize) -> VmResult<Value> {
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
        self.nursery.tasks.borrow_mut().clear();
        self.nursery.ready.borrow_mut().clear();
        self.module_metadata = program.declarations().to_vec();
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
            let (op, span) = self.next_instruction(program)?;
            match op {
                Op::Constant(index) => {
                    let chunk = self.current_chunk(program)?;
                    let value = match chunk.constants.get(index) {
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
                    for (index, text) in parts.into_iter().enumerate() {
                        output.push_str(&text);
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
                Op::GetLocal(slot) => self.stack.push(self.local(slot, span)?.borrow().clone()),
                Op::SetLocal(slot) => {
                    let value = self.pop(span.clone())?;
                    self.set_local(slot, value, span)?;
                }
                Op::GetCapture(slot) => {
                    let value = self
                        .frames
                        .last()
                        .and_then(|frame| frame.closure.captures.get(slot))
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
                        .and_then(|frame| frame.closure.captures.get(slot))
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
                        .get(&name)
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
                    if self.imported_globals.remove(&name) {
                        self.warning(format!(
                            "local binding `{name}` shadows an imported binding"
                        ));
                    }
                    if !self
                        .globals
                        .borrow()
                        .get(&name)
                        .is_some_and(|binding| binding.replace_binding(value.clone()))
                    {
                        self.globals.borrow_mut().insert(name, value);
                    }
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
                    let arguments = self.pop_values(arguments, span.clone())?;
                    if self
                        .module_metadata
                        .get(declaration)
                        .is_none_or(|declaration| declaration.tags.get(tag).is_none())
                    {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "module tag metadata does not exist".into(),
                            span,
                        ));
                    }
                    self.module_metadata[declaration].tags[tag].arguments = arguments;
                }
                Op::SetGlobal(name) => {
                    if !self.globals.borrow().contains_key(&name) {
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
                        .get(&name)
                        .is_some_and(|binding| binding.replace_binding(value.clone()))
                    {
                        self.globals.borrow_mut().insert(name, value);
                    }
                }
                Op::MakeClosure { chunk, captures } => {
                    program.chunk(chunk).ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("function chunk {chunk} does not exist"),
                            span.clone(),
                        )
                    })?;
                    let capture_sources = captures.clone();
                    let captures = captures
                        .into_iter()
                        .map(|capture| match capture {
                            Capture::Local(slot) => self.local(slot, span.clone()),
                            Capture::Capture(slot) => self
                                .frames
                                .last()
                                .and_then(|frame| frame.closure.captures.get(slot))
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
                        chunk,
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
                    let values = self.pop_values(count, span.clone())?;
                    self.stack.push(Value::List(Rc::new(values)));
                }
                Op::ListSpread(spreads) => self.list_spread(spreads, span)?,
                Op::Map(count) => {
                    let values = self.pop_values(count.saturating_mul(2), span.clone())?;
                    let mut entries = Vec::with_capacity(count);
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
                            name: field.name.into(),
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
                        construct_struct(schema, &fields, &values)
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::StructCopy(fields) => {
                    let replacements = self.pop_values(fields.len(), span.clone())?;
                    let value = self.pop(span.clone())?;
                    self.stack.push(
                        copy_struct(value, &fields, &replacements)
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
                        usize::from(has_start) + usize::from(has_end) + usize::from(has_step);
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
                Op::Jump(target) => self.jump(target, span)?,
                Op::JumpIfFalse(target) => {
                    if !self.peek(span.clone())?.is_truthy() {
                        self.jump(target, span)?;
                    }
                }
                Op::JumpIfProvided { slot, target } => {
                    if self
                        .frames
                        .last()
                        .and_then(|frame| frame.provided.get(slot))
                        .copied()
                        == Some(true)
                    {
                        self.jump(target, span)?;
                    }
                }
                Op::Call(count) => self.call(program, count, None, span)?,
                Op::CallSpread(kinds) => self.call_spread(program, kinds, span)?,
                Op::PipelineCall(kinds) => self.pipeline_call(program, kinds, span)?,
                Op::Import(kinds) => self.import(kinds, span)?,
                Op::Spawn => self.spawn_task(program, span)?,
                Op::Nursery { has_limit } => self.run_nursery(program, has_limit, span)?,
                Op::Select(cases) => self.select(&cases, span)?,
                Op::SelectApply => self.select_apply(program, span)?,
                Op::TryMatch {
                    pattern,
                    bindings,
                    operands,
                } => {
                    let operands = self.pop_values(operands, span.clone())?;
                    let value = self.pop(span.clone())?;
                    let mut values = Vec::new();
                    let matched = matches_pattern(&pattern, &value, &operands, &mut values)
                        .map_err(|(kind, message)| self.error(kind, message, span.clone()))?;
                    if matched && values.len() != bindings {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "match pattern binding count is invalid".into(),
                            span,
                        ));
                    }
                    if matched {
                        self.stack.extend(values);
                    } else {
                        self.stack.extend((0..bindings).map(|_| Value::Nil));
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
                        Value::Closure(_) | Value::Native(_) | Value::Builtin(_)
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
                    scope.push(Deferred { action, mode });
                }
                Op::Recur(kinds) => self.recur(program, kinds, span)?,
                Op::Return => {
                    let value = self.pop(span.clone())?;
                    if let Some(value) = self.begin_return(program, value)? {
                        return Ok(ExecutionOutcome::Settled(Ok(value)));
                    }
                }
            }
        }
    }

    fn next_instruction(&mut self, program: &Program) -> VmResult<(Op, Option<SourceSpan>)> {
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
        let instruction = chunk
            .code
            .get(ip)
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("function `{}` ended without Return", chunk.name),
                    None,
                )
            })?
            .clone();
        self.frames.last_mut().expect("active frame was checked").ip += 1;
        Ok((instruction.op, instruction.span))
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
            Value::Native(function) => {
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
        };
        let mut locals = arguments.into_iter().map(binding_cell).collect::<Vec<_>>();
        locals.resize_with(chunk.locals, || binding_cell(Value::Nil));
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
        match self
            .module_closure_execution(program.clone(), closure, arguments, provided, span, options)?
            .run()
        {
            TaskRunOutcome::Settled(result) => result,
            TaskRunOutcome::Suspended(_) => Err(self.error(
                RuntimeErrorKind::InvalidCall,
                "blocking operations require a spawned task".into(),
                None,
            )),
        }
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
            self.nursery.ready.clone(),
        ));
        self.nursery.tasks.borrow_mut().push(task.clone());
        self.nursery.ready.borrow_mut().push_back(task.clone());
        self.stack.push(Value::Task(task));
        Ok(())
    }

    fn run_task(&self, task: &Task) {
        while task.is_pending() {
            if !self.make_progress() {
                return;
            }
        }
    }

    fn run_next_ready_task(&self) -> bool {
        let Some(next) = self.next_ready_task() else {
            return false;
        };
        let Some(run) = next.take_pending(&next) else {
            return true;
        };
        match run.run() {
            TaskRunOutcome::Settled(result) => next.complete(&result),
            TaskRunOutcome::Suspended(mut execution) => {
                let wait_registration = execution.take_wait_registration();
                next.suspend(*execution, wait_registration);
            }
        }
        true
    }

    fn make_progress(&self) -> bool {
        self.run_next_ready_task() || self.wait_for_timer()
    }

    fn wait_for_timer(&self) -> bool {
        if self.wake_due_timers() {
            return true;
        }
        let Some(deadline) = self.nursery.timers.borrow().next_deadline() else {
            return false;
        };
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline.duration_since(now));
        }
        self.wake_due_timers()
    }

    fn wake_due_timers(&self) -> bool {
        let due = self.nursery.timers.borrow_mut().take_due();
        let woke = !due.is_empty();
        for waiter in due {
            waiter.resume(Ok(Value::Nil));
        }
        woke
    }

    fn next_ready_task(&self) -> Option<Rc<Task>> {
        let mut ready = self.nursery.ready.borrow_mut();
        let candidates = ready.len();
        for _ in 0..candidates {
            let task = ready.pop_front().expect("ready queue length was checked");
            if !task.is_pending() {
                continue;
            }
            if task.try_admit() {
                return Some(task);
            }
            ready.push_back(task);
        }
        None
    }

    fn settle_tasks(&self, result: VmResult<Value>) -> VmResult<Value> {
        match result {
            Ok(value) => {
                let mut index = 0;
                while let Some(task) = self.nursery.tasks.borrow().get(index).cloned() {
                    self.run_task(&task);
                    if self.nursery.fail_fast {
                        let tasks = self.nursery.tasks.borrow().clone();
                        if let Some(error) = tasks.iter().find_map(|task| task.unobserved_error()) {
                            let cancellation = self.error(
                                RuntimeErrorKind::Thrown,
                                "sibling cancelled due to fail-fast".into(),
                                None,
                            );
                            for sibling in &tasks {
                                sibling.cancel(&cancellation);
                            }
                            return Err(error);
                        }
                    }
                    if task.is_pending() {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidCall,
                            "task remains blocked with no runnable work".into(),
                            None,
                        ));
                    }
                    index += 1;
                }
                self.nursery
                    .tasks
                    .borrow()
                    .iter()
                    .find_map(|task| task.unobserved_error())
                    .map_or(Ok(value), Err)
            }
            Err(error) => Err(error),
        }
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
        let value = self.call_module_closure(
            &Rc::new(program.clone()),
            closure,
            Vec::new(),
            None,
            span,
            ClosureCallOptions {
                direct_task_limit: limit,
                direct_task_count: limit.map(|_| Rc::new(Cell::new(0))),
                nursery: Rc::new(Nursery::explicit()),
                settle_nursery: true,
            },
        )?;
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

    fn callable_signature(value: &Value) -> Option<Vec<(bool, bool)>> {
        let Value::Closure(closure) = value.resolve().ok()? else {
            return None;
        };
        let program = closure.program.as_deref()?;
        program.chunk(closure.chunk).map(|chunk| {
            chunk
                .parameters
                .iter()
                .map(|parameter| (parameter.has_default, parameter.variadic))
                .collect()
        })
    }

    fn callable_signatures(value: &Value) -> Option<Vec<Vec<(bool, bool)>>> {
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
            self.bind_overload_arguments(program, overloads, &positional, &named, span.clone())?
        } else {
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
        self.call_spread(program, kinds, span)
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
            Builtin::Await => {
                if arguments.len() != 1 {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!("`await` expects 1 argument, got {}", arguments.len()),
                        span,
                    ));
                }
                let Value::Task(task) = &arguments[0] else {
                    return Err(self.error(
                        RuntimeErrorKind::Type,
                        format!("await expects task, got {}", arguments[0].type_name()),
                        span,
                    ));
                };
                self.run_task(task);
                if task.is_running() {
                    return Err(self.error(
                        RuntimeErrorKind::InvalidCall,
                        "task cannot await itself while it is running".into(),
                        span,
                    ));
                }
                if let Some(outcome) = task.await_outcome() {
                    return outcome;
                }
                let waiter = self.current_waiter(span.clone())?;
                if matches!(waiter, Waiter::Task(_)) {
                    self.wait_registration =
                        Some(WaitSet::one(WaitRegistration::TaskAwait(task.clone())));
                }
                task.wait_for(waiter);
                self.suspension = Some(Suspension::Await);
                Ok(Value::Nil)
            }
            Builtin::Channel => self.channel(arguments, span),
            Builtin::Send => self.send(arguments, span),
            Builtin::Recv => self.recv(arguments, span),
            Builtin::Close => self.close_channel(arguments, span),
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
                    channel.drain_native();
                    let mut state = channel.state.borrow_mut();
                    if let Some(value) = state.messages.pop_front() {
                        if let Some((sender, pending)) = state.senders.pop_front() {
                            state.messages.push_back(pending);
                            drop(state);
                            sender.resume(Ok(Value::Nil));
                        }
                        self.push_select_result(value, handler.clone());
                        return Ok(());
                    }
                    if let Some((sender, value)) = state.senders.pop_front() {
                        drop(state);
                        sender.resume(Ok(Value::Nil));
                        self.push_select_result(value, handler.clone());
                        return Ok(());
                    }
                    if state.closed {
                        self.push_select_result(Value::Nil, handler.clone());
                        return Ok(());
                    }
                }
                RuntimeSelectCase::Send {
                    channel,
                    value,
                    handler,
                } => {
                    let mut state = channel.state.borrow_mut();
                    if state.closed {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidCall,
                            "send on a closed channel".into(),
                            span,
                        ));
                    }
                    if let Some(receiver) = state.receivers.pop_front() {
                        drop(state);
                        receiver.resume(Ok(value.clone()));
                        self.push_select_result(Value::Nil, handler.clone());
                        return Ok(());
                    }
                    if state.messages.len() < state.capacity {
                        state.messages.push_back(value.clone());
                        self.push_select_result(Value::Nil, handler.clone());
                        return Ok(());
                    }
                }
                RuntimeSelectCase::Await { task, handler } => {
                    self.run_task(task);
                    if task.is_running() {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidCall,
                            "task cannot await itself while it is running".into(),
                            span,
                        ));
                    }
                    if let Some(outcome) = task.await_outcome() {
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
                        handler,
                    };
                    channel.state.borrow_mut().receivers.push_back(waiter);
                    registrations.push(WaitRegistration::ChannelReceive(channel));
                }
                RuntimeSelectCase::Send {
                    channel,
                    value,
                    handler,
                } => {
                    let waiter = Waiter::Select {
                        state: select_state.clone(),
                        handler,
                    };
                    waiter.set_closed_send_error(self.error(
                        RuntimeErrorKind::InvalidCall,
                        "send on a closed channel".into(),
                        span.clone(),
                    ));
                    channel
                        .state
                        .borrow_mut()
                        .senders
                        .push_back((waiter, value));
                    registrations.push(WaitRegistration::ChannelSend(channel));
                }
                RuntimeSelectCase::Await { task, handler } => {
                    task.wait_for(Waiter::Select {
                        state: select_state.clone(),
                        handler,
                    });
                    registrations.push(WaitRegistration::TaskAwait(task));
                }
                RuntimeSelectCase::After { deadline, handler } => {
                    self.nursery.timers.borrow_mut().register(
                        deadline,
                        Waiter::Select {
                            state: select_state.clone(),
                            handler,
                        },
                    );
                    registrations.push(WaitRegistration::Timer(self.nursery.timers.clone()));
                }
                RuntimeSelectCase::Default { .. } => {}
            }
        }
        let registrations = WaitSet::many(registrations);
        WaitSet::set_select_registrations(&select_state, registrations.clone());
        self.wait_registration = Some(registrations);
        self.stack.push(Value::Nil);
        self.suspension = Some(Suspension::Select);
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

    fn channel(&self, arguments: &[Value], span: Option<SourceSpan>) -> VmResult<Value> {
        if arguments.len() != 1 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`channel` expects 1 argument, got {}", arguments.len()),
                span,
            ));
        }
        let Value::Int(capacity) = arguments[0] else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                format!(
                    "channel capacity expects num, got {}",
                    arguments[0].type_name()
                ),
                span,
            ));
        };
        let capacity = usize::try_from(capacity).map_err(|_| {
            self.error(
                RuntimeErrorKind::Type,
                "channel capacity must not be negative or too large".into(),
                None,
            )
        })?;
        Ok(Value::Channel(Rc::new(Channel::new(capacity))))
    }

    fn send(&mut self, arguments: &[Value], span: Option<SourceSpan>) -> VmResult<Value> {
        if arguments.len() != 2 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`send` expects 2 arguments, got {}", arguments.len()),
                span,
            ));
        }
        let Value::Channel(channel) = &arguments[0] else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                format!("send expects chan, got {}", arguments[0].type_name()),
                span,
            ));
        };
        if matches!(arguments[1], Value::Nil) {
            return Err(self.error(RuntimeErrorKind::Type, "send cannot send nil".into(), span));
        }
        let mut state = channel.state.borrow_mut();
        if state.closed {
            return Err(self.error(
                RuntimeErrorKind::InvalidCall,
                "send on a closed channel".into(),
                span,
            ));
        }
        if let Some(receiver) = state.receivers.pop_front() {
            drop(state);
            receiver.resume(Ok(arguments[1].clone()));
            return Ok(Value::Nil);
        }
        if state.messages.len() < state.capacity {
            state.messages.push_back(arguments[1].clone());
            return Ok(Value::Nil);
        }
        let sender = self.current_waiter(span.clone())?;
        sender.set_closed_send_error(self.error(
            RuntimeErrorKind::InvalidCall,
            "send on a closed channel".into(),
            span.clone(),
        ));
        if matches!(sender, Waiter::Task(_)) {
            self.wait_registration =
                Some(WaitSet::one(WaitRegistration::ChannelSend(channel.clone())));
        }
        state.senders.push_back((sender, arguments[1].clone()));
        self.suspension = Some(Suspension::Send(span));
        Ok(Value::Nil)
    }

    fn recv(&mut self, arguments: &[Value], span: Option<SourceSpan>) -> VmResult<Value> {
        if arguments.len() != 1 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`recv` expects 1 argument, got {}", arguments.len()),
                span,
            ));
        }
        let Value::Channel(channel) = &arguments[0] else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                format!("recv expects chan, got {}", arguments[0].type_name()),
                span,
            ));
        };
        channel.drain_native();
        let mut state = channel.state.borrow_mut();
        if let Some(value) = state.messages.pop_front() {
            if let Some((sender, pending)) = state.senders.pop_front() {
                state.messages.push_back(pending);
                drop(state);
                sender.resume(Ok(Value::Nil));
            }
            return Ok(value);
        }
        if let Some((sender, value)) = state.senders.pop_front() {
            drop(state);
            sender.resume(Ok(Value::Nil));
            return Ok(value);
        }
        if state.closed {
            return Ok(Value::Nil);
        }
        let receiver = self.current_waiter(span.clone())?;
        if matches!(receiver, Waiter::Task(_)) {
            self.wait_registration = Some(WaitSet::one(WaitRegistration::ChannelReceive(
                channel.clone(),
            )));
        }
        state.receivers.push_back(receiver);
        self.suspension = Some(Suspension::Receive);
        Ok(Value::Nil)
    }

    fn close_channel(&mut self, arguments: &[Value], span: Option<SourceSpan>) -> VmResult<Value> {
        if arguments.len() != 1 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`close` expects 1 argument, got {}", arguments.len()),
                span,
            ));
        }
        let Value::Channel(channel) = &arguments[0] else {
            return Err(self.error(
                RuntimeErrorKind::Type,
                format!("close expects chan, got {}", arguments[0].type_name()),
                span,
            ));
        };
        let mut state = channel.state.borrow_mut();
        if state.closed {
            return Ok(Value::Nil);
        }
        state.closed = true;
        let receivers = state.receivers.drain(..).collect::<Vec<_>>();
        let senders = state
            .senders
            .drain(..)
            .map(|(sender, _)| sender)
            .collect::<Vec<_>>();
        drop(state);
        for receiver in receivers {
            receiver.resume(Ok(Value::Nil));
        }
        for sender in senders {
            sender.reject_closed_send();
        }
        Ok(Value::Nil)
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
}
