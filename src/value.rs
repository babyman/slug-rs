use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    fmt,
    fmt::Write as _,
    rc::{Rc, Weak},
    sync::Arc,
    time::Instant,
};

use crate::{
    native::{NativeChannelProducer, NativeFunction, NativeResource},
    scheduler_signal::SchedulerSignal,
    source::environment::CallableIdentity,
};

/// VM-owned builtins that require host-service context at call time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builtin {
    Cfg,
    Argv,
    Argm,
}

/// A FIFO channel with bounded buffering and parked task wait queues.
pub struct Channel {
    pub(crate) state: Rc<RefCell<ChannelState>>,
    native_producer: Option<NativeChannelProducer>,
}

pub(crate) struct ChannelState {
    pub(crate) capacity: usize,
    pub(crate) messages: VecDeque<Value>,
    pub(crate) senders: VecDeque<(Waiter, Value)>,
    pub(crate) receivers: VecDeque<Waiter>,
    pub(crate) closed: bool,
}

pub(crate) enum ChannelSend {
    Ready,
    Pending,
    Closed,
}

pub(crate) enum ChannelReceive {
    Ready(Value),
    Pending,
}

#[derive(Clone)]
pub(crate) enum Waiter {
    Task(Rc<Task>),
    Root(RootWaiter),
    Select {
        state: Rc<RefCell<SelectWaitState>>,
        wake: SelectWake,
    },
}

#[derive(Clone)]
pub(crate) enum SelectWake {
    Value {
        handler: Option<Value>,
    },
    TaskAwait {
        handler: Option<Value>,
        observer: TaskObserver,
    },
}

impl SelectWake {
    fn selected(&self) -> Option<Value> {
        match self {
            Self::Value { handler } => handler.clone(),
            Self::TaskAwait { handler, observer } => {
                observer.observe();
                handler.clone()
            }
        }
    }
}

pub(crate) struct SelectWaitState {
    waiter: Waiter,
    selected: bool,
    registrations: Option<WaitSet>,
}

impl Waiter {
    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        match self {
            Self::Task(task) => task.resume(result),
            Self::Root(root) => root.resume(result),
            Self::Select { state, wake } => {
                let mut state = state.borrow_mut();
                if state.selected {
                    return;
                }
                state.selected = true;
                let registrations = state.registrations.take();
                let waiter = state.waiter.clone();
                drop(state);
                let handler = wake.selected();
                if let Some(registrations) = registrations {
                    registrations.remove_for_waiter(&waiter);
                }
                waiter.resume(
                    result.map(|value| {
                        Value::List(vec![value, handler.unwrap_or(Value::Nil)].into())
                    }),
                );
            }
        }
    }

    pub(crate) fn set_closed_send_error(&self, error: crate::RuntimeError) {
        if let Self::Root(root) = self {
            root.set_closed_send_error(error);
        } else if let Self::Select { state, .. } = self {
            state.borrow().waiter.set_closed_send_error(error);
        }
    }

    pub(crate) fn reject_closed_send(&self) {
        match self {
            Self::Task(task) => task.reject_closed_send(),
            Self::Root(root) => root.reject_closed_send(),
            Self::Select { state, .. } => state.borrow().waiter.reject_closed_send(),
        }
    }

    fn is_same(&self, other: &Waiter) -> bool {
        match (self, other) {
            (Self::Task(left), Self::Task(right)) => Rc::ptr_eq(&left.state, &right.state),
            (Self::Root(left), Self::Root(right)) => Rc::ptr_eq(&left.state, &right.state),
            (Self::Select { state, .. }, other) => state.borrow().waiter.is_same(other),
            (other, Self::Select { state, .. }) => other.is_same(&state.borrow().waiter),
            _ => false,
        }
    }
}

#[derive(Clone)]
pub(crate) enum WaitRegistration {
    ChannelSend(Rc<Channel>),
    ChannelReceive(Rc<Channel>),
    TaskAwait(Rc<Task>),
    Timer(Rc<RefCell<TimerService>>),
}

impl WaitRegistration {
    fn remove(self, waiter: &Waiter) {
        match self {
            Self::ChannelSend(channel) => {
                channel
                    .state
                    .borrow_mut()
                    .senders
                    .retain(|(candidate, _)| !candidate.is_same(waiter));
            }
            Self::ChannelReceive(channel) => {
                channel
                    .state
                    .borrow_mut()
                    .receivers
                    .retain(|candidate| !candidate.is_same(waiter));
            }
            Self::TaskAwait(target) => {
                target
                    .state
                    .borrow_mut()
                    .waiters
                    .retain(|candidate| !candidate.is_same(waiter));
            }
            Self::Timer(timers) => {
                timers
                    .borrow_mut()
                    .waiters
                    .retain(|(_, candidate)| !candidate.is_same(waiter));
            }
        }
    }
}

