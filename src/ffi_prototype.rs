//! Unstable, test-only bridge for exercising a minimal Slug-aware C module.
//!
//! This is deliberately not the version 1 native ABI. It contains the unsafe
//! dynamic-loader and raw-pointer work in one feature-gated module so the rest
//! of the runtime continues to prohibit unsafe code.

use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    ffi::{CStr, CString, c_char, c_void},
    fmt,
    mem::size_of,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, LazyLock, Mutex},
};

use crate::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeModule, NativeOwnedValue,
    NativeProducerStatus, NativeSendValue, NativeStatus, Vm,
};

const ABI_MAJOR: u32 = 0;
const ABI_MINOR: u32 = 6;
const MAX_FUNCTIONS: usize = 64;
const MAX_RESOURCES: usize = 64;

#[repr(C)]
struct HostApi {
    abi_major: u32,
    abi_minor: u32,
    table_size: u32,
    argument_i64: unsafe extern "C" fn(*mut c_void, usize, *mut i64) -> bool,
    argument_f64: unsafe extern "C" fn(*mut c_void, usize, *mut f64) -> bool,
    argument_text: unsafe extern "C" fn(*mut c_void, usize, *mut FfiText) -> bool,
    argument_resource: unsafe extern "C" fn(*mut c_void, usize, FfiText, *mut *mut c_void) -> bool,
    set_i64: unsafe extern "C" fn(*mut c_void, i64),
    set_f64: unsafe extern "C" fn(*mut c_void, f64),
    set_error: unsafe extern "C" fn(*mut c_void, FfiText, FfiText),
    set_resource: unsafe extern "C" fn(*mut c_void, FfiText, *mut c_void) -> bool,
    close_resource: unsafe extern "C" fn(*mut c_void, usize, FfiText) -> bool,
    channel_create:
        unsafe extern "C" fn(*mut c_void, u64, *mut *mut FfiProducer) -> *mut FfiChannel,
    set_channel: unsafe extern "C" fn(*mut c_void, *mut FfiChannel) -> bool,
    channel_destroy: unsafe extern "C" fn(*mut FfiChannel),
    producer_send_i64: unsafe extern "C" fn(*mut FfiProducer, i64) -> i32,
    producer_destroy: unsafe extern "C" fn(*mut FfiProducer),
    producer_send_text:
        unsafe extern "C" fn(*mut FfiProducer, FfiText, Option<ProducerTextDestroy>) -> i32,
}

type Callback = unsafe extern "C" fn(*const HostApi, *mut c_void, *mut c_void) -> i32;
type ModuleDestroy = unsafe extern "C" fn(*mut c_void);
type ResourceDestroy = unsafe extern "C" fn(*mut c_void);
type ProducerTextDestroy = unsafe extern "C" fn(*mut c_void);
type ModuleInit = unsafe extern "C" fn(*const HostApi, *mut *mut c_void) -> *const ModuleDescriptor;

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
struct ResourceDescriptor {
    descriptor_size: u32,
    name: FfiText,
    destroy_resource: Option<ResourceDestroy>,
}

#[repr(C)]
struct ModuleDescriptor {
    abi_major: u32,
    abi_minor: u32,
    descriptor_size: u32,
    module_name: FfiText,
    destroy_module: Option<ModuleDestroy>,
    functions: *const FunctionDescriptor,
    function_count: u64,
    resources: *const ResourceDescriptor,
    resource_count: u64,
}

struct CallBridge<'call> {
    call: *mut NativeCall<'call>,
}

struct LoadedLibrary(*mut c_void);

unsafe impl Send for LoadedLibrary {}
unsafe impl Sync for LoadedLibrary {}

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

trait PlatformLoader {
    unsafe fn open(path: &Path) -> Result<LoadedLibrary, FfiPrototypeError>;

    unsafe fn symbol<T>(library: &LoadedLibrary, name: &CStr) -> Result<T, FfiPrototypeError>
    where
        T: Copy;
}

struct MacOsLoader;

impl PlatformLoader for MacOsLoader {
    unsafe fn open(path: &Path) -> Result<LoadedLibrary, FfiPrototypeError> {
        unsafe { LoadedLibrary::open(path) }
    }

