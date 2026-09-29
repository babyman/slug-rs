//! Construction of the standard Slug host environment shared by executables.

use std::{
    cell::RefCell,
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use slug_desktop_loader::{
    ClutchRepository, ClutchRepositoryError, DesktopResolver, clutch::StagedClutchPlugin,
};
use slug_frontend::{
    InteractiveCompilation, InteractiveCompilerState, ModuleGraph, ModuleGraphHost, SourceError,
};
use slug_loader::{
    ModuleActivation, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
};
use slug_vm::{
    Configuration, NativeDescriptorError, NativeFunction, NativeResourceRegistry, Value, Vm,
    VmHost, VmHostError, VmModuleExports, native_resource_registry,
};

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
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    native_resources: NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    shutdown_errors: RefCell<Vec<String>>,
    active_plugins: RefCell<HashMap<ModuleActivation, StagedClutchPlugin>>,
}

impl DesktopLoader {
    #[must_use]
    pub fn new(resolver: DesktopResolver, configuration: Configuration) -> Self {
        Self {
            state: Rc::new(DesktopLoaderState {
                resolver,
                graph: ModuleGraph::new(),
                configuration,
                native_globals: RefCell::new(HashMap::new()),
                foreign_functions: RefCell::new(HashMap::new()),
                native_resources: native_resource_registry(),
                warnings: RefCell::new(Vec::new()),
                shutdown_errors: RefCell::new(Vec::new()),
                active_plugins: RefCell::new(HashMap::new()),
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

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        let keys = functions
            .iter()
            .map(|function| {
                (
                    function.module_name().to_string(),
                    function.name().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let mut registry = self.state.foreign_functions.borrow_mut();
        for (index, key) in keys.iter().enumerate() {
            if registry.contains_key(key) || keys[..index].contains(key) {
                return Err(NativeDescriptorError::new(format!(
                    "foreign binding `{}.{}` is already defined",
                    key.0, key.1
                )));
            }
        }
        for (key, function) in keys.into_iter().zip(functions) {
            registry.insert(key, function);
        }
        Ok(())
    }

    fn remove_foreign_batch(&self, functions: &[NativeFunction]) {
        let mut registry = self.state.foreign_functions.borrow_mut();
        for function in functions {
            let key = (
                function.module_name().to_string(),
                function.name().to_string(),
            );
            if registry
                .get(&key)
                .is_some_and(|registered| registered.same_function(function))
            {
                registry.remove(&key);
            }
        }
    }

    fn shutdown(&self) {
        self.state
            .shutdown_errors
            .borrow_mut()
            .extend(self.state.native_resources.finalize_all_for_shutdown());
        let plugins = std::mem::take(&mut *self.state.active_plugins.borrow_mut());
        for (_, mut plugin) in plugins {
            self.remove_foreign_batch(&plugin.functions);
            if let Err(error) = plugin.cleanup() {
                self.state.shutdown_errors.borrow_mut().push(error);
            }
        }
    }
}

impl ModuleResolver for DesktopLoader {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.state.resolver.resolve(request)
    }
}

impl ModuleGraphHost for DesktopLoader {
    type Activation = StagedClutchPlugin;

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.state
            .foreign_functions
            .borrow()
            .iter()
            .filter(|((module, _), _)| module == "slug.builtin")
            .map(|((_, name), function)| (name.clone(), Value::Native(function.clone())))
            .collect()
    }

    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError> {
        if source
            .activation
            .as_ref()
            .is_some_and(|lease| self.state.active_plugins.borrow().contains_key(lease))
        {
            return Ok(None);
        }
        self.state.resolver.stage_activation(source)
    }

    fn register_module_activation(
        &self,
        source: &ModuleSource,
        activation: &Self::Activation,
    ) -> Result<(), ModuleLoadError> {
        self.define_foreign_batch(activation.functions.clone())
            .map_err(|error| ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            })
    }

    fn remove_module_activation(&self, activation: &Self::Activation) {
        self.remove_foreign_batch(&activation.functions);
    }

    fn cleanup_module_activation(&self, activation: &mut Option<Self::Activation>) {
        if let Some(activation) = activation {
            let _ = activation.cleanup();
        }
    }

    fn retain_module_activation(&self, source: &ModuleSource, activation: Self::Activation) {
        if let Some(lease) = &source.activation {
            self.state
                .active_plugins
                .borrow_mut()
                .insert(lease.clone(), activation);
        }
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
        self.state
            .foreign_functions
            .borrow()
            .get(&(module.into(), name.into()))
            .cloned()
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        self.define_foreign_batch(functions)
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.state.native_globals.borrow_mut().insert(name, value);
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.state.native_resources.clone()
    }

    fn configuration(&self) -> &Configuration {
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
        for plugin in self.active_plugins.get_mut().values_mut() {
            if let Err(error) = plugin.cleanup() {
                self.shutdown_errors.get_mut().push(error);
            }
        }
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
    let loader = DesktopLoader::new(
        DesktopResolver::with_clutch_repository(
            source_root.to_path_buf(),
            default_library_root(slug_home),
            clutches,
        ),
        configuration,
    );
    Ok((Vm::with_host(Rc::new(loader.clone())), loader))
}
