//! Construction of the standard Slug host environment shared by executables.

use std::{
    env, fs, io,
    ops::Deref,
    path::{Path, PathBuf},
};

use crate::{
    ClutchRepository, ClutchRepositoryError, Configuration, ModuleLoader, ModuleRequest,
    ModuleResolver, ModuleSource, Vm,
};

/// Desktop host policy for entry programs and imported modules.
#[derive(Clone, Debug)]
pub struct DesktopLoader {
    module_loader: ModuleLoader,
}

impl DesktopLoader {
    #[must_use]
    pub fn new(module_loader: ModuleLoader) -> Self {
        Self { module_loader }
    }

    /// Reads an entry program by explicit path, module root, or installed library name.
    ///
    /// The library fallback accepts a bare name such as `hello` and reads
    /// `lib/hello.slug`; explicit paths retain their supplied extension.
    ///
    /// # Errors
    ///
    /// Returns the candidate path and its read failure when a matching entry
    /// cannot be read.
    pub fn load_entry(
        path: &str,
        source_root: Option<&Path>,
        library_root: Option<&Path>,
    ) -> Result<(PathBuf, String), (PathBuf, io::Error)> {
        let requested = Path::new(path);
        let mut candidates = vec![requested.to_path_buf()];
        if let Some(source_root) = source_root {
            let candidate = source_root.join(requested);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        if let Some(library_root) = library_root {
            let library_entry = if requested.extension().is_some() {
                requested.to_path_buf()
            } else {
                requested.with_extension("slug")
            };
            candidates.push(library_root.join(library_entry));
        }

        for candidate in candidates {
            match fs::read_to_string(&candidate) {
                Ok(source) => return Ok((candidate, source)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err((candidate, error)),
            }
        }
        Err((
            requested.to_path_buf(),
            io::Error::from(io::ErrorKind::NotFound),
        ))
    }
}

impl Deref for DesktopLoader {
    type Target = ModuleLoader;

    fn deref(&self) -> &Self::Target {
        &self.module_loader
    }
}

impl ModuleResolver for DesktopLoader {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, crate::ModuleLoadError> {
        self.module_loader.resolve(request)
    }
}

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
) -> Result<(Vm, DesktopLoader), ClutchRepositoryError> {
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
    let desktop_loader = DesktopLoader::new(loader);
    Ok((
        Vm::with_module_loader(desktop_loader.module_loader.clone()),
        desktop_loader,
    ))
}