    unsafe fn symbol<T>(library: &LoadedLibrary, name: &CStr) -> Result<T, FfiPrototypeError>
    where
        T: Copy,
    {
        unsafe { library.symbol(name) }
    }
}

static LIBRARIES: LazyLock<Mutex<HashMap<PathBuf, Arc<LoadedLibrary>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn resident_library(path: &Path) -> Result<Arc<LoadedLibrary>, FfiPrototypeError> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        FfiPrototypeError::new(format!("cannot resolve FFI module path: {error}"))
    })?;
    let mut libraries = LIBRARIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(library) = libraries.get(&path) {
        return Ok(library.clone());
    }
    // SAFETY: platform loading is contained in this feature-gated module.
    let library = Arc::new(unsafe { MacOsLoader::open(&path) }?);
    libraries.insert(path, library.clone());
    Ok(library)
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

type ValidatedDescriptor = (
    String,
    Option<ModuleDestroy>,
    HashMap<String, RegisteredFunction>,
    Vec<RegisteredResource>,
);

struct RegisteredResource {
    name: String,
    destroy: ResourceDestroy,
}

struct CResource {
    pointer: *mut c_void,
    destroy: ResourceDestroy,
}

struct FfiChannel {
    value: NativeOwnedValue,
}

struct FfiProducer {
    producer: crate::NativeChannelProducer,
}

#[derive(Clone)]
struct CResourceType {
    resource_type: crate::NativeResourceType<CResource>,
    destroy: ResourceDestroy,
}

struct FfiModuleState {
    _library: Arc<LoadedLibrary>,
    module_state: *mut c_void,
    destroy_module: Option<ModuleDestroy>,
    functions: HashMap<String, RegisteredFunction>,
    resources: Rc<RefCell<HashMap<String, CResourceType>>>,
}