/// Shared monotonic timer queue for one dynamic nursery.
pub(crate) struct TimerService {
    waiters: Vec<(Instant, Waiter)>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<crate::vm::VmMetrics>>,
}

impl TimerService {
    pub(crate) fn new(
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<crate::vm::VmMetrics>>,
    ) -> Self {
        Self {
            waiters: Vec::new(),
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn register(&mut self, deadline: Instant, waiter: Waiter) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_registrations += 1;
        }
        self.waiters.push((deadline, waiter));
    }

    pub(crate) fn take_due(&mut self) -> Vec<Waiter> {
        let now = Instant::now();
        let mut due = Vec::new();
        self.waiters.retain(|(deadline, waiter)| {
            if *deadline <= now {
                due.push(waiter.clone());
                false
            } else {
                true
            }
        });
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_wakeups += due.len();
        }
        due
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_deadline_lookups += 1;
        }
        self.waiters.iter().map(|(deadline, _)| *deadline).min()
    }
}

/// All queues a suspended execution currently occupies. A regular blocking
/// operation has one entry; a future select owns several and removes losers
/// before its winner resumes.
#[derive(Clone)]
pub(crate) struct WaitSet {
    registrations: Vec<WaitRegistration>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<crate::vm::VmMetrics>>,
}

impl WaitSet {
    pub(crate) fn many(
        registrations: Vec<WaitRegistration>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<crate::vm::VmMetrics>>,
    ) -> Self {
        Self {
            registrations,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn select_state(waiter: Waiter) -> Rc<RefCell<SelectWaitState>> {
        Rc::new(RefCell::new(SelectWaitState {
            waiter,
            selected: false,
            registrations: None,
        }))
    }

    pub(crate) fn set_select_registrations(
        state: &Rc<RefCell<SelectWaitState>>,
        registrations: WaitSet,
    ) {
        state.borrow_mut().registrations = Some(registrations);
    }

    fn remove(self, waiter: &Waiter) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().wait_registration_removals += self.registrations.len();
        }
        for registration in self.registrations {
            registration.remove(waiter);
        }
    }

    fn remove_losers(self, task: &Task) {
        if self.registrations.len() > 1 {
            self.remove(&Waiter::Task(Rc::new(task.clone())));
        }
    }

    pub(crate) fn remove_for_waiter(self, waiter: &Waiter) {
        self.remove(waiter);
    }
}

#[derive(Clone)]
pub(crate) struct RootWaiter {
    state: Rc<RefCell<RootWaiterState>>,
}

struct RootWaiterState {
    resume: Option<Result<Value, crate::RuntimeError>>,
    closed_send_error: Option<crate::RuntimeError>,
}

