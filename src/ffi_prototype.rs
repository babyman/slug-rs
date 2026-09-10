//! Unstable bridge for the experimental clutch native-module layout.
//!
//! This is deliberately not the version 1 native ABI. It contains the unsafe
//! dynamic-loader and raw-pointer work in one isolated module so the rest of
//! the runtime continues to prohibit unsafe code.

use std::{
    cell::Cell,
    cell::RefCell,
    collections::{HashMap, HashSet},
    error::Error,
    ffi::{CStr, c_char, c_void},
    fmt,
    mem::size_of,
    path::Path,
    rc::Rc,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[cfg(unix)]
use std::ffi::CString;

use crate::{
    ClutchPluginRegistrar, NativeArity, NativeCall, NativeDescriptorError, NativeError,
    NativeModule, NativeOwnedValue, NativeProducerStatus, NativeSendValue, NativeStatus, Vm,
};

const ABI_MAJOR: u32 = 0;
const ABI_MINOR: u32 = 13;
pub(crate) const ABI_PROFILE: &str = "slug-ffi-prototype/0.13";
const MAX_FUNCTIONS: usize = 64;
const MAX_RESOURCES: usize = 64;
static NEXT_LIBRARY_SCOPE: AtomicUsize = AtomicUsize::new(1);
static NEXT_VALUE_SCOPE: AtomicUsize = AtomicUsize::new(1);

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
    set_nil: unsafe extern "C" fn(*mut c_void),
    set_text: unsafe extern "C" fn(*mut c_void, FfiText) -> bool,
    argument_bytes: unsafe extern "C" fn(*mut c_void, usize, *mut FfiText) -> bool,
    argument_kind: unsafe extern "C" fn(*mut c_void, usize, *mut FfiValueKind) -> bool,
    list_create: unsafe extern "C" fn(*mut c_void, u64) -> *mut FfiList,
    list_destroy: unsafe extern "C" fn(*mut FfiList),
    map_create: unsafe extern "C" fn(*mut c_void, u64) -> *mut FfiMap,
    map_destroy: unsafe extern "C" fn(*mut FfiMap),
    map_set_nil: unsafe extern "C" fn(*mut c_void, *mut FfiMap, FfiText) -> bool,
    map_set_i64: unsafe extern "C" fn(*mut c_void, *mut FfiMap, FfiText, i64) -> bool,
    map_set_f64: unsafe extern "C" fn(*mut c_void, *mut FfiMap, FfiText, f64) -> bool,
    map_set_text: unsafe extern "C" fn(*mut c_void, *mut FfiMap, FfiText, FfiText) -> bool,
    map_set_bytes: unsafe extern "C" fn(*mut c_void, *mut FfiMap, FfiText, FfiText) -> bool,
    list_append_map: unsafe extern "C" fn(*mut c_void, *mut FfiList, *mut FfiMap) -> bool,
    set_list: unsafe extern "C" fn(*mut c_void, *mut FfiList) -> bool,
    argument_count: unsafe extern "C" fn(*mut c_void) -> u64,
    argument_value: unsafe extern "C" fn(*mut c_void, usize, *mut FfiValue) -> bool,
    value_map_length: unsafe extern "C" fn(*mut c_void, FfiValue, *mut u64) -> bool,
    value_map_entry:
        unsafe extern "C" fn(*mut c_void, FfiValue, u64, *mut FfiValue, *mut FfiValue) -> bool,
    list_append_value: unsafe extern "C" fn(*mut c_void, *mut FfiList, FfiValue) -> bool,
    producer_send_nil: unsafe extern "C" fn(*mut FfiProducer) -> i32,
    producer_send_bool: unsafe extern "C" fn(*mut FfiProducer, bool) -> i32,
    producer_send_f64: unsafe extern "C" fn(*mut FfiProducer, f64) -> i32,
    producer_send_bytes:
        unsafe extern "C" fn(*mut FfiProducer, FfiText, Option<ProducerTextDestroy>) -> i32,
    producer_close: unsafe extern "C" fn(*mut FfiProducer),
}

