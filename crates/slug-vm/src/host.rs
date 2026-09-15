//! Construction of the standard Slug host environment shared by executables.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{ClutchRepository, ClutchRepositoryError, Configuration, ModuleLoader, Vm};

/// Resolves the bundled library root from the standard host environment.
#[must_use]
pub fn default_library_root(slug_home: Option<&Path>) -> Option<PathBuf> {
    env::var_os("SLUG_FIXTURE_LIBRARY_ROOT")
        .map(PathBuf::from)
        .or_else(|| slug_home.map(|home| home.join("lib")))
}

/// Builds the default Slug host VM, including module resolution and configuration.
///
/// The caller supplies its source root and entry-module identity; library and
/// clutch discovery deliberately follow the same environment contract as `slug`.
///
/// # Errors
///
/// Returns an error when an installed clutch manifest cannot be loaded.
pub fn build_default_host_vm(
    source_root: &Path,
    slug_home: Option<&Path>,
    program_arguments: &[String],
    entry_module: &str,
) -> Result<(Vm, ModuleLoader), ClutchRepositoryError> {
    let configuration = Configuration::load(
        source_root,
        slug_home,
        env::vars(),
        program_arguments,
        entry_module,
    );
    let clutches = match slug_home {
        Some(home) if home.join("clutch/manifest.toml").exists() => {
            ClutchRepository::from_manifest(home.join("clutch"))?
        }
        _ => ClutchRepository::default(),
    };
    let loader = ModuleLoader::with_configuration_and_clutch_repository(
        source_root.to_path_buf(),
        default_library_root(slug_home),
        configuration,
        clutches,
    );
    Ok((Vm::with_module_loader(loader.clone()), loader))
}
