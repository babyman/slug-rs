//! Construction of the standard Slug host environment shared by executables.

use std::{
    cell::RefCell,
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use slug_desktop_loader::{
    ClutchRepository, ClutchRepositoryError, DesktopActivation, DesktopResolver,
};
use slug_frontend::{
    InteractiveCompilation, InteractiveCompilerState, ModuleGraph, ModuleGraphHost, ModuleInstance,
    SourceError,
};
use slug_loader::{ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource};
use slug_vm::{
    NativeDescriptorError, NativeFunction, NativeResourceRegistry, Program, Value, Vm,
    VmConfiguration, VmHost, VmHostError, VmModuleExports, native_resource_registry,
};

use crate::Configuration;

/// Desktop host policy for entry programs and imported modules.
#[derive(Clone)]
pub struct DesktopLoader {
    state: Rc<DesktopLoaderState>,
}

struct DesktopLoaderState {
    resolver: DesktopResolver,
    graph: ModuleGraph,
    configuration: Configuration,
    native_globals: RefCell<HashMap<String, Value>>,
    native_resources: NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    shutdown_errors: RefCell<Vec<String>>,
}

impl DesktopLoader {
    /// Creates a desktop host with default configuration and no Clutch repository.
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self::with_configuration(source_root, library_root, Configuration::default())
    }

    /// Creates a desktop host with the configuration shared by all modules.
    #[must_use]
    pub fn with_configuration(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        configuration: Configuration,
    ) -> Self {
        Self::with_configuration_and_clutch_repository(
            source_root,
            library_root,
            configuration,
            ClutchRepository::default(),
        )
    }

    /// Creates a desktop host with an explicit Clutch repository.
    #[must_use]
    pub fn with_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        clutches: ClutchRepository,
    ) -> Self {
        Self::with_configuration_and_clutch_repository(
            source_root,
            library_root,
            Configuration::default(),
            clutches,
        )
    }

    /// Creates a desktop host with caller-supplied configuration and Clutches.
    #[must_use]
    pub fn with_configuration_and_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        configuration: Configuration,
        clutches: ClutchRepository,
    ) -> Self {
        Self::with_resolver(
            DesktopResolver::with_clutch_repository(source_root, library_root, clutches),
            configuration,
        )
    }

    /// Creates a desktop host from explicitly assembled resolution and VM policy.
    #[must_use]
    pub fn with_resolver(resolver: DesktopResolver, configuration: Configuration) -> Self {
        Self {
            state: Rc::new(DesktopLoaderState {
                resolver,
                graph: ModuleGraph::new(),
                configuration,
                native_globals: RefCell::new(HashMap::new()),
                native_resources: native_resource_registry(),
                warnings: RefCell::new(Vec::new()),
                shutdown_errors: RefCell::new(Vec::new()),
            }),
        }
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

    /// Compiles entry source while resolving imports through this desktop host.
    ///
    /// # Errors
    ///
    /// Returns checked source errors.
    pub fn compile_source(
        &self,
        path: &str,
        source: &str,
    ) -> Result<slug_vm::Program, SourceError> {
        self.state.graph.compile_source(self, path, source)
    }

    /// Resolves one import without initializing it.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested module cannot be resolved or read.
    pub fn load(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleSource, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.resolve(ModuleRequest::new(importer.as_ref(), name))
    }

    /// Resolves and compiles one module, caching its program for repeat requests.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested module cannot be resolved, read, or
    /// compiled.
    pub fn compile(&self, importer: Option<&Path>, name: &str) -> Result<Program, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.state
            .graph
            .compile(self, ModuleRequest::new(importer.as_ref(), name))
    }

    /// Returns the number of compiled modules retained by this host graph.
    #[must_use]
    pub fn cached_module_count(&self) -> usize {
        self.state.graph.cached_module_count()
    }

    /// Initializes one import through this desktop host.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested module cannot be resolved, loaded,
    /// compiled, or initialized.
    pub fn initialize(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleInstance, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.state
            .graph
            .initialize(self, ModuleRequest::new(importer.as_ref(), name))
    }

    /// Returns the number of initialized modules retained by this host graph.
    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.graph.initialized_module_count()
    }

    #[doc(hidden)]
    pub fn compile_interactive_forms(
        &self,
        path: &str,
        source: &str,
        state: &InteractiveCompilerState,
    ) -> Result<Vec<InteractiveCompilation>, SourceError> {
        self.state
            .graph
            .compile_interactive_forms(self, path, source, state)
    }

    /// Returns and clears warnings accumulated during evaluation.
    #[must_use]
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.warnings.borrow_mut())
    }

    fn shutdown(&self) {
        self.state
            .shutdown_errors
            .borrow_mut()
            .extend(self.state.native_resources.finalize_all_for_shutdown());
        self.state.resolver.shutdown();
    }
}

impl ModuleResolver for DesktopLoader {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.state.resolver.resolve(request)
    }
}

impl ModuleGraphHost for DesktopLoader {
    type Activation = DesktopActivation;

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.state.resolver.builtin_globals()
    }

    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError> {
        self.state.resolver.stage_module_activation(source)
    }

    fn register_module_activation(
        &self,
        source: &ModuleSource,
        activation: &Self::Activation,
    ) -> Result<(), ModuleLoadError> {
        self.state
            .resolver
            .register_module_activation(source, activation)
    }

    fn remove_module_activation(&self, activation: &Self::Activation) {
        self.state.resolver.remove_module_activation(activation);
    }

    fn cleanup_module_activation(&self, activation: &mut Option<Self::Activation>) {
        self.state.resolver.cleanup_module_activation(activation);
    }

    fn retain_module_activation(&self, source: &ModuleSource, activation: Self::Activation) {
        self.state
            .resolver
            .retain_module_activation(source, activation);
    }

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: Rc<dyn VmHost> = Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for DesktopLoader {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let importer = importer.map(ModuleKey::new);
        let instance = self
            .state
            .graph
            .initialize(self, ModuleRequest::new(importer.as_ref(), name))
            .map_err(|error| match error {
                ModuleLoadError::NotFound { .. } => VmHostError::not_found(error.to_string()),
                error => VmHostError::new(error.to_string()),
            })?;
        Ok(VmModuleExports {
            exports: instance.live_exports(),
        })
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        ModuleGraphHost::builtin_globals(self)
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        self.state.native_globals.borrow().clone()
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.state.resolver.foreign_function(module, name)
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        self.state.resolver.define_foreign_batch(functions)
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.state.native_globals.borrow_mut().insert(name, value);
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.state.native_resources.clone()
    }

    fn configuration(&self) -> &dyn VmConfiguration {
        &self.state.configuration
    }

    fn warn(&self, message: String) {
        self.state.warnings.borrow_mut().push(message);
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}

impl Drop for DesktopLoaderState {
    fn drop(&mut self) {
        self.shutdown_errors
            .get_mut()
            .extend(self.native_resources.finalize_all_for_shutdown());
        self.resolver.shutdown();
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
    let loader = DesktopLoader::with_resolver(
        DesktopResolver::with_clutch_repository(
            source_root.to_path_buf(),
            default_library_root(slug_home),
            clutches,
        ),
        configuration,
    );
    Ok((Vm::with_host(Rc::new(loader.clone())), loader))
}