type Callback = unsafe extern "C" fn(*const HostApi, *mut c_void, *mut c_void) -> i32;
type LibraryDestroy = unsafe extern "C" fn(*mut c_void);
type ResourceDestroy = unsafe extern "C" fn(*mut c_void);
type ProducerTextDestroy = unsafe extern "C" fn(*mut c_void);
type LibraryInit =
    unsafe extern "C" fn(*const HostApi, *mut *mut c_void) -> *const LibraryDescriptor;

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
    functions: *const FunctionDescriptor,
    function_count: u64,
    resources: *const ResourceDescriptor,
    resource_count: u64,
}

#[repr(C)]
struct LibraryDescriptor {
    abi_major: u32,
    abi_minor: u32,
    descriptor_size: u32,
    destroy_library: Option<LibraryDestroy>,
    modules: *const ModuleDescriptor,
    module_count: u64,
}

struct CallBridge<'call> {
    call: *mut NativeCall<'call>,
    value_scope: u64,
    values: Vec<NativeOwnedValue>,
}

struct LoadedLibrary(*mut c_void);

unsafe impl Send for LoadedLibrary {}
unsafe impl Sync for LoadedLibrary {}

impl LoadedLibrary {
    #[cfg(unix)]
    unsafe fn open(path: &Path) -> Result<Self, FfiPrototypeError> {
        use std::os::unix::ffi::OsStrExt;

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

    #[cfg(windows)]
    unsafe fn open(path: &Path) -> Result<Self, FfiPrototypeError> {
        use std::os::windows::ffi::OsStrExt;

        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: `path` is a NUL-terminated UTF-16 string that remains live for the call.
        let handle = unsafe { LoadLibraryW(path.as_ptr()) };
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
        let symbol = unsafe { lookup_symbol(self.0, name.as_ptr()) };
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

impl Drop for LoadedLibrary {
    fn drop(&mut self) {
        // SAFETY: this is the unique final lease for an open platform handle.
        unsafe { close_library(self.0) };
    }
}

fn library_lease(path: &Path) -> Result<Arc<LoadedLibrary>, FfiPrototypeError> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        FfiPrototypeError::new(format!("cannot resolve FFI module path: {error}"))
    })?;
    // SAFETY: platform loading is contained in this private prototype module.
    Ok(Arc::new(unsafe { LoadedLibrary::open(&path) }?))
}

#[derive(Clone, Debug)]
struct RegisteredFunction {
    name: String,
    arity: NativeArity,
    callback: Callback,
}

type ValidatedDescriptor = (
    String,
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

#[repr(C)]
enum FfiValueKind {
    Nil = 0,
    Int = 1,
    Float = 2,
    Text = 3,
    Bytes = 4,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfiValue {
    scope: u64,
    token: u64,
}

struct FfiList {
    values: Vec<NativeOwnedValue>,
}
struct FfiMap {
    entries: Vec<(NativeOwnedValue, NativeOwnedValue)>,
}

// C receives only opaque pointers for these call-scoped builders. Track every
// allocation before exposing it so a stale, double-freed, or cross-kind handle
// is rejected before Rust dereferences or deallocates it.
static FFI_LIST_HANDLES: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static FFI_MAP_HANDLES: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn register_handle<T>(handles: &Mutex<HashSet<usize>>, handle: *mut T) {
    handles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(handle.cast::<()>() as usize);
}

fn has_handle<T>(handles: &Mutex<HashSet<usize>>, handle: *mut T) -> bool {
    !handle.is_null()
        && handles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&(handle.cast::<()>() as usize))
}

fn take_handle<T>(handles: &Mutex<HashSet<usize>>, handle: *mut T) -> bool {
    !handle.is_null()
        && handles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(handle.cast::<()>() as usize))
}

#[derive(Clone)]
struct CResourceType {
    resource_type: crate::NativeResourceType<CResource>,
    destroy: ResourceDestroy,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FfiResourceIdentity {
    module_name: String,
    resource_name: String,
}

impl FfiResourceIdentity {
    fn declared(module_name: String, resource_name: String) -> Self {
        Self {
            module_name,
            resource_name,
        }
    }

    fn from_ffi(value: FfiText) -> Option<Self> {
        let value = unsafe { text_from_ffi(value) }?;
        let (module_name, resource_name) = value.rsplit_once('.')?;
        if module_name.trim().is_empty() || resource_name.trim().is_empty() {
            return None;
        }
        Some(Self::declared(module_name.into(), resource_name.into()))
    }
}

impl fmt::Display for FfiResourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.module_name, self.resource_name)
    }
}