impl Drop for FfiModuleState {
    fn drop(&mut self) {
        if !self.module_state.is_null()
            && let Some(destroy_module) = self.destroy_module
        {
            // SAFETY: the module owns this state and its library remains resident.
            unsafe { destroy_module(self.module_state) };
        }
    }
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
            let library = resident_library(path.as_ref())?;
            let init_name = c"slug_ffi_module_init";
            let init: ModuleInit = MacOsLoader::symbol(&library, init_name)?;
            let mut module_state = std::ptr::null_mut();
            let descriptor = init(host_api(), &raw mut module_state);
            let (module_name, destroy_module, functions, resources) =
                validate_descriptor(descriptor)?;
            if !module_state.is_null() && destroy_module.is_none() {
                return Err(FfiPrototypeError::new(
                    "FFI module returned state without a destroy callback",
                ));
            }
            let registered = functions
                .iter()
                .map(|(member_key, function)| {
                    (function.name.clone(), function.arity, member_key.clone())
                })
                .collect();
            let resource_types = Rc::new(RefCell::new(HashMap::new()));
            let module = NativeModule::new(
                module_name,
                FfiModuleState {
                    _library: library,
                    module_state,
                    destroy_module,
                    functions,
                    resources: resource_types.clone(),
                },
            )
            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
            let resource_types_to_register = resources
                .into_iter()
                .map(|resource| {
                    let resource_type = module
                        .resource_type(resource.name.clone(), close_c_resource, destroy_c_resource)
                        .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
                    Ok((resource.name, resource_type, resource.destroy))
                })
                .collect::<Result<Vec<_>, FfiPrototypeError>>()?;
            resource_types
                .borrow_mut()
                .extend(resource_types_to_register.into_iter().map(
                    |(name, resource_type, destroy)| {
                        (
                            name,
                            CResourceType {
                                resource_type,
                                destroy,
                            },
                        )
                    },
                ));
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
    argument_text,
    argument_resource,
    set_i64,
    set_f64,
    set_error,
    set_resource,
    close_resource,
    channel_create,
    set_channel,
    channel_destroy,
    producer_send_i64,
    producer_destroy,
    producer_send_text,
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

unsafe extern "C" fn argument_text(
    context: *mut c_void,
    index: usize,
    output: *mut FfiText,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI text output is null",
        ));
        return false;
    }
    match call.argument(index).and_then(crate::NativeValueRef::as_str) {
        Ok(value) => {
            let Ok(length) = u64::try_from(value.len()) else {
                call.set_error(NativeError::new(
                    "native.contract",
                    "FFI text length is too large",
                ));
                return false;
            };
            // SAFETY: checked non-null above; the text borrow is valid for this callback only.
            unsafe {
                *output = FfiText {
                    data: value.as_ptr().cast(),
                    length,
                };
            }
            true
        }
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn argument_resource(
    context: *mut c_void,
    index: usize,
    resource_name: FfiText,
    output: *mut *mut c_void,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI resource output is null",
        ));
        return false;
    }
    let Some(resource_type) = resource_type(call, resource_name) else {
        return false;
    };
    match call.with_resource(index, &resource_type.resource_type, |resource| {
        resource.pointer
    }) {
        Ok(pointer) => {
            // SAFETY: checked non-null above; the borrowed pointer is valid only during this callback.
            unsafe { *output = pointer };
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

unsafe extern "C" fn set_resource(
    context: *mut c_void,
    resource_name: FfiText,
    pointer: *mut c_void,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if pointer.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI resource pointer is null",
        ));
        return false;
    }
    let Some(resource_type) = resource_type(call, resource_name) else {
        return false;
    };
    match call.resource(
        &resource_type.resource_type,
        CResource {
            pointer,
            destroy: resource_type.destroy,
        },
    ) {
        Ok(value) => {
            call.set_result(value);
            true
        }
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn close_resource(
    context: *mut c_void,
    index: usize,
    resource_name: FfiText,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    let Some(resource_type) = resource_type(call, resource_name) else {
        return false;
    };
    match call.close_resource(index, &resource_type.resource_type) {
        Ok(()) => true,
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn channel_create(
    context: *mut c_void,
    capacity: u64,
    producer_output: *mut *mut FfiProducer,
) -> *mut FfiChannel {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return std::ptr::null_mut();
    };
    if producer_output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI channel producer output is null",
        ));
        return std::ptr::null_mut();
    }
    let Ok(capacity) = usize::try_from(capacity) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI channel capacity is too large",
        ));
        return std::ptr::null_mut();
    };
    let (value, producer) = call.channel(capacity);
    let producer = Box::into_raw(Box::new(FfiProducer { producer }));
    // SAFETY: checked non-null above; the C callback owns the returned producer.
    unsafe { *producer_output = producer };
    Box::into_raw(Box::new(FfiChannel { value }))
}

unsafe extern "C" fn set_channel(context: *mut c_void, channel: *mut FfiChannel) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if channel.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI channel handle is null",
        ));
        return false;
    }
    // SAFETY: a non-null channel handle is transferred once from the C callback.
    let channel = unsafe { Box::from_raw(channel) };
    call.set_result(channel.value);
    true
}

unsafe extern "C" fn channel_destroy(channel: *mut FfiChannel) {
    if !channel.is_null() {
        // SAFETY: C destroys only a channel handle it still owns.
        drop(unsafe { Box::from_raw(channel) });
    }
}

unsafe extern "C" fn producer_send_i64(producer: *mut FfiProducer, value: i64) -> i32 {
    let Some(producer) = (unsafe { producer.as_ref() }) else {
        return 2;
    };
    match producer.producer.try_send(NativeSendValue::integer(value)) {
        NativeProducerStatus::Sent => 0,
        NativeProducerStatus::Full(_) => 1,
        NativeProducerStatus::Closed(_) => 2,
    }
}

unsafe extern "C" fn producer_destroy(producer: *mut FfiProducer) {
    if !producer.is_null() {
        // SAFETY: C destroys only a producer capability it still owns.
        drop(unsafe { Box::from_raw(producer) });
    }
}

unsafe extern "C" fn producer_send_text(
    producer: *mut FfiProducer,
    text: FfiText,
    destroy: Option<ProducerTextDestroy>,
) -> i32 {
    let (Some(producer), Some(destroy)) = (unsafe { producer.as_ref() }, destroy) else {
        return 3;
    };
    let data = text.data;
    let Some(text) = (unsafe { text_from_ffi(text) }) else {
        return 3;
    };
    match producer.producer.try_send(NativeSendValue::string(text)) {
        NativeProducerStatus::Sent => {
            // SAFETY: C transfers ownership of the buffer only after a successful send.
            unsafe { destroy(data.cast_mut().cast()) };
            0
        }
        NativeProducerStatus::Full(_) => 1,
        NativeProducerStatus::Closed(_) => 2,
    }
}

