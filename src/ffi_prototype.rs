//! Unstable, test-only bridge for exercising a minimal Slug-aware C module.
//!
//! This is deliberately not the version 1 native ABI. It contains the unsafe
//! dynamic-loader and raw-pointer work in one feature-gated module so the rest
//! of the runtime continues to prohibit unsafe code.

use std::{
    collections::HashMap,
    error::Error,
    ffi::{CStr, CString, c_char, c_void},
    fmt,
    mem::size_of,
    os::unix::ffi::OsStrExt,
    path::Path,
};

use crate::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeModule, NativeOwnedValue,
    NativeStatus, Vm,
};

const ABI_MAJOR: u32 = 0;
const ABI_MINOR: u32 = 1;
const MAX_FUNCTIONS: usize = 64;

#[repr(C)]
struct HostApi {
    abi_major: u32,
    abi_minor: u32,
    table_size: usize,
    argument_i64: unsafe extern "C" fn(*mut c_void, usize, *mut i64) -> bool,
    argument_f64: unsafe extern "C" fn(*mut c_void, usize, *mut f64) -> bool,
    set_i64: unsafe extern "C" fn(*mut c_void, i64),
    set_f64: unsafe extern "C" fn(*mut c_void, f64),
    set_error: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
}

type Callback = unsafe extern "C" fn(*const HostApi, *mut c_void) -> CStatus;
type ModuleInit = unsafe extern "C" fn(*const HostApi) -> *const ModuleDescriptor;

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
#[allow(dead_code)]
enum CStatus {
    Ok = 0,
    Error = 1,
}

#[repr(C)]
struct FunctionDescriptor {
    name: *const c_char,
    minimum_arity: usize,
    maximum_arity: usize,
    callback: Option<Callback>,
}

#[repr(C)]
struct ModuleDescriptor {
    abi_major: u32,
    abi_minor: u32,
    descriptor_size: usize,
    module_name: *const c_char,
    functions: *const FunctionDescriptor,
    function_count: usize,
}

struct CallBridge<'call> {
    call: *mut NativeCall<'call>,
}

struct LoadedLibrary(*mut c_void);

impl LoadedLibrary {
    unsafe fn open(path: &Path) -> Result<Self, FfiPrototypeError> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| FfiPrototypeError::new("FFI module path contains an interior NUL byte"))?;
        // SAFETY: `path` is a NUL-terminated byte string that remains live for the call.
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(FfiPrototypeError::new(format!(
                "cannot load FFI module: {}",
                unsafe { loader_error() }
            )));
        }
        Ok(Self(handle))
    }

    unsafe fn symbol<T>(&self, name: &CStr) -> Result<T, FfiPrototypeError>
    where
        T: Copy,
    {
        // SAFETY: `self.0` is an open library handle and `name` is NUL-terminated.
        let symbol = unsafe { dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(FfiPrototypeError::new(format!(
                "FFI module is missing `{}`: {}",
                name.to_string_lossy(),
                unsafe { loader_error() }
            )));
        }
        // SAFETY: the caller requests a symbol with the exact ABI documented by this module.
        Ok(unsafe { std::mem::transmute_copy(&symbol) })
    }
}

impl fmt::Debug for LoadedLibrary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<loaded ffi prototype library>")
    }
}

// Prototype modules intentionally remain loaded for process lifetime, matching
// the planned native ABI's no-unload rule. Keeping the handle in module state
// also makes the callback pointers valid for as long as the state is live.

#[derive(Clone, Copy, Debug)]
struct RegisteredFunction {
    arity: NativeArity,
    callback: Callback,
}

#[derive(Debug)]
struct FfiModuleState {
    _library: LoadedLibrary,
    functions: HashMap<String, RegisteredFunction>,
}

/// A loaded, deliberately unstable Slug-aware C module.
#[derive(Clone)]
pub struct FfiPrototypeModule {
    module: NativeModule,
    functions: Vec<(String, NativeArity)>,
}

