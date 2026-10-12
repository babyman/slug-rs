//! Platform dynamic-library ownership for native clutches: open a library,
//! look up its symbols, and close it with the final lease.

use std::{
    error::Error,
    ffi::{CStr, c_char, c_void},
    fmt,
    path::Path,
    sync::{Arc, LazyLock, Mutex},
};

#[cfg(unix)]
use std::ffi::CString;

// A C worker that received a producer may still be returning through library
// code after its last send, and no callback reports when it has finished, so
// such a library stays loaded for the process lifetime.
static FOREIGN_WORKER_LIBRARIES: LazyLock<Mutex<Vec<Arc<LoadedLibrary>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// A native library that cannot be loaded or lacks a symbol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeLibraryError {
    message: String,
}

impl NativeLibraryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NativeLibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for NativeLibraryError {}

pub(crate) struct LoadedLibrary(*mut c_void);

// SAFETY: a platform library handle may be used and closed from any thread.
unsafe impl Send for LoadedLibrary {}
// SAFETY: symbol lookup through a shared handle is thread-safe on every
// supported platform.
unsafe impl Sync for LoadedLibrary {}

impl LoadedLibrary {
    #[cfg(unix)]
    unsafe fn open(path: &Path) -> Result<Self, NativeLibraryError> {
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| NativeLibraryError::new("native library path contains a NUL byte"))?;
        // SAFETY: `path` is a NUL-terminated byte string that remains live for the call.
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(NativeLibraryError::new(format!(
                "cannot load native library: {}",
                unsafe { loader_error() }
            )));
        }
        Ok(Self(handle))
    }

    #[cfg(windows)]
    unsafe fn open(path: &Path) -> Result<Self, NativeLibraryError> {
        use std::os::windows::ffi::OsStrExt;

        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: `path` is a NUL-terminated UTF-16 string that remains live for the call.
        let handle = unsafe { LoadLibraryW(path.as_ptr()) };
        if handle.is_null() {
            return Err(NativeLibraryError::new(format!(
                "cannot load native library: {}",
                unsafe { loader_error() }
            )));
        }
        Ok(Self(handle))
    }

    /// Looks up `name` as a value of type `T`.
    ///
    /// # Safety
    ///
    /// The library must export `name` with exactly the type `T`, and the
    /// caller must keep this library loaded while it uses the result.
    pub(crate) unsafe fn symbol<T>(&self, name: &CStr) -> Result<T, NativeLibraryError>
    where
        T: Copy,
    {
        // SAFETY: `self.0` is an open library handle and `name` is NUL-terminated.
        let symbol = unsafe { lookup_symbol(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(NativeLibraryError::new(format!(
                "native library is missing `{}`: {}",
                name.to_string_lossy(),
                unsafe { loader_error() }
            )));
        }
        // SAFETY: the caller guarantees the symbol has type `T`.
        Ok(unsafe { std::mem::transmute_copy(&symbol) })
    }
}

impl fmt::Debug for LoadedLibrary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<loaded native library>")
    }
}

impl Drop for LoadedLibrary {
    fn drop(&mut self) {
        // SAFETY: this is the unique final lease for an open platform handle.
        unsafe { close_library(self.0) };
    }
}

/// Opens the library at `path` and returns a shared lease that closes it when
/// the last holder drops it.
pub(crate) fn library_lease(path: &Path) -> Result<Arc<LoadedLibrary>, NativeLibraryError> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        NativeLibraryError::new(format!("cannot resolve native library path: {error}"))
    })?;
    // SAFETY: opening a library runs its initializers; clutch libraries are
    // trusted code selected by the clutch manifest.
    Ok(Arc::new(unsafe { LoadedLibrary::open(&path) }?))
}

/// Keeps `library` loaded for the process lifetime.
pub(crate) fn retain_library_for_foreign_worker(library: Arc<LoadedLibrary>) {
    FOREIGN_WORKER_LIBRARIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(library);
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
    if error.is_null() {
        return "unknown dynamic loader error".into();
    }
    // SAFETY: a non-null `dlerror` result is a NUL-terminated string that
    // stays valid until the next loader call on this thread.
    unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned()
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