struct FfiModuleState {
    library: Rc<FfiLibraryState>,
    functions: HashMap<String, RegisteredFunction>,
    resources: Rc<RefCell<HashMap<FfiResourceIdentity, CResourceType>>>,
}

struct FfiLibraryState {
    library: RefCell<Option<Arc<LoadedLibrary>>>,
    library_state: Cell<*mut c_void>,
    destroy_library: Option<LibraryDestroy>,
    active: Cell<bool>,
}

impl FfiLibraryState {
    fn shutdown(&self) {
        if !self.active.replace(false) {
            return;
        }
        let library_state = self.library_state.replace(std::ptr::null_mut());
        if !library_state.is_null()
            && let Some(destroy_library) = self.destroy_library
        {
            // SAFETY: the library owns this state and its library lease remains live.
            unsafe { destroy_library(library_state) };
        }
        self.library.borrow_mut().take();
    }
}

impl Drop for FfiLibraryState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A loaded, deliberately unstable Slug-aware C library.
type RegisteredModule = (NativeModule, Vec<(String, NativeArity, String)>);

#[derive(Clone)]
pub struct FfiPrototypeLibrary {
    modules: Vec<RegisteredModule>,
    library: Rc<FfiLibraryState>,
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

impl FfiPrototypeLibrary {
    /// Loads and validates one C library that follows the prototype header.
    ///
    /// The library is held by this module's lease and is unloaded after its
    /// clutch has deterministically finalized all native state.
    ///
    /// # Errors
    ///
    /// Returns an error when the dynamic library cannot load, lacks the entry
    /// symbol, or provides a malformed or incompatible descriptor.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FfiPrototypeError> {
        // SAFETY: all dynamic-loader interactions and C descriptor reads are
        // validated within this private prototype boundary.
        unsafe {
            let library = library_lease(path.as_ref())?;
            let init_name = c"slug_ffi_library_init";
            let init: LibraryInit = library.symbol(init_name)?;
            let mut library_state = std::ptr::null_mut();
            let descriptors =
                validate_library_descriptor(init(host_api(), &raw mut library_state))?;
            if !library_state.is_null() && descriptors.0.is_none() {
                return Err(FfiPrototypeError::new(
                    "FFI library returned state without a destroy callback",
                ));
            }
            let resource_types = Rc::new(RefCell::new(HashMap::new()));
            let library_state = Rc::new(FfiLibraryState {
                library: RefCell::new(Some(library)),
                library_state: Cell::new(library_state),
                destroy_library: descriptors.0,
                active: Cell::new(true),
            });
            let scope = NEXT_LIBRARY_SCOPE.fetch_add(1, Ordering::Relaxed);
            let mut modules = Vec::new();
            for (module_name, functions, resources) in descriptors.1 {
                let registered = functions
                    .iter()
                    .map(|(key, function)| (function.name.clone(), function.arity, key.clone()))
                    .collect();
                let module = NativeModule::new_with_scope(
                    module_name.clone(),
                    FfiModuleState {
                        library: library_state.clone(),
                        functions,
                        resources: resource_types.clone(),
                    },
                    scope,
                )
                .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
                let resource_types_to_register = resources
                    .into_iter()
                    .map(|resource| {
                        let resource_type = module
                            .resource_type(
                                resource.name.clone(),
                                close_c_resource,
                                destroy_c_resource,
                            )
                            .map_err(|error| FfiPrototypeError::new(error.to_string()))?;
                        Ok((
                            FfiResourceIdentity::declared(module_name.clone(), resource.name),
                            resource_type,
                            resource.destroy,
                        ))
                    })
                    .collect::<Result<Vec<_>, FfiPrototypeError>>()?;
                resource_types
                    .borrow_mut()
                    .extend(resource_types_to_register.into_iter().map(
                        |(identity, resource_type, destroy)| {
                            (
                                identity,
                                CResourceType {
                                    resource_type,
                                    destroy,
                                },
                            )
                        },
                    ));
                modules.push((module, registered));
            }
            Ok(Self {
                modules,
                library: library_state,
            })
        }
    }

