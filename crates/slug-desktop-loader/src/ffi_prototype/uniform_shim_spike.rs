//! Private loader experiment for one uniform generated-shim entry point.

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

const ABI_VERSION: u32 = 1;
const FIXTURE_BINDING_COUNT: u32 = 7;

#[repr(C)]
union ValueData {
    i64: i64,
    pointer: *const c_void,
}

#[repr(C)]
struct Value {
    data: ValueData,
    length: u64,
}

#[repr(C)]
struct Cleanup {
    release: Option<unsafe extern "C" fn(*mut c_void)>,
    context: *mut c_void,
}

type Entry = unsafe extern "C" fn(*const Value, *mut Value, *mut Cleanup) -> i32;
type GetTable = unsafe extern "C" fn() -> *const Table;

#[repr(C)]
struct Binding {
    module: *const c_char,
    name: *const c_char,
    entry: Entry,
    argument_count: u32,
    result_count: u32,
    signature_hash: u64,
}

#[repr(C)]
struct Table {
    abi_version: u32,
    binding_count: u32,
    bindings: *const Binding,
    _table_hash: u64,
}

#[derive(Clone, Copy)]
struct ExpectedBinding {
    name: &'static str,
    arguments: u32,
    results: u32,
    signature_hash: u64,
}

const EXPECTED: [ExpectedBinding; 7] = [
    ExpectedBinding {
        name: "openRead",
        arguments: 1,
        results: 1,
        signature_hash: 0x1001,
    },
    ExpectedBinding {
        name: "openWrite",
        arguments: 1,
        results: 1,
        signature_hash: 0x1002,
    },
    ExpectedBinding {
        name: "openAppend",
        arguments: 1,
        results: 1,
        signature_hash: 0x1003,
    },
    ExpectedBinding {
        name: "readLine",
        arguments: 1,
        results: 1,
        signature_hash: 0x1004,
    },
    ExpectedBinding {
        name: "write",
        arguments: 2,
        results: 1,
        signature_hash: 0x1005,
    },
    ExpectedBinding {
        name: "close",
        arguments: 1,
        results: 0,
        signature_hash: 0x1006,
    },
    ExpectedBinding {
        name: "add",
        arguments: 2,
        results: 1,
        signature_hash: 0x1007,
    },
];

struct Handle {
    pointer: *const c_void,
    close: Entry,
    closed: bool,
}

struct State {
    _library: Arc<LoadedLibrary>,
    entries: HashMap<&'static str, Entry>,
    handle: Option<NativeResourceType<Handle>>,
}

/// Private fixture client for the generated uniform table. It has one entry
/// function-pointer type, independent of every target C signature.
pub struct UniformShimSpike {
    module: NativeModule,
}

impl UniformShimSpike {
    /// Loads and validates the private fixture's uniform shim table.
    ///
    /// # Errors
    ///
    /// Returns an error when the library, table, or fixture signatures are invalid.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FfiPrototypeError> {
        let library = library_lease(path.as_ref())?;
        let symbol = c"slug_shim_get_table";
        // SAFETY: this is the single table lookup specified by the private shim ABI.
        let get_table: GetTable = unsafe { library.symbol(symbol)? };
        // SAFETY: the generated library owns the returned static table for its lifetime.
        let table = unsafe { get_table().as_ref() }
            .ok_or_else(|| FfiPrototypeError::new("uniform shim returned a null table"))?;
        if table.abi_version != ABI_VERSION {
            return Err(FfiPrototypeError::new(format!(
                "uniform shim ABI version {} does not match {}",
                table.abi_version, ABI_VERSION
            )));
        }
        if table.binding_count != FIXTURE_BINDING_COUNT || table.bindings.is_null() {
            return Err(FfiPrototypeError::new(
                "uniform shim binding table does not match fixture",
            ));
        }
        // SAFETY: the table reports exactly its contiguous binding count.
        let bindings =
            unsafe { std::slice::from_raw_parts(table.bindings, table.binding_count as usize) };
        let mut entries = HashMap::new();
        for expected in EXPECTED {
            let binding = bindings
                .iter()
                .find(|binding| {
                    // SAFETY: generated fixture names are static NUL-terminated strings.
                    unsafe {
                        CStr::from_ptr(binding.module) == c"slug.spike"
                            && CStr::from_ptr(binding.name).to_bytes() == expected.name.as_bytes()
                    }
                })
                .ok_or_else(|| {
                    FfiPrototypeError::new(format!(
                        "uniform shim is missing `slug.spike.{}`",
                        expected.name
                    ))
                })?;
            if binding.argument_count != expected.arguments
                || binding.result_count != expected.results
                || binding.signature_hash != expected.signature_hash
            {
                return Err(FfiPrototypeError::new(format!(
                    "uniform shim binding `slug.spike.{}` does not match the fixture signature",
                    expected.name
                )));
            }
            if entries.insert(expected.name, binding.entry).is_some() {
                return Err(FfiPrototypeError::new(
                    "uniform shim has a duplicate fixture binding",
                ));
            }
        }
        let state = Rc::new(RefCell::new(State {
            _library: library,
            entries,
            handle: None,
        }));
        let module = NativeModule::new("slug.spike", state.clone())
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
        let handle = module
            .resource_type("File", close_handle, destroy_handle)
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
        state.borrow_mut().handle = Some(handle);
        Ok(Self { module })
    }

    /// Creates the fixture's Slug-visible registrations.
    ///
    /// # Errors
    ///
    /// Returns an error if a native descriptor cannot be constructed.
    pub fn foreign_functions(&self) -> Result<Vec<NativeFunction>, NativeDescriptorError> {
        [
            (
                "openRead",
                NativeArity::Exact(1),
                open as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "openWrite",
                NativeArity::Exact(1),
                open as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "openAppend",
                NativeArity::Exact(1),
                open as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "readLine",
                NativeArity::Exact(1),
                read_line as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "write",
                NativeArity::Exact(2),
                write as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "close",
                NativeArity::Exact(1),
                close as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
            (
                "add",
                NativeArity::Exact(2),
                add as for<'a> fn(&mut NativeCall<'a>) -> NativeStatus,
            ),
        ]
        .into_iter()
        .map(|(name, arity, callback)| {
            self.module
                .function_with_member_key(name, arity, name, callback)
        })
        .collect()
    }
}

