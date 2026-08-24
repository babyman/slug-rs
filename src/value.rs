use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    fmt,
    fmt::Write as _,
    rc::Rc,
};

use crate::native::{NativeFunction, NativeResource};

/// VM-owned builtins that require host-service context at call time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builtin {
    Cfg,
    Argv,
    Argm,
    Await,
    Channel,
    Send,
    Recv,
    Close,
}

/// A FIFO channel with bounded buffering and parked task wait queues.
#[derive(Clone)]
pub struct Channel {
    pub(crate) state: Rc<RefCell<ChannelState>>,
}

pub(crate) struct ChannelState {
    pub(crate) capacity: usize,
    pub(crate) messages: VecDeque<Value>,
    pub(crate) senders: VecDeque<(Rc<Task>, Value)>,
    pub(crate) receivers: VecDeque<Rc<Task>>,
    pub(crate) closed: bool,
}

impl Channel {
    #[must_use]
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            state: Rc::new(RefCell::new(ChannelState {
                capacity,
                messages: VecDeque::new(),
                senders: VecDeque::new(),
                receivers: VecDeque::new(),
                closed: false,
            })),
        }
    }
}

impl fmt::Debug for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<chan>")
    }
}

/// Shared storage for a lexical binding captured by one or more closures.
pub(crate) type BindingCell = Rc<RefCell<Value>>;
pub(crate) type GlobalEnvironment = Rc<RefCell<HashMap<String, Value>>>;

pub(crate) fn global_environment() -> GlobalEnvironment {
    Rc::new(RefCell::new(HashMap::new()))
}

pub(crate) fn binding_cell(value: Value) -> BindingCell {
    Rc::new(RefCell::new(value))
}