impl RootWaiter {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(RootWaiterState {
                resume: None,
                closed_send_error: None,
            })),
        }
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        self.state.borrow_mut().resume = Some(result);
    }

    pub(crate) fn set_closed_send_error(&self, error: crate::RuntimeError) {
        self.state.borrow_mut().closed_send_error = Some(error);
    }

    pub(crate) fn reject_closed_send(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.closed_send_error.clone() {
            state.resume = Some(Err(error));
        }
    }

    pub(crate) fn take_resume(&self) -> Option<Result<Value, crate::RuntimeError>> {
        self.state.borrow_mut().resume.take()
    }
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
            native_producer: None,
        }
    }

    pub(crate) fn native(capacity: usize) -> (Self, NativeChannelProducer) {
        let producer = NativeChannelProducer::bounded(capacity);
        (
            Self {
                state: Rc::new(RefCell::new(ChannelState {
                    capacity,
                    messages: VecDeque::new(),
                    senders: VecDeque::new(),
                    receivers: VecDeque::new(),
                    closed: false,
                })),
                native_producer: Some(producer.receiver_handle()),
            },
            producer,
        )
    }

    pub(crate) fn has_native_producer(&self) -> bool {
        self.native_producer.is_some()
    }

    pub(crate) fn revoke_native_producer(&self) {
        if let Some(producer) = &self.native_producer {
            producer.close();
        }
    }

    pub(crate) fn register_scheduler(&self, signal: &Arc<SchedulerSignal>) {
        if let Some(producer) = &self.native_producer {
            producer.register_scheduler(signal);
        }
    }

    pub(crate) fn has_live_native_producer(&self) -> bool {
        self.native_producer
            .as_ref()
            .is_some_and(NativeChannelProducer::has_external_producer)
    }

    pub(crate) fn reserve_buffer_slot(&self) -> bool {
        self.native_producer
            .as_ref()
            .is_none_or(NativeChannelProducer::reserve_slot)
    }

    pub(crate) fn release_buffer_slot(&self) {
        if let Some(producer) = &self.native_producer {
            producer.release_slot();
        }
    }

    pub(crate) fn drain_native(&self) -> bool {
        let Some(producer) = &self.native_producer else {
            return false;
        };
        let values = producer.drain(usize::MAX);
        let changed = !values.is_empty() || producer.is_closed();
        let mut state = self.state.borrow_mut();
        let mut resumed = Vec::new();
        let mut rejected_senders = Vec::new();
        for value in values {
            if let Some(receiver) = state.receivers.pop_front() {
                self.release_buffer_slot();
                resumed.push((receiver, value.into_value()));
            } else {
                state.messages.push_back(value.into_value());
            }
        }
        if producer.is_closed() {
            state.closed = true;
            resumed.extend(
                state
                    .receivers
                    .drain(..)
                    .map(|receiver| (receiver, Value::Nil)),
            );
            rejected_senders.extend(state.senders.drain(..).map(|(sender, _)| sender));
        }
        drop(state);
        for (receiver, value) in resumed {
            receiver.resume(Ok(value));
        }
        for sender in rejected_senders {
            sender.reject_closed_send();
        }
        changed
    }

    pub(crate) fn try_send(&self, value: Value) -> ChannelSend {
        self.drain_native();
        let mut state = self.state.borrow_mut();
        if state.closed {
            return ChannelSend::Closed;
        }
        if let Some(receiver) = state.receivers.pop_front() {
            drop(state);
            receiver.resume(Ok(value));
            return ChannelSend::Ready;
        }
        if state.messages.len() < state.capacity && self.reserve_buffer_slot() {
            state.messages.push_back(value);
            return ChannelSend::Ready;
        }
        ChannelSend::Pending
    }

    pub(crate) fn park_sender(&self, waiter: Waiter, value: Value) {
        self.state.borrow_mut().senders.push_back((waiter, value));
    }

    pub(crate) fn try_receive(&self) -> ChannelReceive {
        self.drain_native();
        let mut state = self.state.borrow_mut();
        if let Some(value) = state.messages.pop_front() {
            self.release_buffer_slot();
            if !state.senders.is_empty()
                && self.reserve_buffer_slot()
                && let Some((sender, pending)) = state.senders.pop_front()
            {
                state.messages.push_back(pending);
                drop(state);
                sender.resume(Ok(Value::Nil));
            }
            return ChannelReceive::Ready(value);
        }
        if let Some((sender, value)) = state.senders.pop_front() {
            drop(state);
            sender.resume(Ok(Value::Nil));
            return ChannelReceive::Ready(value);
        }
        if state.closed {
            return ChannelReceive::Ready(Value::Nil);
        }
        ChannelReceive::Pending
    }

    pub(crate) fn park_receiver(&self, waiter: Waiter) {
        self.state.borrow_mut().receivers.push_back(waiter);
    }

    pub(crate) fn close(&self) {
        self.revoke_native_producer();
        let mut state = self.state.borrow_mut();
        if state.closed {
            return;
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
    }
}

impl fmt::Debug for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<chan>")
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.revoke_native_producer();
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
    phase: TaskPhase,
    admission: Option<TaskAdmission>,
    admitted: bool,
    observed: bool,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    waiters: Vec<Waiter>,
    wait_registration: Option<WaitSet>,
}

enum TaskPhase {
    Pending(Box<crate::vm::TaskExecution>),
    Running,
    Settled(Result<Value, crate::RuntimeError>),
}

#[derive(Clone)]
pub(crate) struct TaskObserver(Weak<RefCell<TaskState>>);

