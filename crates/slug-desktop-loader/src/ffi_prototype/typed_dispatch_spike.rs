//! Private, removable dispatch candidates for the typed-native-call spike.
//!
//! This is intentionally not a manifest reader or a replacement ABI. It
//! accepts only the fixture's finite shape table so the experiment can compare
//! a header-checked C shim with direct Rust function-pointer dispatch.

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    rc::Rc,
    sync::Arc,
};

use slug_vm::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeFunction, NativeModule,
    NativeOwnedValue, NativeResourceType, NativeStatus,
};

use super::{
    FfiPrototypeError,
    loader::{LoadedLibrary, library_lease},
};

const STATUS_OK: i32 = 0;
const STATUS_EOF: i32 = 1;
const STATUS_IO: i32 = 2;

type Open = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type ReadLine = unsafe extern "C" fn(*mut c_void, *mut *const c_char) -> i32;
type Write = unsafe extern "C" fn(*mut c_void, *const c_char, u64) -> i64;
type Close = unsafe extern "C" fn(*mut c_void) -> i32;
type Add = unsafe extern "C" fn(i64, i64) -> i64;

/// The only direct-call shapes supported by this spike.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchShape {
    CstrToHandle,
    HandleOutTextToStatus,
    HandleStrToI64,
    HandleToStatus,
    I64I64ToI64,
}

impl DispatchShape {
    #[must_use]
    pub const fn grammar(self) -> &'static str {
        match self {
            Self::CstrToHandle => "(cstr) -> handle",
            Self::HandleOutTextToStatus => "(handle, out text) -> status",
            Self::HandleStrToI64 => "(handle, str) -> i64",
            Self::HandleToStatus => "(handle) -> status",
            Self::I64I64ToI64 => "(i64, i64) -> i64",
        }
    }

    /// Parses only the finite spike grammar; arbitrary C signatures are not a
    /// fallback path for this dispatcher.
    pub fn parse(grammar: &str) -> Result<Self, FfiPrototypeError> {
        [
            Self::CstrToHandle,
            Self::HandleOutTextToStatus,
            Self::HandleStrToI64,
            Self::HandleToStatus,
            Self::I64I64ToI64,
        ]
        .into_iter()
        .find(|shape| shape.grammar() == grammar)
        .ok_or_else(|| {
            FfiPrototypeError::new(format!("unsupported typed-dispatch shape `{grammar}`"))
        })
    }
}

/// One private fixture binding. Names are Slug-visible only in the tests.
#[derive(Clone, Copy, Debug)]
pub struct DispatchBinding {
    pub name: &'static str,
    pub symbol: &'static str,
    pub shape: DispatchShape,
}

/// The fixture contract shared by the generated-shim and Rust candidates.
pub const FIXTURE_BINDINGS: [DispatchBinding; 6] = [
    DispatchBinding {
        name: "openRead",
        symbol: "slug_spike_open_read",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "openWrite",
        symbol: "slug_spike_open_write",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "openAppend",
        symbol: "slug_spike_open_append",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "readLine",
        symbol: "slug_spike_read_line",
        shape: DispatchShape::HandleOutTextToStatus,
    },
    DispatchBinding {
        name: "write",
        symbol: "slug_spike_write",
        shape: DispatchShape::HandleStrToI64,
    },
    DispatchBinding {
        name: "close",
        symbol: "slug_spike_close",
        shape: DispatchShape::HandleToStatus,
    },
];

pub const ADD_BINDING: DispatchBinding = DispatchBinding {
    name: "add",
    symbol: "slug_spike_add",
    shape: DispatchShape::I64I64ToI64,
};

pub const FIXTURE_SHIM_BINDINGS: [DispatchBinding; 6] = [
    DispatchBinding {
        name: "openRead",
        symbol: "slug_shim_open_read",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "openWrite",
        symbol: "slug_shim_open_write",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "openAppend",
        symbol: "slug_shim_open_append",
        shape: DispatchShape::CstrToHandle,
    },
    DispatchBinding {
        name: "readLine",
        symbol: "slug_shim_read_line",
        shape: DispatchShape::HandleOutTextToStatus,
    },
    DispatchBinding {
        name: "write",
        symbol: "slug_shim_write",
        shape: DispatchShape::HandleStrToI64,
    },
    DispatchBinding {
        name: "close",
        symbol: "slug_shim_close",
        shape: DispatchShape::HandleToStatus,
    },
];

pub const SHIM_ADD_BINDING: DispatchBinding = DispatchBinding {
    name: "add",
    symbol: "slug_shim_add",
    shape: DispatchShape::I64I64ToI64,
};

#[derive(Clone, Copy)]
enum ResolvedBinding {
    Open(Open),
    ReadLine(ReadLine),
    Write(Write),
    Close(Close),
    Add(Add),
}

struct Handle {
    pointer: *mut c_void,
    close: Close,
    closed: bool,
}

