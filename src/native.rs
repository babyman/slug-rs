use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    fmt,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    sync::{
        Arc, Mutex, Once, Weak as SyncWeak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use crate::{Value, scheduler_signal::SchedulerSignal, value::Channel};

/// An owned value that a foreign thread may publish through a channel producer.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeSendValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl NativeSendValue {
    #[must_use]
    pub fn nil() -> Self {
        Self::Nil
    }
    #[must_use]
    pub fn boolean(value: bool) -> Self {
        Self::Bool(value)
    }
    #[must_use]
    pub fn integer(value: i64) -> Self {
        Self::Int(value)
    }
    #[must_use]
    pub fn float(value: f64) -> Self {
        Self::Float(value)
    }
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }
    #[must_use]
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes(value.into())
    }

    pub(crate) fn into_value(self) -> Value {
        match self {
            Self::Nil => Value::Nil,
            Self::Bool(value) => Value::Bool(value),
            Self::Int(value) => Value::Int(value),
            Self::Float(value) => Value::Float(value),
            Self::String(value) => Value::string(value),
            Self::Bytes(value) => Value::Bytes(value.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NativeProducerStatus {
    Sent,
    /// The bounded mailbox was full. The caller retains the value and may
    /// retry, coalesce, or discard it according to its own policy.
    Full(NativeSendValue),
    /// The receiver or runtime was closed. The caller retains the value.
    Closed(NativeSendValue),
}

pub struct NativeChannelProducer {
    state: Arc<NativeProducerState>,
    producer_lease: bool,
}

struct NativeProducerState {
    capacity: usize,
    occupied: AtomicUsize,
    queue: Mutex<VecDeque<NativeSendValue>>,
    closed: AtomicBool,
    producer_leases: AtomicUsize,
    scheduler_signals: Mutex<Vec<SyncWeak<SchedulerSignal>>>,
}

impl NativeChannelProducer {
    #[must_use]
    pub(crate) fn bounded(capacity: usize) -> Self {
        Self {
            state: Arc::new(NativeProducerState {
                capacity,
                occupied: AtomicUsize::new(0),
                queue: Mutex::new(VecDeque::new()),
                closed: AtomicBool::new(false),
                producer_leases: AtomicUsize::new(1),
                scheduler_signals: Mutex::new(Vec::new()),
            }),
            producer_lease: true,
        }
    }
    #[must_use]
    pub fn try_send(&self, value: NativeSendValue) -> NativeProducerStatus {
        if !self.reserve_slot() {
            return if self.is_closed() {
                NativeProducerStatus::Closed(value)
            } else {
                NativeProducerStatus::Full(value)
            };
        }
        if self.is_closed() {
            self.release_slot();
            return NativeProducerStatus::Closed(value);
        }
        let Ok(mut queue) = self.state.queue.lock() else {
            self.release_slot();
            return NativeProducerStatus::Closed(value);
        };
        if self.is_closed() {
            self.release_slot();
            return NativeProducerStatus::Closed(value);
        }
        queue.push_back(value);
        drop(queue);
        self.notify_schedulers();
        NativeProducerStatus::Sent
    }
    pub fn close(&self) {
        self.state.closed.store(true, Ordering::Release);
        self.notify_schedulers();
    }
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }
    pub(crate) fn drain(&self, limit: usize) -> Vec<NativeSendValue> {
        self.state.queue.lock().map_or_else(
            |_| Vec::new(),
            |mut queue| {
                let end = limit.min(queue.len());
                queue.drain(..end).collect()
            },
        )
    }

    pub(crate) fn reserve_slot(&self) -> bool {
        let mut occupied = self.state.occupied.load(Ordering::Acquire);
        loop {
            if occupied >= self.state.capacity {
                return false;
            }
            match self.state.occupied.compare_exchange_weak(
                occupied,
                occupied + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => occupied = actual,
            }
        }
    }

    pub(crate) fn release_slot(&self) {
        let previous = self.state.occupied.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "native channel occupancy underflow");
    }

    pub(crate) fn register_scheduler(&self, signal: &Arc<SchedulerSignal>) {
        let mut signals = self
            .state
            .scheduler_signals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        signals.retain(|candidate| candidate.strong_count() > 0);
        if !signals
            .iter()
            .any(|candidate| candidate.ptr_eq(&Arc::downgrade(signal)))
        {
            signals.push(Arc::downgrade(signal));
        }
    }

    pub(crate) fn has_external_producer(&self) -> bool {
        self.state.producer_leases.load(Ordering::Acquire) > 0 && !self.is_closed()
    }

    pub(crate) fn receiver_handle(&self) -> Self {
        Self {
            state: self.state.clone(),
            producer_lease: false,
        }
    }

    fn notify_schedulers(&self) {
        let signals = {
            let mut signals = self
                .state
                .scheduler_signals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            signals.retain(|signal| signal.strong_count() > 0);
            signals
                .iter()
                .filter_map(SyncWeak::upgrade)
                .collect::<Vec<_>>()
        };
        for signal in signals {
            signal.notify();
        }
    }
}

impl Clone for NativeChannelProducer {
    fn clone(&self) -> Self {
        if self.producer_lease {
            self.state.producer_leases.fetch_add(1, Ordering::AcqRel);
        }
        Self {
            state: self.state.clone(),
            producer_lease: self.producer_lease,
        }
    }
}

impl Drop for NativeChannelProducer {
    fn drop(&mut self) {
        if self.producer_lease {
            let previous = self.state.producer_leases.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "native producer lease underflow");
        }
        self.notify_schedulers();
    }
}