    /// Installs the module's descriptors into a loader-backed VM.
    ///
    /// # Errors
    ///
    /// Returns an error when the VM cannot register a descriptor.
    pub fn register(&self, vm: &mut Vm) -> Result<(), NativeDescriptorError> {
        vm.define_foreign_batch(self.foreign_functions()?)
    }

    /// Stages this module's descriptors under a clutch-owned registration scope.
    ///
    /// # Errors
    ///
    /// Returns an error when the scope does not match the module or a native
    /// descriptor is invalid.
    pub fn stage(
        &self,
        registrar: &mut ClutchPluginRegistrar,
    ) -> Result<(), NativeDescriptorError> {
        for function in self.foreign_functions()? {
            registrar.define_foreign(function)?;
        }
        Ok(())
    }

    /// Finalizes module state and releases this module's dynamic-library lease.
    pub fn shutdown(&self) {
        self.library.shutdown();
    }

    fn foreign_functions(&self) -> Result<Vec<crate::NativeFunction>, NativeDescriptorError> {
        let functions = self
            .modules
            .iter()
            .flat_map(|(module, descriptors)| {
                descriptors.iter().map(move |(name, arity, member_key)| {
                    module.function_with_member_key(
                        name.clone(),
                        *arity,
                        member_key.clone(),
                        ffi_callback,
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(functions)
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
    set_nil,
    set_text,
    argument_bytes,
    argument_kind,
    list_create,
    list_destroy,
    map_create,
    map_destroy,
    map_set_nil,
    map_set_i64,
    map_set_f64,
    map_set_text,
    map_set_bytes,
    list_append_map,
    set_list,
    argument_count,
    argument_value,
    value_map_length,
    value_map_entry,
    list_append_value,
    producer_send_nil,
    producer_send_bool,
    producer_send_f64,
    producer_send_bytes,
    producer_close,
});

fn host_api() -> *const HostApi {
    &raw const *HOST_API
}

unsafe extern "C" fn argument_count(context: *mut c_void) -> u64 {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return 0;
    };
    u64::try_from(call.argument_count()).unwrap_or(u64::MAX)
}

fn bridge_value<'bridge>(
    bridge: &'bridge CallBridge<'_>,
    value: FfiValue,
) -> Option<&'bridge NativeOwnedValue> {
    if value.scope != bridge.value_scope {
        return None;
    }
    let index = usize::try_from(value.token.checked_sub(1)?).ok()?;
    bridge.values.get(index)
}

fn bridge_store_value(bridge: &mut CallBridge<'_>, value: NativeOwnedValue) -> Option<FfiValue> {
    bridge.values.push(value);
    u64::try_from(bridge.values.len())
        .ok()
        .map(|token| FfiValue {
            scope: bridge.value_scope,
            token,
        })
}

unsafe fn bridge_from_context<'call>(context: *mut c_void) -> Option<&'call mut CallBridge<'call>> {
    unsafe { context.cast::<CallBridge<'call>>().as_mut() }
}

unsafe extern "C" fn argument_value(
    context: *mut c_void,
    index: usize,
    output: *mut FfiValue,
) -> bool {
    let Some(bridge) = (unsafe { bridge_from_context(context) }) else {
        return false;
    };
    let Some(call) = (unsafe { bridge.call.as_mut() }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value output is null",
        ));
        return false;
    }
    let value = match call.argument(index) {
        Ok(value) => value.to_owned(),
        Err(error) => {
            call.set_error(error);
            return false;
        }
    };
    let Some(value) = bridge_store_value(bridge, value) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value table is too large",
        ));
        return false;
    };
    unsafe { *output = value };
    true
}

unsafe extern "C" fn value_map_length(
    context: *mut c_void,
    value: FfiValue,
    output: *mut u64,
) -> bool {
    let Some(bridge) = (unsafe { bridge_from_context(context) }) else {
        return false;
    };
    let Some(call) = (unsafe { bridge.call.as_mut() }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map-length output is null",
        ));
        return false;
    }
    let Some(value) = bridge_value(bridge, value) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value handle is invalid",
        ));
        return false;
    };
    if value.as_ref().kind() != crate::NativeValueKind::Map {
        call.set_error(NativeError::new("native.type", "expected map"));
        return false;
    }
    let Ok(length) = u64::try_from(value.as_ref().len().expect("map has a length")) else {
        call.set_error(NativeError::new(
            "native.contract",
            "map length exceeds FFI range",
        ));
        return false;
    };
    unsafe { *output = length };
    true
}