struct State {
    _library: Arc<LoadedLibrary>,
    bindings: HashMap<&'static str, ResolvedBinding>,
    handle: Option<NativeResourceType<Handle>>,
}

/// Loaded finite-shape dispatcher for one direct fixture library.
pub struct TypedDispatchSpike {
    module: NativeModule,
    state: Rc<RefCell<State>>,
    bindings: Vec<DispatchBinding>,
}

impl TypedDispatchSpike {
    /// Loads only the declared finite table. A different shape is rejected
    /// before a symbol is called, rather than falling back to arbitrary C FFI.
    pub fn load(
        path: impl AsRef<Path>,
        bindings: &[DispatchBinding],
    ) -> Result<Self, FfiPrototypeError> {
        let library = library_lease(path.as_ref())?;
        let state = Rc::new(RefCell::new(State {
            _library: library.clone(),
            bindings: HashMap::new(),
            handle: None,
        }));
        let module = NativeModule::new("slug.spike", state.clone())
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
        let handle = module
            .resource_type("File", close_handle, destroy_handle)
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
        state.borrow_mut().handle = Some(handle);

        for binding in bindings {
            let resolved = resolve_binding(&library, *binding)?;
            if state
                .borrow_mut()
                .bindings
                .insert(binding.name, resolved)
                .is_some()
            {
                return Err(FfiPrototypeError::new(format!(
                    "typed-dispatch spike binding `{}` is duplicated",
                    binding.name
                )));
            }
        }
        Ok(Self {
            module,
            state,
            bindings: bindings.to_vec(),
        })
    }

    /// Produces source-foreign registrations for the fixture's `slug.spike`
    /// module. This leaves the VM and frontend free of unsafe code.
    pub fn foreign_functions(&self) -> Result<Vec<NativeFunction>, NativeDescriptorError> {
        self.bindings
            .iter()
            .map(|binding| {
                let (arity, callback): (
                    NativeArity,
                    for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
                ) = match binding.shape {
                    DispatchShape::CstrToHandle => (NativeArity::Exact(1), open),
                    DispatchShape::HandleOutTextToStatus => (NativeArity::Exact(1), read_line),
                    DispatchShape::HandleStrToI64 => (NativeArity::Exact(2), write),
                    DispatchShape::HandleToStatus => (NativeArity::Exact(1), close),
                    DispatchShape::I64I64ToI64 => (NativeArity::Exact(2), add),
                };
                self.module
                    .function_with_member_key(binding.name, arity, binding.name, callback)
            })
            .collect()
    }

    #[must_use]
    pub fn shape_grammar() -> [&'static str; 5] {
        [
            DispatchShape::CstrToHandle.grammar(),
            DispatchShape::HandleOutTextToStatus.grammar(),
            DispatchShape::HandleStrToI64.grammar(),
            DispatchShape::HandleToStatus.grammar(),
            DispatchShape::I64I64ToI64.grammar(),
        ]
    }
}

fn resolve_binding(
    library: &Arc<LoadedLibrary>,
    binding: DispatchBinding,
) -> Result<ResolvedBinding, FfiPrototypeError> {
    let symbol = CString::new(binding.symbol).map_err(|_| {
        FfiPrototypeError::new("typed-dispatch spike symbol contains an interior NUL")
    })?;
    // SAFETY: this table is deliberately finite. Each requested pointer type is
    // the fixture contract being tested; a wrong external declaration remains
    // an explicit unsupported safety risk for the Rust-dispatch candidate.
    unsafe {
        match binding.shape {
            DispatchShape::CstrToHandle => {
                Ok(ResolvedBinding::Open(library.symbol(symbol.as_c_str())?))
            }
            DispatchShape::HandleOutTextToStatus => Ok(ResolvedBinding::ReadLine(
                library.symbol(symbol.as_c_str())?,
            )),
            DispatchShape::HandleStrToI64 => {
                Ok(ResolvedBinding::Write(library.symbol(symbol.as_c_str())?))
            }
            DispatchShape::HandleToStatus => {
                Ok(ResolvedBinding::Close(library.symbol(symbol.as_c_str())?))
            }
            DispatchShape::I64I64ToI64 => {
                Ok(ResolvedBinding::Add(library.symbol(symbol.as_c_str())?))
            }
        }
    }
}

fn state(call: &NativeCall<'_>) -> Result<Rc<RefCell<State>>, NativeError> {
    call.state::<Rc<RefCell<State>>>().cloned().ok_or_else(|| {
        NativeError::new("native.spike", "typed-dispatch spike state is unavailable")
    })
}