#[derive(Clone)]
pub(crate) struct NativeResourceRegistry(Rc<NativeResourceRegistryInner>);

struct NativeResourceRegistryInner {
    resources: RefCell<Vec<Weak<NativeResource>>>,
}

pub(crate) fn native_resource_registry() -> NativeResourceRegistry {
    NativeResourceRegistry(Rc::new(NativeResourceRegistryInner {
        resources: RefCell::new(Vec::new()),
    }))
}

impl NativeResourceRegistry {
    pub(crate) fn register(&self, resources: Vec<Weak<NativeResource>>) {
        let mut tracked = self.0.resources.borrow_mut();
        tracked.retain(|resource| resource.strong_count() > 0);
        tracked.extend(
            resources
                .into_iter()
                .filter(|resource| resource.strong_count() > 0),
        );
    }

    #[cfg(test)]
    fn tracked_count(&self) -> usize {
        self.0.resources.borrow().len()
    }
}

impl fmt::Debug for NativeResourceRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeResourceRegistry")
            .field("tracked", &self.0.resources.borrow().len())
            .finish()
    }
}

impl Drop for NativeResourceRegistryInner {
    fn drop(&mut self) {
        for resource in self.resources.get_mut().iter() {
            if let Some(resource) = resource.upgrade() {
                let _ = resource.close();
            }
        }
    }
}

static NEXT_MODULE_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_RESOURCE_TYPE_ID: AtomicUsize = AtomicUsize::new(1);
static NATIVE_PANIC_HOOK: Once = Once::new();

thread_local! {
    static NATIVE_BOUNDARY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct NativeBoundaryGuard {
    previous_depth: usize,
}

impl NativeBoundaryGuard {
    fn enter() -> Self {
        install_native_panic_hook();
        let previous_depth = NATIVE_BOUNDARY_DEPTH.with(|depth| {
            let previous = depth.get();
            depth.set(previous.saturating_add(1));
            previous
        });
        Self { previous_depth }
    }
}

impl Drop for NativeBoundaryGuard {
    fn drop(&mut self) {
        NATIVE_BOUNDARY_DEPTH.with(|depth| depth.set(self.previous_depth));
    }
}

fn install_native_panic_hook() {
    NATIVE_PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let inside_native = NATIVE_BOUNDARY_DEPTH.with(|depth| depth.get() > 0);
            if !inside_native {
                previous(info);
            }
        }));
    });
}

