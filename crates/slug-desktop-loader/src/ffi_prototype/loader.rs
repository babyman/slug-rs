//! Platform dynamic-library ownership for the experimental native ABI.
//!
//! Keeping this boundary separate makes the rest of the FFI bridge concerned
//! only with descriptor and call validation.

use std::{
    ffi::CStr,
    fmt,
    path::Path,
    sync::{Arc, LazyLock, Mutex},
};

#[cfg(unix)]
use std::ffi::CString;

#[cfg(windows)]
use super::LoadLibraryW;
#[cfg(unix)]
use super::RTLD_NOW;
#[cfg(unix)]
use super::dlopen;
use super::{FfiPrototypeError, c_void, close_library, loader_error, lookup_symbol};

// ABI 0.13 lets a C worker release its final producer capability from inside
// that library's own thread. Its return path can still execute library code
// after `producer_destroy`, so unloading at that point is unsafe. This ABI has
// no worker-quiescence callback; keep libraries that export a producer loaded
// for the process lifetime.
static FOREIGN_WORKER_LIBRARIES: LazyLock<Mutex<Vec<Arc<LoadedLibrary>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

pub(super) struct LoadedLibrary(*mut c_void);

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

    pub(super) unsafe fn symbol<T>(&self, name: &CStr) -> Result<T, FfiPrototypeError>
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

pub(super) fn library_lease(path: &Path) -> Result<Arc<LoadedLibrary>, FfiPrototypeError> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        FfiPrototypeError::new(format!("cannot resolve FFI module path: {error}"))
    })?;
    // SAFETY: platform loading is contained in this private prototype boundary.
    Ok(Arc::new(unsafe { LoadedLibrary::open(&path) }?))
}

pub(super) fn retain_library_for_foreign_worker(library: Arc<LoadedLibrary>) {
    FOREIGN_WORKER_LIBRARIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(library);
}