/// A checked failure while loading or registering an FFI prototype module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FfiPrototypeError {
    message: String,
}

impl FfiPrototypeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FfiPrototypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FfiPrototypeError {}

impl FfiPrototypeModule {
    /// Loads and validates one C module that follows the prototype header.
    ///
    /// The library remains loaded for process lifetime. This API is available
    /// only behind the `ffi-prototype` feature and is not an ABI promise.
    ///
    /// # Errors
    ///
    /// Returns an error when the dynamic library cannot load, lacks the entry
    /// symbol, or provides a malformed or incompatible descriptor.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FfiPrototypeError> {
        // SAFETY: all dynamic-loader interactions and C descriptor reads are
        // validated within this feature-gated boundary.
        unsafe {
            let library = LoadedLibrary::open(path.as_ref())?;
            let init_name = c"slug_ffi_module_init";
            let init: ModuleInit = library.symbol(init_name)?;
            let descriptor = init(&raw const HOST_API);
            let (module_name, functions) = validate_descriptor(descriptor)?;
            let registered = functions
                .iter()
                .map(|(name, function)| (name.clone(), function.arity))
                .collect();
            let module = NativeModule::new(
                module_name,
                FfiModuleState {
                    _library: library,
                    functions,
                },
            )
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
            Ok(Self {
                module,
                functions: registered,
            })
        }
    }

    /// Installs the module's descriptors into a loader-backed VM.
    ///
    /// # Errors
    ///
    /// Returns an error when the VM cannot register a descriptor.
    pub fn register(&self, vm: &mut Vm) -> Result<(), NativeDescriptorError> {
        for (name, arity) in &self.functions {
            let function = self.module.function(name.clone(), *arity, ffi_callback)?;
            vm.define_foreign(function)?;
        }
        Ok(())
    }
}

static HOST_API: HostApi = HostApi {
    abi_major: ABI_MAJOR,
    abi_minor: ABI_MINOR,
    table_size: size_of::<HostApi>(),
    argument_i64,
    argument_f64,
    set_i64,
    set_f64,
    set_error,
};

unsafe extern "C" fn argument_i64(context: *mut c_void, index: usize, output: *mut i64) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI integer output is null",
        ));
        return false;
    }
    match call.argument(index).and_then(crate::NativeValueRef::as_i64) {
        Ok(value) => {
            // SAFETY: checked non-null above; C owns the pointed-to output slot.
            unsafe { *output = value };
            true
        }
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn argument_f64(context: *mut c_void, index: usize, output: *mut f64) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI float output is null",
        ));
        return false;
    }
    match call.argument(index).and_then(crate::NativeValueRef::as_f64) {
        Ok(value) => {
            // SAFETY: checked non-null above; C owns the pointed-to output slot.
            unsafe { *output = value };
            true
        }
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn set_i64(context: *mut c_void, value: i64) {
    if let Some(call) = unsafe { call_from_context(context) } {
        call.set_result(NativeOwnedValue::integer(value));
    }
}

unsafe extern "C" fn set_f64(context: *mut c_void, value: f64) {
    if let Some(call) = unsafe { call_from_context(context) } {
        call.set_result(NativeOwnedValue::float(value));
    }
}

unsafe extern "C" fn set_error(context: *mut c_void, code: *const c_char, message: *const c_char) {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return;
    };
    let code = unsafe { c_string(code) }.unwrap_or("native.contract");
    let message = unsafe { c_string(message) }.unwrap_or("FFI module returned invalid error text");
    call.set_error(NativeError::new(code, message));
}

unsafe fn call_from_context<'call>(context: *mut c_void) -> Option<&'call mut NativeCall<'call>> {
    let bridge = unsafe { context.cast::<CallBridge<'call>>().as_mut() }?;
    unsafe { bridge.call.as_mut() }
}

unsafe fn c_string(value: *const c_char) -> Option<&'static str> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value).to_str().ok() }
}