fn catch_native_unwind<R>(operation: impl FnOnce() -> R) -> std::thread::Result<R> {
    let _guard = NativeBoundaryGuard::enter();
    catch_unwind(AssertUnwindSafe(operation))
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NativeArity {
    Exact(usize),
    Range { minimum: usize, maximum: usize },
    Variadic { minimum: usize },
}

impl NativeArity {
    fn accepts(self, count: usize) -> bool {
        match self {
            Self::Exact(expected) => count == expected,
            Self::Range { minimum, maximum } => (minimum..=maximum).contains(&count),
            Self::Variadic { minimum } => count >= minimum,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Exact(expected) => expected.to_string(),
            Self::Range { minimum, maximum } => format!("between {minimum} and {maximum}"),
            Self::Variadic { minimum } => format!("at least {minimum}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeStatus {
    Ok,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeValueKind {
    Nil,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    List,
    Map,
    StructSchema,
    Struct,
    Channel,
    Function,
    Resource,
    Task,
}

#[derive(Clone, Debug)]
pub struct NativeError {
    code: String,
    message: String,
    data: Option<NativeOwnedValue>,
}

impl NativeError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: NativeOwnedValue) -> Self {
        self.data = Some(data);
        self
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct NativeOwnedValue(Value);

impl NativeOwnedValue {
    #[must_use]
    pub fn nil() -> Self {
        Self(Value::Nil)
    }

    #[must_use]
    pub fn boolean(value: bool) -> Self {
        Self(Value::Bool(value))
    }

    #[must_use]
    pub fn integer(value: i64) -> Self {
        Self(Value::Int(value))
    }

    #[must_use]
    pub fn float(value: f64) -> Self {
        Self(Value::Float(value))
    }

    #[must_use]
    pub fn string(value: impl Into<Rc<str>>) -> Self {
        Self(Value::Str(value.into()))
    }

    #[must_use]
    pub fn bytes(value: impl Into<Rc<[u8]>>) -> Self {
        Self(Value::Bytes(value.into()))
    }

    #[must_use]
    pub fn list(values: Vec<Self>) -> Self {
        Self(Value::List(Rc::new(
            values.into_iter().map(|value| value.0).collect(),
        )))
    }

    #[must_use]
    pub fn map(entries: Vec<(Self, Self)>) -> Self {
        Self(Value::Map(Rc::new(
            entries
                .into_iter()
                .map(|(key, value)| (key.0, value.0))
                .collect(),
        )))
    }

    pub(crate) fn into_value(self) -> Value {
        self.0
    }
}

#[derive(Clone, Copy)]
pub struct NativeValueRef<'call> {
    value: &'call Value,
}

impl<'call> NativeValueRef<'call> {
    #[must_use]
    pub fn kind(self) -> NativeValueKind {
        match self.value {
            Value::Nil => NativeValueKind::Nil,
            Value::Bool(_) => NativeValueKind::Bool,
            Value::Int(_) => NativeValueKind::Int,
            Value::Float(_) => NativeValueKind::Float,
            Value::Str(_) => NativeValueKind::String,
            Value::Bytes(_) => NativeValueKind::Bytes,
            Value::List(_) => NativeValueKind::List,
            Value::Map(_) => NativeValueKind::Map,
            Value::StructSchema(_) => NativeValueKind::StructSchema,
            Value::Struct(_) => NativeValueKind::Struct,
            Value::Channel(_) => NativeValueKind::Channel,
            Value::Closure(_)
            | Value::Native(_)
            | Value::DeclaredNative { .. }
            | Value::Builtin(_)
            | Value::Overloads(_) => NativeValueKind::Function,
            Value::NativeResource(_) => NativeValueKind::Resource,
            Value::Task(_) => NativeValueKind::Task,
            Value::Uninitialized | Value::Binding { .. } => {
                unreachable!("native arguments are resolved before invocation")
            }
        }
    }

    /// Reads a boolean without exposing its VM representation.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not a boolean.
    pub fn as_bool(self) -> Result<bool, NativeError> {
        let Value::Bool(value) = self.value else {
            return Err(self.type_error("bool"));
        };
        Ok(*value)
    }

    /// Reads an integer without numeric narrowing.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not an integer.
    pub fn as_i64(self) -> Result<i64, NativeError> {
        let Value::Int(value) = self.value else {
            return Err(self.type_error("int"));
        };
        Ok(*value)
    }

    /// Reads a floating-point value without converting an integer.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not a float.
    pub fn as_f64(self) -> Result<f64, NativeError> {
        let Value::Float(value) = self.value else {
            return Err(self.type_error("float"));
        };
        Ok(*value)
    }

    /// Borrows UTF-8 text for the current native call.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not a string.
    pub fn as_str(self) -> Result<&'call str, NativeError> {
        let Value::Str(value) = self.value else {
            return Err(self.type_error("str"));
        };
        Ok(value)
    }

    /// Borrows bytes for the current native call.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not bytes.
    pub fn as_bytes(self) -> Result<&'call [u8], NativeError> {
        let Value::Bytes(value) = self.value else {
            return Err(self.type_error("bytes"));
        };
        Ok(value)
    }

    #[must_use]
    pub fn len(self) -> Option<usize> {
        match self.value {
            Value::List(values) => Some(values.len()),
            Value::Map(entries) => Some(entries.len()),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_empty(self) -> Option<bool> {
        self.len().map(|length| length == 0)
    }

    /// Reads one list slot, borrowing it for the current call.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not a list.
    pub fn list_get(self, index: usize) -> Result<Option<NativeValueRef<'call>>, NativeError> {
        let Value::List(values) = self.value else {
            return Err(self.type_error("list"));
        };
        Ok(values.get(index).map(|value| NativeValueRef { value }))
    }

    /// Reads one map entry, borrowing it for the current call.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the value is not a map.
    pub fn map_get(
        self,
        index: usize,
    ) -> Result<Option<(NativeValueRef<'call>, NativeValueRef<'call>)>, NativeError> {
        let Value::Map(entries) = self.value else {
            return Err(self.type_error("map"));
        };
        Ok(entries
            .get(index)
            .map(|(key, value)| (NativeValueRef { value: key }, NativeValueRef { value })))
    }

    #[must_use]
    pub fn to_display_string(self) -> String {
        self.value.to_string()
    }

    #[must_use]
    pub fn to_owned(self) -> NativeOwnedValue {
        NativeOwnedValue(self.value.clone())
    }

    fn type_error(self, expected: &str) -> NativeError {
        NativeError::new(
            "native.type",
            format!("expected {expected}, got {:?}", self.kind()),
        )
    }
}

impl fmt::Debug for NativeValueRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NativeValueRef").field(&self.kind()).finish()
    }
}

#[derive(Clone)]
pub struct NativeModule {
    inner: Rc<NativeModuleInner>,
}

struct NativeModuleInner {
    id: usize,
    name: Rc<str>,
    state: Box<dyn Any>,
    function_signatures: RefCell<HashSet<(String, NativeArity)>>,
    resource_types: RefCell<HashSet<String>>,
}

impl NativeModule {
    /// Creates one identity-bearing native module instance with opaque state.
    ///
    /// # Errors
    ///
    /// Returns an error when the module name is empty.
    pub fn new<T: Any>(name: impl Into<Rc<str>>, state: T) -> Result<Self, NativeDescriptorError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(NativeDescriptorError::new(
                "native module name cannot be empty",
            ));
        }
        Ok(Self {
            inner: Rc::new(NativeModuleInner {
                id: NEXT_MODULE_ID.fetch_add(1, Ordering::Relaxed),
                name,
                state: Box::new(state),
                function_signatures: RefCell::new(HashSet::new()),
                resource_types: RefCell::new(HashSet::new()),
            }),
        })
    }

    /// Describes a synchronous function owned by this module.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding name is empty.
    pub fn function(
        &self,
        name: impl Into<Rc<str>>,
        arity: NativeArity,
        callback: for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
    ) -> Result<NativeFunction, NativeDescriptorError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(NativeDescriptorError::new(
                "native function name cannot be empty",
            ));
        }
        if !self
            .inner
            .function_signatures
            .borrow_mut()
            .insert((name.to_string(), arity))
        {
            return Err(NativeDescriptorError::new(format!(
                "native function `{}.{name}` with arity {arity:?} is already registered",
                self.inner.name
            )));
        }
        Ok(NativeFunction::new(self.clone(), name, arity, callback))
    }

    /// Registers an identity-bearing resource type and its lifecycle callbacks.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty or duplicate type name.
    pub fn resource_type<T: Any>(
        &self,
        name: impl Into<Rc<str>>,
        close: fn(&mut T),
        destroy: fn(T),
    ) -> Result<NativeResourceType<T>, NativeDescriptorError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(NativeDescriptorError::new(
                "native resource type name cannot be empty",
            ));
        }
        if !self
            .inner
            .resource_types
            .borrow_mut()
            .insert(name.to_string())
        {
            return Err(NativeDescriptorError::new(format!(
                "native resource type `{}` is already registered in module `{}`",
                name, self.inner.name
            )));
        }
        Ok(NativeResourceType {
            registration: Rc::new(ResourceTypeRegistration {
                id: NEXT_RESOURCE_TYPE_ID.fetch_add(1, Ordering::Relaxed),
                module_id: self.inner.id,
                name,
                close: Box::new(move |payload| {
                    if let Some(payload) = payload.downcast_mut::<T>() {
                        close(payload);
                    }
                }),
                destroy: Box::new(move |payload| {
                    if let Ok(payload) = payload.downcast::<T>() {
                        destroy(*payload);
                    }
                }),
            }),
            marker: PhantomData,
        })
    }
}

