//! Desktop activation for typed native clutches: load the shim library,
//! and install its table through the shared adapter.
//!
//! See `docs/decisions/2026-10-11-typed-native-manifest-bindings.md`.

use std::{any::Any, path::Path, rc::Rc};

use slug_native_adapter::{ExpectedBinding, Installed, abi::GetTable};
use slug_native_manifest::{Binding, NativeManifest};

use crate::ffi_prototype::loader::library_lease;

/// Loads a typed clutch's library and returns its Slug-visible functions.
/// The functions keep the library loaded.
pub(crate) fn load(library: &Path, manifest: &NativeManifest) -> Result<Installed, String> {
    let lease = library_lease(library).map_err(|error| error.to_string())?;
    // SAFETY: a dynamic shim exports `slug_shim_get_table` with this type.
    let get_table: GetTable =
        unsafe { lease.symbol(c"slug_shim_get_table") }.map_err(|error| error.to_string())?;
    // SAFETY: the table function takes no arguments and returns a static table.
    let table = unsafe { get_table() };
    let signatures = manifest
        .bindings()
        .map(Binding::canonical_signature)
        .collect::<Vec<_>>();
    let expected = manifest
        .bindings()
        .zip(&signatures)
        .map(|(binding, signature)| ExpectedBinding {
            module: &binding.module,
            name: &binding.name,
            signature,
        })
        .collect::<Vec<_>>();
    let owner: Rc<dyn Any> = Rc::new(lease);
    // SAFETY: the table belongs to the library `owner` keeps loaded, and a
    // generated shim's entries follow the contract for the signatures it lists.
    unsafe { slug_native_adapter::install(table, &expected, &owner) }
        .map_err(|error| error.to_string())
}