fn ffi_callback(call: &mut NativeCall<'_>) -> NativeStatus {
    let function = call.state::<FfiModuleState>().and_then(|state| {
        state
            .functions
            .values()
            .find(|function| arity_accepts(function.arity, call.argument_count()))
            .copied()
    });
    let Some(function) = function else {
        return call.raise(NativeError::new(
            "native.contract",
            "no matching FFI function",
        ));
    };
    let mut bridge = CallBridge { call };
    // SAFETY: the callback, host table, and call bridge follow the prototype
    // header and remain valid for the synchronous dynamic extent of this call.
    match unsafe { (function.callback)(&raw const HOST_API, (&raw mut bridge).cast()) } {
        CStatus::Ok => NativeStatus::Ok,
        CStatus::Error => NativeStatus::Error,
    }
}

fn arity_accepts(arity: NativeArity, count: usize) -> bool {
    match arity {
        NativeArity::Exact(expected) => count == expected,
        NativeArity::Range { minimum, maximum } => (minimum..=maximum).contains(&count),
        NativeArity::Variadic { minimum } => count >= minimum,
    }
}

unsafe fn validate_descriptor(
    descriptor: *const ModuleDescriptor,
) -> Result<(String, HashMap<String, RegisteredFunction>), FfiPrototypeError> {
    let descriptor = unsafe { descriptor.as_ref() }
        .ok_or_else(|| FfiPrototypeError::new("FFI module returned a null descriptor"))?;
    if descriptor.abi_major != ABI_MAJOR {
        return Err(FfiPrototypeError::new(format!(
            "FFI module requires ABI major {}, host supports {ABI_MAJOR}",
            descriptor.abi_major
        )));
    }
    if descriptor.abi_minor > ABI_MINOR
        || descriptor.descriptor_size < size_of::<ModuleDescriptor>()
    {
        return Err(FfiPrototypeError::new(
            "FFI module requires an unsupported ABI table",
        ));
    }
    if descriptor.function_count > MAX_FUNCTIONS {
        return Err(FfiPrototypeError::new(
            "FFI module declares too many functions",
        ));
    }
    let module_name = unsafe { c_string(descriptor.module_name) }
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| FfiPrototypeError::new("FFI module has an invalid name"))?
        .to_owned();
    if descriptor.function_count > 0 && descriptor.functions.is_null() {
        return Err(FfiPrototypeError::new(
            "FFI module has a null function table",
        ));
    }
    let descriptors =
        unsafe { std::slice::from_raw_parts(descriptor.functions, descriptor.function_count) };
    let mut functions = HashMap::new();
    for function in descriptors {
        let name = unsafe { c_string(function.name) }
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| FfiPrototypeError::new("FFI module has an invalid function name"))?
            .to_owned();
        let callback = function.callback.ok_or_else(|| {
            FfiPrototypeError::new(format!("FFI function `{name}` has no callback"))
        })?;
        if function.minimum_arity != function.maximum_arity {
            return Err(FfiPrototypeError::new(format!(
                "FFI prototype function `{name}` must have exact arity"
            )));
        }
        let arity = NativeArity::Exact(function.minimum_arity);
        if functions
            .values()
            .any(|existing: &RegisteredFunction| existing.arity == arity)
        {
            return Err(FfiPrototypeError::new(format!(
                "FFI prototype functions must have distinct arities; `{name}` conflicts with another descriptor"
            )));
        }
        if functions
            .insert(name.clone(), RegisteredFunction { arity, callback })
            .is_some()
        {
            return Err(FfiPrototypeError::new(format!(
                "FFI module declares `{name}` more than once"
            )));
        }
    }
    Ok((module_name, functions))
}

#[cfg(target_os = "macos")]
const RTLD_NOW: i32 = 2;

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

unsafe fn loader_error() -> String {
    let error = unsafe { dlerror() };
    unsafe { c_string(error) }
        .unwrap_or("unknown dynamic loader error")
        .to_owned()
}