fn binding(call: &NativeCall<'_>, expected: DispatchShape) -> Result<ResolvedBinding, NativeError> {
    let name = call
        .member_key()
        .ok_or_else(|| NativeError::new("native.spike", "missing binding key"))?;
    let binding = state(call)?
        .borrow()
        .bindings
        .get(name)
        .copied()
        .ok_or_else(|| {
            NativeError::new("native.spike", format!("binding `{name}` is unavailable"))
        })?;
    if matches!(
        (binding, expected),
        (ResolvedBinding::Open(_), DispatchShape::CstrToHandle)
            | (
                ResolvedBinding::ReadLine(_),
                DispatchShape::HandleOutTextToStatus
            )
            | (ResolvedBinding::Write(_), DispatchShape::HandleStrToI64)
            | (ResolvedBinding::Close(_), DispatchShape::HandleToStatus)
            | (ResolvedBinding::Add(_), DispatchShape::I64I64ToI64)
    ) {
        Ok(binding)
    } else {
        Err(NativeError::new(
            "native.spike",
            "binding shape does not match its dispatcher",
        ))
    }
}

fn handle_type(call: &NativeCall<'_>) -> Result<NativeResourceType<Handle>, NativeError> {
    state(call)?.borrow().handle.clone().ok_or_else(|| {
        NativeError::new(
            "native.spike",
            "typed-dispatch spike handle type is unavailable",
        )
    })
}

fn cstr_argument(call: &NativeCall<'_>, index: usize) -> Result<CString, NativeError> {
    CString::new(call.argument(index)?.as_str()?).map_err(|_| {
        NativeError::new(
            "native.interior_nul",
            "cstr argument contains an interior NUL byte",
        )
    })
}

fn open(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let ResolvedBinding::Open(function) = binding(call, DispatchShape::CstrToHandle)? else {
            unreachable!()
        };
        let path = cstr_argument(call, 0)?;
        // SAFETY: the fixture shape passes a temporary NUL-terminated path.
        let pointer = unsafe { function(path.as_ptr()) };
        if pointer.is_null() {
            return Err(NativeError::new("native.io", "native open failed"));
        }
        let Some(ResolvedBinding::Close(close)) =
            state(call)?.borrow().bindings.get("close").copied()
        else {
            return Err(NativeError::new(
                "native.spike",
                "close binding is unavailable",
            ));
        };
        call.resource(
            &handle_type(call)?,
            Handle {
                pointer,
                close,
                closed: false,
            },
        )
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}

fn read_line(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let ResolvedBinding::ReadLine(function) =
            binding(call, DispatchShape::HandleOutTextToStatus)?
        else {
            unreachable!()
        };
        let resource = handle_type(call)?;
        call.with_resource(0, &resource, |handle| {
            let mut text = std::ptr::null();
            // SAFETY: the checked resource owns a fixture handle and `text` is a writable out slot.
            let status = unsafe { function(handle.pointer, &raw mut text) };
            match status {
                STATUS_OK if text.is_null() => Ok(NativeOwnedValue::nil()),
                STATUS_OK => {
                    // SAFETY: fixture text is borrowed and NUL-terminated for this call.
                    let text = unsafe { CStr::from_ptr(text) }.to_str().map_err(|_| {
                        NativeError::new("native.text", "native read returned non-UTF-8 text")
                    })?;
                    Ok(NativeOwnedValue::string(text))
                }
                STATUS_EOF => Ok(NativeOwnedValue::nil()),
                STATUS_IO => Err(NativeError::new("native.io", "native read failed")),
                value => Err(NativeError::new(
                    "native.status",
                    format!("unknown native status {value}"),
                )),
            }
        })?
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}

fn write(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let ResolvedBinding::Write(function) = binding(call, DispatchShape::HandleStrToI64)? else {
            unreachable!()
        };
        let text = call.argument(1)?.as_str()?;
        let resource = handle_type(call)?;
        call.with_resource(0, &resource, |handle| {
            // SAFETY: text is borrowed for this callback and the handle is checked open.
            let written =
                unsafe { function(handle.pointer, text.as_ptr().cast(), text.len() as u64) };
            if written < 0 {
                Err(NativeError::new("native.io", "native write failed"))
            } else {
                Ok(NativeOwnedValue::integer(written))
            }
        })?
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}

fn close(call: &mut NativeCall<'_>) -> NativeStatus {
    match handle_type(call).and_then(|resource| call.close_resource(0, &resource)) {
        Ok(()) => call.return_value(NativeOwnedValue::nil()),
        Err(error) => call.raise(error),
    }
}

fn add(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let ResolvedBinding::Add(function) = binding(call, DispatchShape::I64I64ToI64)? else {
            unreachable!()
        };
        let left = call.argument(0)?.as_i64()?;
        let right = call.argument(1)?.as_i64()?;
        // SAFETY: the resolved binding has the finite `(i64, i64) -> i64` fixture shape.
        Ok(NativeOwnedValue::integer(unsafe { function(left, right) }))
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}

fn close_handle(handle: &mut Handle) {
    if !handle.closed {
        // SAFETY: the handle and matching close function are owned by this resource.
        let _ = unsafe { (handle.close)(handle.pointer) };
        handle.closed = true;
    }
}

fn destroy_handle(mut handle: Handle) {
    close_handle(&mut handle);
}