unsafe extern "C" fn value_map_entry(
    context: *mut c_void,
    value: FfiValue,
    index: u64,
    key_output: *mut FfiValue,
    entry_value: *mut FfiValue,
) -> bool {
    let Some(bridge) = (unsafe { bridge_from_context(context) }) else {
        return false;
    };
    let Some(call) = (unsafe { bridge.call.as_mut() }) else {
        return false;
    };
    if key_output.is_null() || entry_value.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map-entry output is null",
        ));
        return false;
    }
    let Ok(index) = usize::try_from(index) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map-entry index is too large",
        ));
        return false;
    };
    let Some(value) = bridge_value(bridge, value) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value handle is invalid",
        ));
        return false;
    };
    let (key, mapped) = match value.as_ref().map_get(index) {
        Ok(Some((key, mapped))) => (key.to_owned(), mapped.to_owned()),
        Ok(None) => {
            call.set_error(NativeError::new(
                "native.range",
                "FFI map-entry index is out of bounds",
            ));
            return false;
        }
        Err(error) => {
            call.set_error(error);
            return false;
        }
    };
    let Some(key_value) = bridge_store_value(bridge, key) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value table is too large",
        ));
        return false;
    };
    let Some(mapped_value) = bridge_store_value(bridge, mapped) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value table is too large",
        ));
        return false;
    };
    unsafe {
        *key_output = key_value;
        *entry_value = mapped_value;
    }
    true
}

unsafe extern "C" fn list_append_value(
    context: *mut c_void,
    list: *mut FfiList,
    value: FfiValue,
) -> bool {
    let Some(bridge) = (unsafe { bridge_from_context(context) }) else {
        return false;
    };
    let Some(call) = (unsafe { bridge.call.as_mut() }) else {
        return false;
    };
    if !has_handle(&FFI_LIST_HANDLES, list) {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list handle is invalid",
        ));
        return false;
    }
    let Some(value) = bridge_value(bridge, value).cloned() else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value handle is invalid",
        ));
        return false;
    };
    let Some(list) = (unsafe { list.as_mut() }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list handle is null",
        ));
        return false;
    };
    list.values.push(value);
    true
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

unsafe extern "C" fn argument_bytes(
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
            "FFI bytes output is null",
        ));
        return false;
    }
    match call
        .argument(index)
        .and_then(crate::NativeValueRef::as_bytes)
    {
        Ok(value) => {
            if let Ok(length) = u64::try_from(value.len()) {
                unsafe {
                    *output = FfiText {
                        data: value.as_ptr().cast(),
                        length,
                    };
                }
                true
            } else {
                call.set_error(NativeError::new(
                    "native.contract",
                    "FFI bytes length is too large",
                ));
                false
            }
        }
        Err(error) => {
            call.set_error(error);
            false
        }
    }
}

unsafe extern "C" fn argument_kind(
    context: *mut c_void,
    index: usize,
    output: *mut FfiValueKind,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if output.is_null() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI value-kind output is null",
        ));
        return false;
    }
    let kind = match call.argument(index).map(crate::NativeValueRef::kind) {
        Ok(crate::NativeValueKind::Nil) => FfiValueKind::Nil,
        Ok(crate::NativeValueKind::Int) => FfiValueKind::Int,
        Ok(crate::NativeValueKind::Float) => FfiValueKind::Float,
        Ok(crate::NativeValueKind::String) => FfiValueKind::Text,
        Ok(crate::NativeValueKind::Bytes) => FfiValueKind::Bytes,
        Ok(_) => {
            call.set_error(NativeError::new(
                "native.type",
                "expected nil, num, str, or bytes",
            ));
            return false;
        }
        Err(error) => {
            call.set_error(error);
            return false;
        }
    };
    unsafe {
        *output = kind;
    }
    true
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

unsafe extern "C" fn set_nil(context: *mut c_void) {
    if let Some(call) = unsafe { call_from_context(context) } {
        call.set_result(NativeOwnedValue::nil());
    }
}