#[derive(Clone)]
pub struct NativeFunction(Rc<NativeFunctionInner>);

struct NativeFunctionInner {
    module: NativeModule,
    name: Rc<str>,
    arity: NativeArity,
    callback: for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
}

impl NativeFunction {
    fn new(
        module: NativeModule,
        name: Rc<str>,
        arity: NativeArity,
        callback: for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
    ) -> Self {
        Self(Rc::new(NativeFunctionInner {
            module,
            name,
            arity,
            callback,
        }))
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.0.name
    }

    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.0.module.inner.name
    }

    #[must_use]
    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.module_name(), self.name())
    }

    pub(crate) fn same_function(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn matches_declared_arity(&self, minimum: usize, maximum: Option<usize>) -> bool {
        match (self.0.arity, maximum) {
            (NativeArity::Exact(expected), Some(maximum)) => {
                expected == minimum && expected == maximum
            }
            (
                NativeArity::Range {
                    minimum: expected_minimum,
                    maximum: expected_maximum,
                },
                Some(maximum),
            ) => expected_minimum == minimum && expected_maximum == maximum,
            (NativeArity::Variadic { minimum: expected }, None) => expected == minimum,
            _ => false,
        }
    }

    pub(crate) fn invoke(&self, arguments: &[Value]) -> NativeInvocation {
        if !self.0.arity.accepts(arguments.len()) {
            return NativeInvocation::Error(
                NativeError::new(
                    "native.arity",
                    format!(
                        "`{}` expects {} arguments, got {}",
                        self.qualified_name(),
                        self.0.arity.describe(),
                        arguments.len()
                    ),
                ),
                Vec::new(),
            );
        }
        let mut call = NativeCall {
            arguments,
            module: &self.0.module,
            outcome: None,
            violation: None,
            resources: Vec::new(),
        };
        let status = catch_native_unwind(|| (self.0.callback)(&mut call));
        let Ok(status) = status else {
            return call.contract_violation(format!("native `{}` panicked", self.qualified_name()));
        };
        if let Some(message) = call.violation.take() {
            return call.contract_violation(message);
        }
        match (status, call.outcome.take()) {
            (NativeStatus::Ok, Some(NativeOutcome::Result(value))) => {
                NativeInvocation::Result(value.into_value(), call.resources)
            }
            (NativeStatus::Error, Some(NativeOutcome::Error(error))) => {
                NativeInvocation::Error(error, call.resources)
            }
            (NativeStatus::Ok, None) => call.contract_violation(format!(
                "native `{}` returned ok without a result",
                self.qualified_name()
            )),
            (NativeStatus::Error, None) => call.contract_violation(format!(
                "native `{}` returned error without an error value",
                self.qualified_name()
            )),
            (NativeStatus::Ok, Some(NativeOutcome::Error(_))) => {
                call.contract_violation("native callback returned ok after setting an error".into())
            }
            (NativeStatus::Error, Some(NativeOutcome::Result(_))) => call
                .contract_violation("native callback returned error after setting a result".into()),
        }
    }
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native {}>", self.qualified_name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeDescriptorError {
    message: String,
}

