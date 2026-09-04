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
    sync::LazyLock,
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
    table_size: u32,
    argument_i64: unsafe extern "C" fn(*mut c_void, usize, *mut i64) -> bool,
    argument_f64: unsafe extern "C" fn(*mut c_void, usize, *mut f64) -> bool,
    set_i64: unsafe extern "C" fn(*mut c_void, i64),
    set_f64: unsafe extern "C" fn(*mut c_void, f64),
    set_error: unsafe extern "C" fn(*mut c_void, FfiText, FfiText),
}

type Callback = unsafe extern "C" fn(*const HostApi, *mut c_void) -> i32;
type ModuleInit = unsafe extern "C" fn(*const HostApi) -> *const ModuleDescriptor;

#[repr(C)]
#[derive(Clone, Copy)]
struct FfiText {
    data: *const c_char,
    length: u64,
}

#[repr(C)]
struct FunctionDescriptor {
    descriptor_size: u32,
    name: FfiText,
    member_key: FfiText,
    minimum_arity: u64,
    maximum_arity: u64,
    callback: Option<Callback>,
}

#[repr(C)]
struct ModuleDescriptor {
    abi_major: u32,
    abi_minor: u32,
    descriptor_size: u32,
    module_name: FfiText,
    functions: *const FunctionDescriptor,
    function_count: u64,
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

#[derive(Clone, Debug)]
struct RegisteredFunction {
    name: String,
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
    functions: Vec<(String, NativeArity, String)>,
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
            let descriptor = init(host_api());
            let (module_name, functions) = validate_descriptor(descriptor)?;
            let registered = functions
                .iter()
                .map(|(member_key, function)| {
                    (function.name.clone(), function.arity, member_key.clone())
                })
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
        let functions = self
            .functions
            .iter()
            .map(|(name, arity, member_key)| {
                self.module.function_with_member_key(
                    name.clone(),
                    *arity,
                    member_key.clone(),
                    ffi_callback,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        vm.define_foreign_batch(functions)
    }
}

static HOST_API: LazyLock<HostApi> = LazyLock::new(|| HostApi {
    abi_major: ABI_MAJOR,
    abi_minor: ABI_MINOR,
    table_size: u32::try_from(size_of::<HostApi>()).expect("prototype host table fits u32"),
    argument_i64,
    argument_f64,
    set_i64,
    set_f64,
    set_error,
});

fn host_api() -> *const HostApi {
    &raw const *HOST_API
}

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

unsafe extern "C" fn set_error(context: *mut c_void, code: FfiText, message: FfiText) {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return;
    };
    let code = unsafe { text_from_ffi(code) }.unwrap_or_else(|| "native.contract".into());
    let message = unsafe { text_from_ffi(message) }
        .unwrap_or_else(|| "FFI module returned invalid error text".into());
    call.set_error(NativeError::new(code, message));
}

unsafe fn call_from_context<'call>(context: *mut c_void) -> Option<&'call mut NativeCall<'call>> {
    let bridge = unsafe { context.cast::<CallBridge<'call>>().as_mut() }?;
    unsafe { bridge.call.as_mut() }
}

unsafe fn text_from_ffi(value: FfiText) -> Option<String> {
    let length = usize::try_from(value.length).ok()?;
    if value.data.is_null() && length > 0 {
        return None;
    }
    if length == 0 {
        return Some(String::new());
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.data.cast::<u8>(), length) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

unsafe fn c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value).to_str().ok().map(str::to_owned) }
}

fn ffi_callback(call: &mut NativeCall<'_>) -> NativeStatus {
    let function = call.member_key().and_then(|member_key| {
        call.state::<FfiModuleState>()
            .and_then(|state| state.functions.get(member_key).cloned())
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
    match unsafe { (function.callback)(host_api(), (&raw mut bridge).cast()) } {
        0 => NativeStatus::Ok,
        1 => NativeStatus::Error,
        status => {
            call.report_contract_violation(format!("FFI callback returned unknown status {status}"))
        }
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
        || descriptor.descriptor_size
            < u32::try_from(size_of::<ModuleDescriptor>()).expect("descriptor fits u32")
    {
        return Err(FfiPrototypeError::new(
            "FFI module requires an unsupported ABI table",
        ));
    }
    let function_count = usize::try_from(descriptor.function_count)
        .map_err(|_| FfiPrototypeError::new("FFI module function count exceeds host limits"))?;
    if function_count > MAX_FUNCTIONS {
        return Err(FfiPrototypeError::new(
            "FFI module declares too many functions",
        ));
    }
    let module_name = unsafe { text_from_ffi(descriptor.module_name) }
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| FfiPrototypeError::new("FFI module has an invalid name"))?;
    if function_count > 0 && descriptor.functions.is_null() {
        return Err(FfiPrototypeError::new(
            "FFI module has a null function table",
        ));
    }
    let descriptors = unsafe { std::slice::from_raw_parts(descriptor.functions, function_count) };
    let mut functions = HashMap::new();
    for function in descriptors {
        if function.descriptor_size
            < u32::try_from(size_of::<FunctionDescriptor>()).expect("function descriptor fits u32")
        {
            return Err(FfiPrototypeError::new(
                "FFI module has an undersized function descriptor",
            ));
        }
        let name = unsafe { text_from_ffi(function.name) }
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| FfiPrototypeError::new("FFI module has an invalid function name"))?;
        let callback = function.callback.ok_or_else(|| {
            FfiPrototypeError::new(format!("FFI function `{name}` has no callback"))
        })?;
        let minimum_arity = usize::try_from(function.minimum_arity).map_err(|_| {
            FfiPrototypeError::new(format!("FFI function `{name}` minimum arity is too large"))
        })?;
        let maximum_arity = usize::try_from(function.maximum_arity).map_err(|_| {
            FfiPrototypeError::new(format!("FFI function `{name}` maximum arity is too large"))
        })?;
        if minimum_arity != maximum_arity {
            return Err(FfiPrototypeError::new(format!(
                "FFI prototype function `{name}` must have exact arity"
            )));
        }
        let member_key = unsafe { text_from_ffi(function.member_key) }
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                FfiPrototypeError::new(format!("FFI function `{name}` has an invalid member key"))
            })?;
        let arity = NativeArity::Exact(minimum_arity);
        if functions
            .insert(
                member_key.clone(),
                RegisteredFunction {
                    name,
                    arity,
                    callback,
                },
            )
            .is_some()
        {
            return Err(FfiPrototypeError::new(format!(
                "FFI module declares member key `{member_key}` more than once"
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
    unsafe { c_string(error) }.unwrap_or_else(|| "unknown dynamic loader error".into())
}