fn resource_type(call: &mut NativeCall<'_>, resource_name: FfiText) -> Option<CResourceType> {
    let name = unsafe { text_from_ffi(resource_name) }.filter(|name| !name.trim().is_empty());
    let Some(name) = name else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI resource type is invalid",
        ));
        return None;
    };
    let resource_type = call
        .state::<FfiModuleState>()
        .and_then(|state| state.resources.borrow().get(&name).cloned());
    if resource_type.is_none() {
        call.set_error(NativeError::new(
            "native.contract",
            format!("FFI module has no resource type `{name}`"),
        ));
    }
    resource_type
}

fn close_c_resource(resource: &mut CResource) {
    if !resource.pointer.is_null() {
        // SAFETY: the resource descriptor owns this pointer until its one teardown call.
        unsafe { (resource.destroy)(resource.pointer) };
        resource.pointer = std::ptr::null_mut();
    }
}

fn destroy_c_resource(mut resource: CResource) {
    close_c_resource(&mut resource);
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
        call.state::<FfiModuleState>().and_then(|state| {
            state
                .functions
                .get(member_key)
                .cloned()
                .map(|function| (function, state.module_state))
        })
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
    match unsafe { (function.0.callback)(host_api(), (&raw mut bridge).cast(), function.1) } {
        0 => NativeStatus::Ok,
        1 => NativeStatus::Error,
        status => {
            call.report_contract_violation(format!("FFI callback returned unknown status {status}"))
        }
    }
}

unsafe fn validate_descriptor(
    descriptor: *const ModuleDescriptor,
) -> Result<ValidatedDescriptor, FfiPrototypeError> {
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
    let descriptors = if function_count == 0 {
        &[]
    } else {
        // SAFETY: a non-empty table is checked non-null immediately above.
        unsafe { std::slice::from_raw_parts(descriptor.functions, function_count) }
    };
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
    let resources = unsafe { validate_resources(descriptor.resources, descriptor.resource_count) }?;
    Ok((module_name, descriptor.destroy_module, functions, resources))
}

unsafe fn validate_resources(
    descriptors: *const ResourceDescriptor,
    declared_count: u64,
) -> Result<Vec<RegisteredResource>, FfiPrototypeError> {
    let resource_count = usize::try_from(declared_count)
        .map_err(|_| FfiPrototypeError::new("FFI module resource count exceeds host limits"))?;
    if resource_count > MAX_RESOURCES {
        return Err(FfiPrototypeError::new(
            "FFI module declares too many resource types",
        ));
    }
    if resource_count > 0 && descriptors.is_null() {
        return Err(FfiPrototypeError::new(
            "FFI module has a null resource table",
        ));
    }
    let descriptors = if resource_count == 0 {
        &[]
    } else {
        // SAFETY: a non-empty table is checked non-null immediately above.
        unsafe { std::slice::from_raw_parts(descriptors, resource_count) }
    };
    let mut resource_names = std::collections::HashSet::new();
    let mut resources = Vec::with_capacity(resource_count);
    for resource in descriptors {
        if resource.descriptor_size
            < u32::try_from(size_of::<ResourceDescriptor>()).expect("resource descriptor fits u32")
        {
            return Err(FfiPrototypeError::new(
                "FFI module has an undersized resource descriptor",
            ));
        }
        let name = unsafe { text_from_ffi(resource.name) }
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                FfiPrototypeError::new("FFI module has an invalid resource type name")
            })?;
        let destroy = resource.destroy_resource.ok_or_else(|| {
            FfiPrototypeError::new(format!(
                "FFI resource type `{name}` has no destroy callback"
            ))
        })?;
        if !resource_names.insert(name.clone()) {
            return Err(FfiPrototypeError::new(format!(
                "FFI module declares resource type `{name}` more than once"
            )));
        }
        resources.push(RegisteredResource { name, destroy });
    }
    Ok(resources)
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