impl NativeDescriptorError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NativeDescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NativeDescriptorError {}

pub struct NativeResourceType<T> {
    registration: Rc<ResourceTypeRegistration>,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for NativeResourceType<T> {
    fn clone(&self) -> Self {
        Self {
            registration: self.registration.clone(),
            marker: PhantomData,
        }
    }
}

type ResourceClose = dyn Fn(&mut dyn Any);
type ResourceDestroy = dyn Fn(Box<dyn Any>);

struct ResourceTypeRegistration {
    id: usize,
    module_id: usize,
    name: Rc<str>,
    close: Box<ResourceClose>,
    destroy: Box<ResourceDestroy>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeResourceState {
    Open,
    Closing,
    Closed,
}

#[doc(hidden)]
pub struct NativeResource {
    registration: Rc<ResourceTypeRegistration>,
    module: Rc<NativeModuleInner>,
    payload: RefCell<Option<Box<dyn Any>>>,
    state: Cell<NativeResourceState>,
}

impl fmt::Debug for NativeResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeResource")
            .field("module", &self.module.name)
            .field("resource_type", &self.registration.name)
            .field("state", &self.state.get())
            .finish_non_exhaustive()
    }
}

impl NativeResource {
    pub(crate) fn close(&self) -> Result<(), String> {
        match self.state.get() {
            NativeResourceState::Closed => return Ok(()),
            NativeResourceState::Closing => {
                return Err(format!(
                    "native resource `{}` is already being closed",
                    self.registration.name
                ));
            }
            NativeResourceState::Open => self.state.set(NativeResourceState::Closing),
        }

        let Ok(mut payload) = self.payload.try_borrow_mut() else {
            self.state.set(NativeResourceState::Open);
            return Err(format!(
                "native resource `{}` is already in use",
                self.registration.name
            ));
        };
        let Some(payload) = payload.as_deref_mut() else {
            self.state.set(NativeResourceState::Closed);
            return Ok(());
        };

        if catch_native_unwind(|| (self.registration.close)(payload)).is_ok() {
            self.state.set(NativeResourceState::Closed);
            Ok(())
        } else {
            self.state.set(NativeResourceState::Open);
            Err(format!(
                "native resource `{}` close callback panicked",
                self.registration.name
            ))
        }
    }
}