unsafe extern "C" fn set_text(context: *mut c_void, value: FfiText) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    let Some(value) = (unsafe { text_from_ffi(value) }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI module returned invalid text",
        ));
        return false;
    };
    call.set_result(NativeOwnedValue::string(value));
    true
}

unsafe extern "C" fn list_create(context: *mut c_void, capacity: u64) -> *mut FfiList {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return std::ptr::null_mut();
    };
    let Ok(capacity) = usize::try_from(capacity) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list capacity is too large",
        ));
        return std::ptr::null_mut();
    };
    let mut values = Vec::new();
    if values.try_reserve_exact(capacity).is_err() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list capacity cannot be allocated",
        ));
        return std::ptr::null_mut();
    }
    let list = Box::into_raw(Box::new(FfiList { values }));
    register_handle(&FFI_LIST_HANDLES, list);
    list
}

unsafe extern "C" fn list_destroy(list: *mut FfiList) {
    if take_handle(&FFI_LIST_HANDLES, list) {
        drop(unsafe { Box::from_raw(list) });
    }
}

unsafe extern "C" fn map_create(context: *mut c_void, capacity: u64) -> *mut FfiMap {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return std::ptr::null_mut();
    };
    let Ok(capacity) = usize::try_from(capacity) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map capacity is too large",
        ));
        return std::ptr::null_mut();
    };
    let mut entries = Vec::new();
    if entries.try_reserve_exact(capacity).is_err() {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map capacity cannot be allocated",
        ));
        return std::ptr::null_mut();
    }
    let map = Box::into_raw(Box::new(FfiMap { entries }));
    register_handle(&FFI_MAP_HANDLES, map);
    map
}

unsafe extern "C" fn map_destroy(map: *mut FfiMap) {
    if take_handle(&FFI_MAP_HANDLES, map) {
        drop(unsafe { Box::from_raw(map) });
    }
}

fn map_set(
    call: &mut NativeCall<'_>,
    map: *mut FfiMap,
    key: FfiText,
    value: NativeOwnedValue,
) -> bool {
    if !has_handle(&FFI_MAP_HANDLES, map) {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map handle is invalid",
        ));
        return false;
    }
    let Some(map) = (unsafe { map.as_mut() }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map handle is null",
        ));
        return false;
    };
    let Some(key) = (unsafe { text_from_ffi(key) }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map key is invalid",
        ));
        return false;
    };
    map.entries.push((NativeOwnedValue::string(key), value));
    true
}

unsafe extern "C" fn map_set_nil(context: *mut c_void, map: *mut FfiMap, key: FfiText) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    map_set(call, map, key, NativeOwnedValue::nil())
}
unsafe extern "C" fn map_set_i64(
    context: *mut c_void,
    map: *mut FfiMap,
    key: FfiText,
    value: i64,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    map_set(call, map, key, NativeOwnedValue::integer(value))
}
unsafe extern "C" fn map_set_f64(
    context: *mut c_void,
    map: *mut FfiMap,
    key: FfiText,
    value: f64,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    map_set(call, map, key, NativeOwnedValue::float(value))
}
unsafe extern "C" fn map_set_text(
    context: *mut c_void,
    map: *mut FfiMap,
    key: FfiText,
    value: FfiText,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    let Some(value) = (unsafe { text_from_ffi(value) }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map text is invalid",
        ));
        return false;
    };
    map_set(call, map, key, NativeOwnedValue::string(value))
}
unsafe extern "C" fn map_set_bytes(
    context: *mut c_void,
    map: *mut FfiMap,
    key: FfiText,
    value: FfiText,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if value.data.is_null() && value.length != 0 {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map bytes are invalid",
        ));
        return false;
    }
    let Ok(length) = usize::try_from(value.length) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map bytes are too large",
        ));
        return false;
    };
    let bytes = if length == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(value.data.cast(), length).to_vec() }
    };
    map_set(call, map, key, NativeOwnedValue::bytes(bytes))
}
unsafe extern "C" fn list_append_map(
    context: *mut c_void,
    list: *mut FfiList,
    map: *mut FfiMap,
) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if !has_handle(&FFI_LIST_HANDLES, list) {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list handle is invalid",
        ));
        return false;
    }
    if !has_handle(&FFI_MAP_HANDLES, map) {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map handle is invalid",
        ));
        return false;
    }
    let Some(list) = (unsafe { list.as_mut() }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list handle is null",
        ));
        return false;
    };
    let Some(map) = (unsafe { map.as_mut() }) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI map handle is null",
        ));
        return false;
    };
    let entries = std::mem::take(&mut map.entries);
    list.values.push(NativeOwnedValue::map(entries));
    if take_handle(&FFI_MAP_HANDLES, map) {
        drop(unsafe { Box::from_raw(map) });
    }
    true
}
unsafe extern "C" fn set_list(context: *mut c_void, list: *mut FfiList) -> bool {
    let Some(call) = (unsafe { call_from_context(context) }) else {
        return false;
    };
    if !take_handle(&FFI_LIST_HANDLES, list) {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI list handle is invalid",
        ));
        return false;
    }
    let list = unsafe { Box::from_raw(list) };
    call.set_result(NativeOwnedValue::list(list.values));
    true
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
    unsafe { producer_send_value(producer, NativeSendValue::integer(value)) }
}

