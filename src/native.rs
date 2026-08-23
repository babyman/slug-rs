use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashSet,
    fmt,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::Value;

pub(crate) type NativeResourceRegistry = Rc<RefCell<Vec<Weak<NativeResource>>>>;

pub(crate) fn native_resource_registry() -> NativeResourceRegistry {
    Rc::new(RefCell::new(Vec::new()))
}

static NEXT_MODULE_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_RESOURCE_TYPE_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeArity {
    Exact(usize),
    Variadic { minimum: usize },
}

impl NativeArity {
    fn accepts(self, count: usize) -> bool {
        match self {
            Self::Exact(expected) => count == expected,
            Self::Variadic { minimum } => count >= minimum,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Exact(expected) => expected.to_string(),
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
    Function,
    Resource,
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
            Value::Closure(_) | Value::Native(_) | Value::Builtin(_) | Value::Overloads(_) => {
                NativeValueKind::Function
            }
            Value::NativeResource(_) => NativeValueKind::Resource,
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
        NativeFunction::new(self.clone(), name, arity, callback)
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
        name: impl Into<Rc<str>>,
        arity: NativeArity,
        callback: for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
    ) -> Result<Self, NativeDescriptorError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(NativeDescriptorError::new(
                "native function name cannot be empty",
            ));
        }
        Ok(Self(Rc::new(NativeFunctionInner {
            module,
            name,
            arity,
            callback,
        })))
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub(crate) fn same_function(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn invoke(&self, arguments: &[Value]) -> NativeInvocation {
        if !self.0.arity.accepts(arguments.len()) {
            return NativeInvocation::Error(NativeError::new(
                "native.arity",
                format!(
                    "`{}.{}` expects {} arguments, got {}",
                    self.0.module.inner.name,
                    self.0.name,
                    self.0.arity.describe(),
                    arguments.len()
                ),
            ));
        }
        let mut call = NativeCall {
            arguments,
            module: &self.0.module,
            outcome: None,
            violation: None,
            resources: Vec::new(),
        };
        let status = catch_unwind(AssertUnwindSafe(|| (self.0.callback)(&mut call)));
        let Ok(status) = status else {
            return NativeInvocation::ContractViolation(format!(
                "native `{}.{}` panicked",
                self.0.module.inner.name, self.0.name
            ));
        };
        if let Some(message) = call.violation {
            return NativeInvocation::ContractViolation(message);
        }
        match (status, call.outcome) {
            (NativeStatus::Ok, Some(NativeOutcome::Result(value))) => {
                NativeInvocation::Result(value.into_value(), call.resources)
            }
            (NativeStatus::Error, Some(NativeOutcome::Error(error))) => {
                NativeInvocation::Error(error)
            }
            (NativeStatus::Ok, None) => NativeInvocation::ContractViolation(format!(
                "native `{}.{}` returned ok without a result",
                self.0.module.inner.name, self.0.name
            )),
            (NativeStatus::Error, None) => NativeInvocation::ContractViolation(format!(
                "native `{}.{}` returned error without an error value",
                self.0.module.inner.name, self.0.name
            )),
            (NativeStatus::Ok, Some(NativeOutcome::Error(_))) => {
                NativeInvocation::ContractViolation(
                    "native callback returned ok after setting an error".into(),
                )
            }
            (NativeStatus::Error, Some(NativeOutcome::Result(_))) => {
                NativeInvocation::ContractViolation(
                    "native callback returned error after setting a result".into(),
                )
            }
        }
    }
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native {}.{}>", self.0.module.inner.name, self.0.name)
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

#[doc(hidden)]
pub struct NativeResource {
    registration: Rc<ResourceTypeRegistration>,
    module: Rc<NativeModuleInner>,
    payload: RefCell<Option<Box<dyn Any>>>,
    closed: Cell<bool>,
}

impl fmt::Debug for NativeResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeResource")
            .field("module", &self.module.name)
            .field("resource_type", &self.registration.name)
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}

impl NativeResource {
    pub(crate) fn close(&self) -> Result<(), String> {
        if self.closed.replace(true) {
            return Ok(());
        }
        let mut payload = self.payload.try_borrow_mut().map_err(|_| {
            format!(
                "native resource `{}` is already in use",
                self.registration.name
            )
        })?;
        let Some(payload) = payload.as_deref_mut() else {
            return Ok(());
        };
        catch_unwind(AssertUnwindSafe(|| (self.registration.close)(payload))).map_err(|_| {
            format!(
                "native resource `{}` close callback panicked",
                self.registration.name
            )
        })
    }
}

impl Drop for NativeResource {
    fn drop(&mut self) {
        if let Some(payload) = self.payload.get_mut().take() {
            let _ = catch_unwind(AssertUnwindSafe(|| (self.registration.destroy)(payload)));
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
            closed: Cell::new(false),
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
        if resource.closed.get() {
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
}

pub(crate) enum NativeInvocation {
    Result(Value, Vec<Weak<NativeResource>>),
    Error(NativeError),
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