impl Drop for NativeResource {
    fn drop(&mut self) {
        if let Some(payload) = self.payload.get_mut().take() {
            let _ = catch_native_unwind(|| (self.registration.destroy)(payload));
        }
    }
}

enum NativeOutcome {
    Result(NativeOwnedValue),
    Error(NativeError),
}

pub struct NativeCall<'call> {
    arguments: &'call [Value],
    module: &'call NativeModule,
    outcome: Option<NativeOutcome>,
    violation: Option<String>,
    resources: Vec<Weak<NativeResource>>,
}

impl NativeCall<'_> {
    #[must_use]
    pub fn argument_count(&self) -> usize {
        self.arguments.len()
    }

    /// Borrows one argument for the dynamic extent of this callback.
    ///
    /// # Errors
    ///
    /// Returns `native.arity` when the slot does not exist.
    pub fn argument(&self, index: usize) -> Result<NativeValueRef<'_>, NativeError> {
        self.arguments
            .get(index)
            .map(|value| NativeValueRef { value })
            .ok_or_else(|| {
                NativeError::new("native.arity", format!("argument {index} does not exist"))
            })
    }

    #[must_use]
    pub fn state<T: Any>(&self) -> Option<&T> {
        self.module.inner.state.downcast_ref()
    }

    pub fn set_result(&mut self, value: NativeOwnedValue) {
        self.set_outcome(NativeOutcome::Result(value));
    }

    pub fn return_value(&mut self, value: NativeOwnedValue) -> NativeStatus {
        self.set_result(value);
        NativeStatus::Ok
    }

    pub fn set_error(&mut self, error: NativeError) {
        if error.code.trim().is_empty() {
            self.violation = Some("native error code cannot be empty".into());
            return;
        }
        self.set_outcome(NativeOutcome::Error(error));
    }

    pub fn raise(&mut self, error: NativeError) -> NativeStatus {
        self.set_error(error);
        NativeStatus::Error
    }

    /// Creates a channel receiver paired with a thread-safe native producer.
    ///
    /// The producer accepts only [`NativeSendValue`] values and must not be
    /// used to access call-scoped values after this callback returns.
    pub fn channel(&mut self, capacity: usize) -> (NativeOwnedValue, NativeChannelProducer) {
        let (channel, producer) = Channel::native(capacity);
        (NativeOwnedValue(Value::Channel(Rc::new(channel))), producer)
    }

    /// Creates an ordinary Slug channel without issuing a native producer capability.
    #[must_use]
    pub fn plain_channel(&mut self, capacity: usize) -> NativeOwnedValue {
        NativeOwnedValue(Value::Channel(Rc::new(Channel::new(capacity))))
    }

    /// Closes a channel argument and wakes blocked Slug senders and receivers.
    ///
    /// # Errors
    ///
    /// Returns `native.type` when the selected argument is not a channel.
    pub fn close_channel(&mut self, index: usize) -> Result<(), NativeError> {
        let value = self.argument(index)?;
        let Value::Channel(channel) = value.value else {
            return Err(value.type_error("chan"));
        };
        channel.close();
        Ok(())
    }

    /// Constructs a typed resource owned by the callback's module.
    ///
    /// # Errors
    ///
    /// Returns `native.resource_type` when the registration belongs to another module.
    pub fn resource<T: Any>(
        &mut self,
        resource_type: &NativeResourceType<T>,
        payload: T,
    ) -> Result<NativeOwnedValue, NativeError> {
        self.validate_resource_type(resource_type)?;
        let resource = Rc::new(NativeResource {
            registration: resource_type.registration.clone(),
            module: self.module.inner.clone(),
            payload: RefCell::new(Some(Box::new(payload))),
            state: Cell::new(NativeResourceState::Open),
        });
        self.resources.push(Rc::downgrade(&resource));
        Ok(NativeOwnedValue(Value::NativeResource(resource)))
    }

    /// Runs a synchronous operation against a checked resource payload.
    ///
    /// # Errors
    ///
    /// Returns a structured resource error for a wrong module, wrong type,
    /// closed handle, missing argument, or overlapping mutable access.
    pub fn with_resource<T: Any, R>(
        &self,
        index: usize,
        resource_type: &NativeResourceType<T>,
        operation: impl FnOnce(&mut T) -> R,
    ) -> Result<R, NativeError> {
        self.validate_resource_type(resource_type)?;
        let value = self.argument(index)?;
        let Value::NativeResource(resource) = value.value else {
            return Err(NativeError::new(
                "native.resource_type",
                "expected native resource",
            ));
        };
        if resource.registration.module_id != self.module.inner.id
            || resource.registration.id != resource_type.registration.id
        {
            return Err(NativeError::new(
                "native.resource_type",
                format!(
                    "expected resource type `{}`",
                    resource_type.registration.name
                ),
            ));
        }
        if resource.state.get() != NativeResourceState::Open {
            return Err(NativeError::new(
                "native.resource_closed",
                "native resource is closed",
            ));
        }
        let mut payload = resource.payload.try_borrow_mut().map_err(|_| {
            NativeError::new("native.resource_busy", "native resource is already in use")
        })?;
        let payload = payload
            .as_deref_mut()
            .and_then(|payload| payload.downcast_mut::<T>())
            .ok_or_else(|| {
                NativeError::new("native.resource_type", "invalid native resource payload")
            })?;
        Ok(operation(payload))
    }

    /// Idempotently closes a checked resource handle.
    ///
    /// # Errors
    ///
    /// Returns a structured resource error for a wrong module, wrong type,
    /// missing argument, or failed close callback.
    pub fn close_resource<T: Any>(
        &self,
        index: usize,
        resource_type: &NativeResourceType<T>,
    ) -> Result<(), NativeError> {
        self.validate_resource_type(resource_type)?;
        let value = self.argument(index)?;
        let Value::NativeResource(resource) = value.value else {
            return Err(NativeError::new(
                "native.resource_type",
                "expected native resource",
            ));
        };
        if resource.registration.module_id != self.module.inner.id
            || resource.registration.id != resource_type.registration.id
        {
            return Err(NativeError::new(
                "native.resource_type",
                format!(
                    "expected resource type `{}`",
                    resource_type.registration.name
                ),
            ));
        }
        resource
            .close()
            .map_err(|message| NativeError::new("native.resource_close", message))
    }

    fn validate_resource_type<T>(
        &self,
        resource_type: &NativeResourceType<T>,
    ) -> Result<(), NativeError> {
        if resource_type.registration.module_id != self.module.inner.id {
            return Err(NativeError::new(
                "native.resource_type",
                "resource type belongs to another native module",
            ));
        }
        Ok(())
    }

    fn set_outcome(&mut self, outcome: NativeOutcome) {
        if self.outcome.is_some() {
            self.violation = Some("native callback set more than one outcome".into());
        } else {
            self.outcome = Some(outcome);
        }
    }

    fn contract_violation(&mut self, message: String) -> NativeInvocation {
        for resource in &self.resources {
            if let Some(resource) = resource.upgrade() {
                let _ = resource.close();
            }
        }
        NativeInvocation::ContractViolation(message)
    }
}