unsafe extern "C" fn producer_send_nil(producer: *mut FfiProducer) -> i32 {
    unsafe { producer_send_value(producer, NativeSendValue::nil()) }
}

unsafe extern "C" fn producer_send_bool(producer: *mut FfiProducer, value: bool) -> i32 {
    unsafe { producer_send_value(producer, NativeSendValue::boolean(value)) }
}

unsafe extern "C" fn producer_send_f64(producer: *mut FfiProducer, value: f64) -> i32 {
    unsafe { producer_send_value(producer, NativeSendValue::float(value)) }
}

unsafe fn producer_send_value(producer: *mut FfiProducer, value: NativeSendValue) -> i32 {
    let Some(producer) = (unsafe { producer.as_ref() }) else {
        return 3;
    };
    match producer.producer.try_send(value) {
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

unsafe extern "C" fn producer_close(producer: *mut FfiProducer) {
    if let Some(producer) = unsafe { producer.as_ref() } {
        producer.producer.close();
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

unsafe extern "C" fn producer_send_bytes(
    producer: *mut FfiProducer,
    bytes: FfiText,
    destroy: Option<ProducerTextDestroy>,
) -> i32 {
    let (Some(producer), Some(destroy)) = (unsafe { producer.as_ref() }, destroy) else {
        return 3;
    };
    let data = bytes.data;
    if bytes.length > 0 && data.is_null() {
        return 3;
    }
    let Ok(length) = usize::try_from(bytes.length) else {
        return 3;
    };
    let bytes = if length == 0 {
        Vec::new()
    } else {
        // SAFETY: the C caller keeps this length-delimited buffer valid for the call.
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec()
    };
    match producer.producer.try_send(NativeSendValue::bytes(bytes)) {
        NativeProducerStatus::Sent => {
            // SAFETY: C transfers ownership of the buffer only after a successful send.
            unsafe { destroy(data.cast_mut().cast()) };
            0
        }
        NativeProducerStatus::Full(_) => 1,
        NativeProducerStatus::Closed(_) => 2,
    }
}

fn resource_type(call: &mut NativeCall<'_>, resource_identity: FfiText) -> Option<CResourceType> {
    let Some(identity) = FfiResourceIdentity::from_ffi(resource_identity) else {
        call.set_error(NativeError::new(
            "native.contract",
            "FFI resource identity must be `module_name.resource_name`",
        ));
        return None;
    };
    let active = call
        .state::<FfiModuleState>()
        .is_some_and(|state| state.library.active.get());
    if !active {
        call.set_error(NativeError::new(
            "native.plugin_inactive",
            "native plugin is no longer active",
        ));
        return None;
    }
    let resource_type = call
        .state::<FfiModuleState>()
        .and_then(|state| state.resources.borrow().get(&identity).cloned());
    if resource_type.is_none() {
        call.set_error(NativeError::new(
            "native.contract",
            format!("FFI library has no resource type `{identity}`"),
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

#[cfg(unix)]
unsafe fn c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value).to_str().ok().map(str::to_owned) }
}

fn ffi_callback(call: &mut NativeCall<'_>) -> NativeStatus {
    let function = call.member_key().and_then(|member_key| {
        call.state::<FfiModuleState>().and_then(|state| {
            if !state.library.active.get() {
                return None;
            }
            state
                .functions
                .get(member_key)
                .cloned()
                .map(|function| (function, state.library.library_state.get()))
        })
    });
    let Some(function) = function else {
        return call.raise(NativeError::new(
            "native.plugin_inactive",
            "native plugin is no longer active",
        ));
    };
    let mut bridge = CallBridge {
        call,
        value_scope: u64::try_from(NEXT_VALUE_SCOPE.fetch_add(1, Ordering::Relaxed))
            .unwrap_or(u64::MAX),
        values: Vec::new(),
    };
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

unsafe fn validate_library_descriptor(
    descriptor: *const LibraryDescriptor,
) -> Result<(Option<LibraryDestroy>, Vec<ValidatedDescriptor>), FfiPrototypeError> {
    let descriptor = unsafe { descriptor.as_ref() }
        .ok_or_else(|| FfiPrototypeError::new("FFI library returned a null descriptor"))?;
    if descriptor.abi_major != ABI_MAJOR {
        return Err(FfiPrototypeError::new(format!(
            "FFI library requires ABI major {}, host supports {ABI_MAJOR}",
            descriptor.abi_major
        )));
    }
    if descriptor.abi_minor != ABI_MINOR
        || descriptor.descriptor_size
            < u32::try_from(size_of::<LibraryDescriptor>()).expect("descriptor fits u32")
    {
        return Err(FfiPrototypeError::new(
            "FFI library requires an unsupported ABI table",
        ));
    }
    let count = usize::try_from(descriptor.module_count)
        .map_err(|_| FfiPrototypeError::new("FFI library module count exceeds host limits"))?;
    if count == 0 || count > MAX_FUNCTIONS || descriptor.modules.is_null() {
        return Err(FfiPrototypeError::new(
            "FFI library has an invalid module table",
        ));
    }
    let modules = unsafe { std::slice::from_raw_parts(descriptor.modules, count) }
        .iter()
        .map(|module| unsafe { validate_descriptor(module) })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((descriptor.destroy_library, modules))
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
    if descriptor.abi_minor != ABI_MINOR
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
        if maximum_arity < minimum_arity {
            return Err(FfiPrototypeError::new(format!(
                "FFI prototype function `{name}` has an invalid arity range"
            )));
        }
        let member_key = unsafe { text_from_ffi(function.member_key) }
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| {
                FfiPrototypeError::new(format!("FFI function `{name}` has an invalid member key"))
            })?;
        let arity = if function.maximum_arity == u64::MAX {
            NativeArity::Variadic {
                minimum: minimum_arity,
            }
        } else if minimum_arity == maximum_arity {
            NativeArity::Exact(minimum_arity)
        } else {
            NativeArity::Range {
                minimum: minimum_arity,
                maximum: maximum_arity,
            }
        };
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
    Ok((module_name, functions, resources))
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

#[cfg(unix)]
const RTLD_NOW: i32 = 2;

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dlclose(handle: *mut c_void) -> i32;
}

#[cfg(target_os = "linux")]
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dlclose(handle: *mut c_void) -> i32;
}

#[cfg(unix)]
unsafe fn lookup_symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { dlsym(handle, name) }
}

#[cfg(windows)]
unsafe fn lookup_symbol(handle: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { GetProcAddress(handle, name.cast()) }
}

#[cfg(unix)]
unsafe fn loader_error() -> String {
    let error = unsafe { dlerror() };
    unsafe { c_string(error) }.unwrap_or_else(|| "unknown dynamic loader error".into())
}

#[cfg(unix)]
unsafe fn close_library(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: `handle` is an open library handle owned by this final lease.
        let _ = unsafe { dlclose(handle) };
    }
}

#[cfg(windows)]
unsafe fn loader_error() -> String {
    let error = unsafe { GetLastError() };
    format!("Windows error {error}")
}

#[cfg(windows)]
unsafe fn close_library(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: `handle` is an open library handle owned by this final lease.
        let _ = unsafe { FreeLibrary(handle) };
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn GetProcAddress(handle: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn FreeLibrary(handle: *mut c_void) -> i32;
}