fn state(call: &NativeCall<'_>) -> Result<Rc<RefCell<State>>, NativeError> {
    call.state::<Rc<RefCell<State>>>()
        .cloned()
        .ok_or_else(|| NativeError::new("native.spike", "uniform shim state is unavailable"))
}
fn entry(call: &NativeCall<'_>) -> Result<Entry, NativeError> {
    let name = call
        .member_key()
        .ok_or_else(|| NativeError::new("native.spike", "missing binding key"))?;
    state(call)?
        .borrow()
        .entries
        .get(name)
        .copied()
        .ok_or_else(|| NativeError::new("native.spike", format!("binding `{name}` is unavailable")))
}
fn handle_type(call: &NativeCall<'_>) -> Result<NativeResourceType<Handle>, NativeError> {
    state(call)?
        .borrow()
        .handle
        .clone()
        .ok_or_else(|| NativeError::new("native.spike", "uniform shim handle type is unavailable"))
}
fn invoke(entry: Entry, arguments: &[Value], results: &mut [Value]) -> Result<(), NativeError> {
    let mut cleanup = Cleanup {
        release: None,
        context: std::ptr::null_mut(),
    };
    // SAFETY: all entries share this exact ABI; cell arrays remain live through the call.
    let status = unsafe { entry(arguments.as_ptr(), results.as_mut_ptr(), &raw mut cleanup) };
    // SAFETY: a shim only supplies its own valid release callback and context.
    if let Some(release) = cleanup.release {
        unsafe { release(cleanup.context) };
    }
    if status == 0 {
        Ok(())
    } else {
        Err(NativeError::new(
            "native.io",
            format!("uniform shim failed with status {status}"),
        ))
    }
}
fn cstr(call: &NativeCall<'_>, index: usize) -> Result<CString, NativeError> {
    CString::new(call.argument(index)?.as_str()?).map_err(|_| {
        NativeError::new(
            "native.interior_nul",
            "cstr argument contains an interior NUL byte",
        )
    })
}
fn blank_values<const N: usize>() -> [Value; N] {
    std::array::from_fn(|_| Value {
        data: ValueData { i64: 0 },
        length: 0,
    })
}
fn open(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let path = cstr(call, 0)?;
        let args = [Value {
            data: ValueData {
                pointer: path.as_ptr().cast(),
            },
            length: 0,
        }];
        let mut results = blank_values::<1>();
        invoke(entry(call)?, &args, &mut results)?;
        let pointer = unsafe { results[0].data.pointer };
        if pointer.is_null() {
            return Err(NativeError::new("native.io", "native open failed"));
        }
        let close = state(call)?
            .borrow()
            .entries
            .get("close")
            .copied()
            .ok_or_else(|| NativeError::new("native.spike", "close binding is unavailable"))?;
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
        let e = entry(call)?;
        let resource = handle_type(call)?;
        call.with_resource(0, &resource, |handle| {
            let args = [Value {
                data: ValueData {
                    pointer: handle.pointer,
                },
                length: 0,
            }];
            let mut results = blank_values::<1>();
            invoke(e, &args, &mut results)?;
            let pointer = unsafe { results[0].data.pointer }.cast::<c_char>();
            if pointer.is_null() {
                Ok(NativeOwnedValue::nil())
            } else {
                let text = unsafe { CStr::from_ptr(pointer) }.to_str().map_err(|_| {
                    NativeError::new("native.text", "native read returned non-UTF-8 text")
                })?;
                Ok(NativeOwnedValue::string(text))
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
        let e = entry(call)?;
        let text = call.argument(1)?.as_str()?;
        let resource = handle_type(call)?;
        call.with_resource(0, &resource, |handle| {
            let args = [
                Value {
                    data: ValueData {
                        pointer: handle.pointer,
                    },
                    length: 0,
                },
                Value {
                    data: ValueData {
                        pointer: text.as_ptr().cast(),
                    },
                    length: text.len() as u64,
                },
            ];
            let mut results = blank_values::<1>();
            invoke(e, &args, &mut results)?;
            Ok(NativeOwnedValue::integer(unsafe { results[0].data.i64 }))
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
        let args = [
            Value {
                data: ValueData {
                    i64: call.argument(0)?.as_i64()?,
                },
                length: 0,
            },
            Value {
                data: ValueData {
                    i64: call.argument(1)?.as_i64()?,
                },
                length: 0,
            },
        ];
        let mut results = blank_values::<1>();
        invoke(entry(call)?, &args, &mut results)?;
        Ok(NativeOwnedValue::integer(unsafe { results[0].data.i64 }))
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}
fn close_handle(handle: &mut Handle) {
    if !handle.closed {
        let args = [Value {
            data: ValueData {
                pointer: handle.pointer,
            },
            length: 0,
        }];
        let mut results = [];
        let _ = invoke(handle.close, &args, &mut results);
        handle.closed = true;
    }
}
fn destroy_handle(mut handle: Handle) {
    close_handle(&mut handle);
}