pub(crate) enum NativeInvocation {
    Result(Value, Vec<Weak<NativeResource>>),
    Error(NativeError, Vec<Weak<NativeResource>>),
    ContractViolation(String),
}

impl NativeError {
    pub(crate) fn into_parts(self) -> (String, String, Option<Value>) {
        (
            self.code,
            self.message,
            self.data.map(NativeOwnedValue::into_value),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_resource_registry_compacts_dead_weak_entries() {
        let registry = native_resource_registry();
        let module = NativeModule::new("test.registry", ()).unwrap();
        let resource_type = module
            .resource_type("value", |_payload: &mut ()| {}, |_payload: ()| {})
            .unwrap();
        let resource = Rc::new(NativeResource {
            registration: resource_type.registration,
            module: module.inner,
            payload: RefCell::new(Some(Box::new(()))),
            state: Cell::new(NativeResourceState::Open),
        });
        registry.register(vec![Rc::downgrade(&resource)]);
        assert_eq!(registry.tracked_count(), 1);

        drop(resource);
        registry.register(Vec::new());
        assert_eq!(registry.tracked_count(), 0);
    }

    #[test]
    fn producer_is_thread_safe_bounded_and_closed() {
        let producer = NativeChannelProducer::bounded(1);
        let sender = producer.clone();
        let status = std::thread::spawn(move || sender.try_send(NativeSendValue::integer(7)))
            .join()
            .expect("producer thread completes");
        assert_eq!(status, NativeProducerStatus::Sent);
        assert_eq!(
            producer.try_send(NativeSendValue::integer(8)),
            NativeProducerStatus::Full(NativeSendValue::integer(8))
        );
        producer.close();
        assert!(producer.is_closed());
        assert_eq!(
            producer.try_send(NativeSendValue::integer(9)),
            NativeProducerStatus::Closed(NativeSendValue::integer(9))
        );
    }

    #[test]
    fn receiver_handles_do_not_count_as_external_producer_leases() {
        let producer = NativeChannelProducer::bounded(1);
        let receiver = producer.receiver_handle();
        let second = producer.clone();
        assert!(receiver.has_external_producer());

        drop(producer);
        assert!(receiver.has_external_producer());
        drop(second);
        assert!(!receiver.has_external_producer());
    }

    #[test]
    fn dropping_a_paired_receiver_revokes_its_producer() {
        let (channel, producer) = Channel::native(1);
        drop(channel);
        assert!(producer.is_closed());
        assert_eq!(
            producer.try_send(NativeSendValue::integer(7)),
            NativeProducerStatus::Closed(NativeSendValue::integer(7))
        );
    }

    #[test]
    fn rejected_producer_values_remain_owned_by_the_sender() {
        let producer = NativeChannelProducer::bounded(1);
        assert_eq!(
            producer.try_send(NativeSendValue::string("first")),
            NativeProducerStatus::Sent
        );
        assert_eq!(
            producer.try_send(NativeSendValue::bytes(vec![1, 2, 3])),
            NativeProducerStatus::Full(NativeSendValue::bytes(vec![1, 2, 3]))
        );
        producer.close();
        assert_eq!(
            producer.try_send(NativeSendValue::string("closed")),
            NativeProducerStatus::Closed(NativeSendValue::string("closed"))
        );
    }

    #[test]
    fn concurrent_senders_observe_revocation_without_losing_rejected_values() {
        use std::sync::{Arc, Barrier};

        let producer = NativeChannelProducer::bounded(1);
        let start = Arc::new(Barrier::new(9));
        let senders = (0_i64..8)
            .map(|index| {
                let producer = producer.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    let mut value = NativeSendValue::integer(index);
                    loop {
                        match producer.try_send(value) {
                            NativeProducerStatus::Sent => {
                                value = NativeSendValue::integer(index);
                            }
                            NativeProducerStatus::Full(rejected) => value = rejected,
                            NativeProducerStatus::Closed(rejected) => return rejected,
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        std::thread::sleep(std::time::Duration::from_millis(5));
        producer.close();

        for (index, sender) in (0_i64..).zip(senders) {
            assert_eq!(
                sender.join().expect("sender thread completes"),
                NativeSendValue::integer(index)
            );
        }
    }
}