pub(crate) fn module_binding(name: impl Into<Rc<str>>) -> Value {
    let name = name.into();
    Value::Binding {
        name,
        cell: binding_cell(Value::Uninitialized),
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub(crate) chunk: usize,
    pub(crate) captures: Vec<BindingCell>,
    pub(crate) program: Option<Rc<crate::Program>>,
    pub(crate) globals: Option<GlobalEnvironment>,
    pub(crate) capture_sources: Vec<crate::Capture>,
}

/// A cached task completion. Tasks are runtime-owned and retain their outcome
/// so repeated awaits can observe the same settlement.
#[derive(Clone)]
pub struct Task {
    state: Rc<RefCell<TaskState>>,
}

struct TaskState {
    outcome: Option<Result<Value, crate::RuntimeError>>,
    pending: Option<crate::vm::TaskExecution>,
    running: bool,
    admission: Option<TaskAdmission>,
    admitted: bool,
    observed: bool,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    waiters: Vec<Rc<Task>>,
}

impl fmt::Debug for Task {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<task>")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TaskAdmission {
    pub(crate) limit: usize,
    pub(crate) count: Rc<Cell<usize>>,
}

impl Task {
    #[must_use]
    pub(crate) fn pending(
        execution: crate::vm::TaskExecution,
        admission: Option<TaskAdmission>,
        ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    ) -> Self {
        let admitted = admission.as_ref().is_none_or(|admission| {
            if admission.count.get() < admission.limit {
                admission.count.set(admission.count.get() + 1);
                true
            } else {
                false
            }
        });
        Self {
            state: Rc::new(RefCell::new(TaskState {
                outcome: None,
                pending: Some(execution),
                running: false,
                admission,
                admitted,
                observed: false,
                ready,
                waiters: Vec::new(),
            })),
        }
    }

    pub(crate) fn take_pending(&self, task: &Rc<Task>) -> Option<crate::vm::TaskExecution> {
        let mut state = self.state.borrow_mut();
        let mut pending = state.pending.take();
        if let Some(execution) = &mut pending {
            execution.set_current_task(task);
        }
        state.running = pending.is_some();
        pending
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state.borrow().running
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.state.borrow().pending.is_some()
    }

    pub(crate) fn try_admit(&self) -> bool {
        let mut state = self.state.borrow_mut();
        if state.pending.is_none() || state.admitted {
            return state.admitted;
        }
        let Some(admission) = &state.admission else {
            state.admitted = true;
            return true;
        };
        if admission.count.get() >= admission.limit {
            return false;
        }
        admission.count.set(admission.count.get() + 1);
        state.admitted = true;
        true
    }

    pub(crate) fn complete(&self, outcome: &Result<Value, crate::RuntimeError>) {
        let mut state = self.state.borrow_mut();
        state.running = false;
        state.outcome = Some(outcome.clone());
        release_admission(&mut state);
        for waiter in state.waiters.drain(..) {
            waiter.resume(outcome.clone());
        }
    }

    pub(crate) fn suspend(&self, execution: crate::vm::TaskExecution) {
        let mut state = self.state.borrow_mut();
        state.running = false;
        state.pending = Some(execution);
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        let mut state = self.state.borrow_mut();
        let Some(execution) = &mut state.pending else {
            return;
        };
        execution.resume(result);
        let ready = state.ready.clone();
        drop(state);
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    pub(crate) fn reject_closed_send(&self) {
        let mut state = self.state.borrow_mut();
        let Some(execution) = &mut state.pending else {
            return;
        };
        execution.reject_closed_send();
        let ready = state.ready.clone();
        drop(state);
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    pub(crate) fn wait_for(&self, waiter: Rc<Task>) {
        let mut state = self.state.borrow_mut();
        state.observed = true;
        state.waiters.push(waiter);
    }

    pub(crate) fn cancel(&self, error: crate::RuntimeError) {
        let mut state = self.state.borrow_mut();
        if state.pending.take().is_some() {
            state.running = false;
            state.outcome = Some(Err(error));
            release_admission(&mut state);
        }
    }

    pub(crate) fn await_outcome(&self) -> Option<Result<Value, crate::RuntimeError>> {
        let mut state = self.state.borrow_mut();
        state.observed = true;
        state.outcome.clone()
    }

    pub(crate) fn unobserved_error(&self) -> Option<crate::RuntimeError> {
        let state = self.state.borrow();
        if state.observed {
            None
        } else {
            state
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.as_ref().err().cloned())
        }
    }
}

fn release_admission(state: &mut TaskState) {
    if state.admitted {
        if let Some(admission) = &state.admission {
            admission.count.set(admission.count.get().saturating_sub(1));
        }
        state.admitted = false;
    }
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub(crate) name: Rc<str>,
    pub(crate) default: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct StructSchema {
    pub(crate) fields: Vec<StructField>,
}

#[derive(Clone, Debug)]
pub struct StructValue {
    pub(crate) schema: Rc<StructSchema>,
    pub(crate) values: Vec<Value>,
}

/// The dynamic values used by the initial Slug VM core.
///
/// Collections are reference-counted so closures and later concurrency support
/// can share them without requiring a copying garbage collector.
#[derive(Clone)]
pub enum Value {
    /// Internal marker for a predeclared module binding that has not run yet.
    Uninitialized,
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
    List(Rc<Vec<Value>>),
    Map(Rc<Vec<(Value, Value)>>),
    StructSchema(Rc<StructSchema>),
    Struct(Rc<StructValue>),
    Channel(Rc<Channel>),
    Closure(Rc<Closure>),
    Task(Rc<Task>),
    Native(NativeFunction),
    NativeResource(Rc<NativeResource>),
    Builtin(Builtin),
    /// Private callable group assembled by a multi-module import.
    Overloads(Rc<Vec<Value>>),
    /// A live module binding exposed through an import map.
    Binding {
        name: Rc<str>,
        cell: BindingCell,
    },
}

impl Value {
    #[must_use]
    pub fn string(value: impl Into<Rc<str>>) -> Self {
        Self::Str(value.into())
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    #[must_use]
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Uninitialized | Self::Binding { .. } => "binding",
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::Float(_) => "num",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::StructSchema(_) => "struct schema",
            Self::Struct(_) => "struct",
            Self::Channel(_) => "chan",
            Self::Closure(_) | Self::Native(_) | Self::Builtin(_) | Self::Overloads(_) => "fn",
            Self::Task(_) => "task",
            Self::NativeResource(_) => "native resource",
        }
    }

    pub(crate) fn resolve(&self) -> Result<Value, String> {
        let Self::Binding { name, cell } = self else {
            return Ok(self.clone());
        };
        let value = cell.borrow().clone();
        if matches!(value, Self::Uninitialized) {
            Err(format!("binding `{name}` is not initialized"))
        } else {
            Ok(value)
        }
    }

    pub(crate) fn replace_binding(&self, value: Value) -> bool {
        let Self::Binding { cell, .. } = self else {
            return false;
        };
        *cell.borrow_mut() = value;
        true
    }

    pub(crate) fn is_uninitialized_binding(&self) -> bool {
        matches!(self, Self::Binding { cell, .. } if matches!(*cell.borrow(), Self::Uninitialized))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Binding { cell, .. }, value) => cell.borrow().eq(value),
            (value, Self::Binding { cell, .. }) => value.eq(&cell.borrow()),
            (Self::Uninitialized, Self::Uninitialized) | (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) | (Self::Float(b), Self::Int(a)) => {
                int_as_float(*a) == *b
            }
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::List(a), Self::List(b)) | (Self::Overloads(a), Self::Overloads(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::StructSchema(a), Self::StructSchema(b)) => Rc::ptr_eq(a, b),
            (Self::Struct(a), Self::Struct(b)) => {
                Rc::ptr_eq(&a.schema, &b.schema) && a.values == b.values
            }
            (Self::Channel(a), Self::Channel(b)) => Rc::ptr_eq(a, b),
            (Self::Closure(a), Self::Closure(b)) => Rc::ptr_eq(a, b),
            (Self::Task(a), Self::Task(b)) => Rc::ptr_eq(a, b),
            (Self::Native(a), Self::Native(b)) => a.same_function(b),
            (Self::NativeResource(a), Self::NativeResource(b)) => Rc::ptr_eq(a, b),
            (Self::Builtin(a), Self::Builtin(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => write!(f, "<uninitialized>"),
            Self::Nil => write!(f, "nil"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bytes(value) => write!(f, "0x\"{}\"", hex(value)),
            Self::List(values) => f.debug_list().entries(values.iter()).finish(),
            Self::Map(entries) => f
                .debug_map()
                .entries(entries.iter().map(|(key, value)| (key, value)))
                .finish(),
            Self::StructSchema(_) => write!(f, "<struct schema>"),
            Self::Struct(value) => {
                write!(f, "struct ")?;
                f.debug_map()
                    .entries(
                        value
                            .schema
                            .fields
                            .iter()
                            .zip(&value.values)
                            .map(|(field, value)| (&field.name, value)),
                    )
                    .finish()
            }
            Self::Channel(_) => write!(f, "<chan>"),
            Self::Closure(_) => write!(f, "<fn>"),
            Self::Task(_) => write!(f, "<task>"),
            Self::Native(function) => write!(f, "<native {}>", function.qualified_name()),
            Self::NativeResource(_) => write!(f, "<native resource>"),
            Self::Builtin(builtin) => write!(f, "<builtin {builtin:?}>"),
            Self::Overloads(_) => write!(f, "<overloads>"),
            Self::Binding { name, .. } => write!(f, "<binding {name}>"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(value) => write!(f, "{value}"),
            value => write!(f, "{value:?}"),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing into String cannot fail");
    }
    output
}

#[allow(clippy::cast_precision_loss)]
fn int_as_float(value: i64) -> f64 {
    value as f64
}