impl TaskObserver {
    fn observe(&self) {
        if let Some(state) = self.0.upgrade() {
            state.borrow_mut().observed = true;
        }
    }
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
                phase: TaskPhase::Pending(Box::new(execution)),
                admission,
                admitted,
                observed: false,
                ready,
                waiters: Vec::new(),
                wait_registration: None,
            })),
        }
    }

    pub(crate) fn take_pending(&self, task: &Rc<Task>) -> Option<crate::vm::TaskExecution> {
        let mut state = self.state.borrow_mut();
        let phase = std::mem::replace(&mut state.phase, TaskPhase::Running);
        match phase {
            TaskPhase::Pending(mut execution) => {
                execution.set_current_task(task);
                Some(*execution)
            }
            phase => {
                state.phase = phase;
                None
            }
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self.state.borrow().phase, TaskPhase::Running)
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(self.state.borrow().phase, TaskPhase::Pending(_))
    }

    pub(crate) fn try_admit(&self) -> bool {
        let mut state = self.state.borrow_mut();
        if !matches!(state.phase, TaskPhase::Pending(_)) || state.admitted {
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
        debug_assert!(matches!(state.phase, TaskPhase::Running));
        state.phase = TaskPhase::Settled(outcome.clone());
        state.wait_registration = None;
        release_admission(&mut state);
        let waiters = std::mem::take(&mut state.waiters);
        drop(state);
        for waiter in waiters {
            waiter.resume(outcome.clone());
        }
    }

    pub(crate) fn suspend(
        &self,
        execution: crate::vm::TaskExecution,
        wait_registration: Option<WaitSet>,
    ) {
        let mut state = self.state.borrow_mut();
        debug_assert!(matches!(state.phase, TaskPhase::Running));
        state.phase = TaskPhase::Pending(Box::new(execution));
        state.wait_registration = wait_registration;
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        let mut state = self.state.borrow_mut();
        let wait_registration = state.wait_registration.take();
        let TaskPhase::Pending(execution) = &mut state.phase else {
            return;
        };
        execution.resume(result);
        let ready = state.ready.clone();
        drop(state);
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_losers(self);
        }
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    pub(crate) fn reject_closed_send(&self) {
        let mut state = self.state.borrow_mut();
        let wait_registration = state.wait_registration.take();
        let TaskPhase::Pending(execution) = &mut state.phase else {
            return;
        };
        execution.reject_closed_send();
        let ready = state.ready.clone();
        drop(state);
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_losers(self);
        }
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    pub(crate) fn wait_for(&self, waiter: Waiter) {
        self.state.borrow_mut().waiters.push(waiter);
    }

    pub(crate) fn observe(&self) {
        self.state.borrow_mut().observed = true;
    }

    pub(crate) fn observer(&self) -> TaskObserver {
        TaskObserver(Rc::downgrade(&self.state))
    }

    pub(crate) fn outcome(&self) -> Option<Result<Value, crate::RuntimeError>> {
        match &self.state.borrow().phase {
            TaskPhase::Settled(outcome) => Some(outcome.clone()),
            TaskPhase::Pending(_) | TaskPhase::Running => None,
        }
    }

    pub(crate) fn cancel(&self, error: &crate::RuntimeError) {
        let mut state = self.state.borrow_mut();
        let phase = std::mem::replace(&mut state.phase, TaskPhase::Running);
        if !matches!(phase, TaskPhase::Pending(_)) {
            state.phase = phase;
            return;
        }
        let wait_registration = state.wait_registration.take();
        let waiters = std::mem::take(&mut state.waiters);
        let ready = state.ready.clone();
        state.phase = TaskPhase::Settled(Err(error.clone()));
        release_admission(&mut state);
        drop(state);
        ready
            .borrow_mut()
            .retain(|candidate| !Rc::ptr_eq(&candidate.state, &self.state));
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_for_waiter(&Waiter::Task(Rc::new(self.clone())));
        }
        for waiter in waiters {
            waiter.resume(Err(error.clone()));
        }
    }

    pub(crate) fn unobserved_error(&self) -> Option<crate::RuntimeError> {
        let state = self.state.borrow();
        if state.observed {
            None
        } else {
            match &state.phase {
                TaskPhase::Settled(Err(error)) => Some(error.clone()),
                TaskPhase::Pending(_) | TaskPhase::Running | TaskPhase::Settled(Ok(_)) => None,
            }
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
    /// A host callable paired with the source declaration's private identity.
    DeclaredNative {
        function: NativeFunction,
        callable_identity: CallableIdentity,
    },
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
            Self::Closure(_)
            | Self::Native(_)
            | Self::DeclaredNative { .. }
            | Self::Builtin(_)
            | Self::Overloads(_) => "fn",
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
            (
                Self::DeclaredNative { function: left, .. },
                Self::DeclaredNative {
                    function: right, ..
                },
            ) => left.same_function(right),
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
            Self::DeclaredNative { function, .. } => {
                write!(f, "<native {}>", function.qualified_name())
            }
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
