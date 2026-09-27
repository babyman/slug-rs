use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    ClutchRepository, Configuration, FfiPrototypeLibrary, ModuleDeclaration, NativeDescriptorError,
    NativeFunction, Program, SourceError, Value, Vm, VmHost, VmHostError, VmModuleExports,
    clutch::{self, StagedClutchPlugin},
    native::{NativeResourceRegistry, native_resource_registry},
    source::{
        InteractiveCompilation, InteractiveCompilerState, compile_with_resolver,
        environment::ModuleSnapshot,
    },
};
use slug_loader::{
    ModuleActivation, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
};

/// Host-owned roots used to load Slug module source.
#[derive(Clone, Debug)]
pub struct ModuleLoader {
    state: Rc<ModuleLoaderState>,
}

#[derive(Debug)]
struct ModuleLoaderState {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
    clutch_repository: ClutchRepository,
    runtime: ModuleRuntime,
    services: ModuleRuntimeServices,
    activation_sources: RefCell<HashMap<ModuleActivation, ClutchPluginSource>>,
    active_clutch_plugins: RefCell<HashMap<ModuleActivation, StagedClutchPlugin>>,
}

/// Runtime services shared by a module graph, independent of desktop lookup.
#[derive(Debug)]
struct ModuleRuntimeServices {
    configuration: Configuration,
    native_globals: RefCell<HashMap<String, Value>>,
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    native_resources: NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    shutdown_errors: RefCell<Vec<String>>,
}

/// The storage-independent state shared by all requests for one module graph.
///
/// Resolver and host services remain on `ModuleLoader` while this type is
/// extracted; keeping the graph caches together makes that boundary explicit.
#[derive(Clone, Debug)]
struct ModuleRuntime {
    state: Rc<ModuleRuntimeState>,
}

#[derive(Debug)]
struct ModuleRuntimeState {
    compiled: RefCell<HashMap<ModuleKey, Program>>,
    semantic_snapshots: RefCell<HashMap<ModuleKey, ModuleSnapshot>>,
    resolving_snapshots: RefCell<HashSet<ModuleKey>>,
    instances: RefCell<HashMap<ModuleKey, ModuleInstance>>,
}

/// The non-resolution services module initialization needs from its host.
///
/// This keeps graph state independent from the desktop loader. In particular,
/// `ModuleRuntime` never inspects an activation's storage representation or
/// native registrations; it only sequences their lifecycle around module
/// compilation and execution.
trait ModuleRuntimeHost: ModuleResolver {
    type Activation;

    fn builtin_globals(&self) -> HashMap<String, Value>;

    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError>;

    fn register_module_activation(
        &self,
        source: &ModuleSource,
        activation: &Self::Activation,
    ) -> Result<(), ModuleLoadError>;

    fn remove_module_activation(&self, activation: &Self::Activation);

    fn cleanup_module_activation(&self, activation: &mut Option<Self::Activation>);

    fn retain_module_activation(&self, source: &ModuleSource, activation: Self::Activation);

    fn module_vm(&self, bindings: &[String]) -> Vm;
}

impl ModuleRuntime {
    fn new() -> Self {
        Self {
            state: Rc::new(ModuleRuntimeState {
                compiled: RefCell::new(HashMap::new()),
                semantic_snapshots: RefCell::new(HashMap::new()),
                resolving_snapshots: RefCell::new(HashSet::new()),
                instances: RefCell::new(HashMap::new()),
            }),
        }
    }

    fn compile(
        &self,
        resolver: &dyn ModuleResolver,
        request: ModuleRequest<'_>,
    ) -> Result<Program, ModuleLoadError> {
        let source = resolver.resolve(request)?;
        if let Some(program) = self.state.compiled.borrow().get(&source.key) {
            return Ok(program.clone());
        }
        let mut program = self
            .compile_source_for_module(
                resolver,
                &source.key,
                &source.diagnostic_name,
                &source.text,
                request.name != "slug.builtin",
            )
            .map_err(|error| ModuleLoadError::Source {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            })?;
        program.set_module_name(request.name);
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key.clone(), program.semantic_snapshot().clone());
        self.state
            .compiled
            .borrow_mut()
            .insert(source.key, program.clone());
        Ok(program)
    }

    fn compile_interactive_forms(
        &self,
        resolver: &dyn ModuleResolver,
        path: &str,
        source: &str,
        state: &InteractiveCompilerState,
    ) -> Result<Vec<InteractiveCompilation>, SourceError> {
        let importer = ModuleKey::new(path);
        crate::source::compile_interactive_forms_with_resolver(path, source, state, |name| {
            self.semantic_snapshot(resolver, ModuleRequest::new(Some(&importer), name))
        })
    }

    fn compile_source_for_module(
        &self,
        resolver: &dyn ModuleResolver,
        key: &ModuleKey,
        diagnostic_name: &str,
        source: &str,
        include_implicit_builtins: bool,
    ) -> Result<Program, SourceError> {
        compile_with_resolver(diagnostic_name, source, include_implicit_builtins, |name| {
            self.semantic_snapshot(resolver, ModuleRequest::new(Some(key), name))
        })
    }

    fn semantic_snapshot(
        &self,
        resolver: &dyn ModuleResolver,
        request: ModuleRequest<'_>,
    ) -> Option<ModuleSnapshot> {
        let source = resolver.resolve(request).ok()?;
        if let Some(snapshot) = self.state.semantic_snapshots.borrow().get(&source.key) {
            return Some(snapshot.clone());
        }
        if !self
            .state
            .resolving_snapshots
            .borrow_mut()
            .insert(source.key.clone())
        {
            return None;
        }
        let snapshot = self
            .compile_source_for_module(
                resolver,
                &source.key,
                &source.diagnostic_name,
                &source.text,
                request.name != "slug.builtin",
            )
            .ok()
            .map(|program| program.semantic_snapshot().clone());
        self.state
            .resolving_snapshots
            .borrow_mut()
            .remove(&source.key);
        let snapshot = snapshot?;
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key, snapshot.clone());
        Some(snapshot)
    }

    fn initialize<H: ModuleRuntimeHost>(
        &self,
        host: &H,
        request: ModuleRequest<'_>,
    ) -> Result<ModuleInstance, ModuleLoadError> {
        let source = match host.resolve(request) {
            Ok(source) => source,
            Err(ModuleLoadError::NotFound { .. })
                if request.name == "slug.builtin" && !host.builtin_globals().is_empty() =>
            {
                return Ok(self.virtual_builtin_module(host));
            }
            Err(error) => return Err(error),
        };
        if let Some(instance) = self.state.instances.borrow().get(&source.key) {
            return Ok(instance.clone());
        }

        let mut activation = host.stage_module_activation(&source)?;
        let program = match self.compile_cached_or_source(host, &source, request.name) {
            Ok(program) => program,
            Err(error) => {
                host.cleanup_module_activation(&mut activation);
                return Err(error);
            }
        };
        if let Some(staged) = activation.as_ref()
            && let Err(error) = host.register_module_activation(&source, staged)
        {
            host.cleanup_module_activation(&mut activation);
            return Err(error);
        }

        let mut vm = host.module_vm(program.bindings());
        let program = match vm.install_named(program, "main") {
            Ok(program) => program,
            Err(error) => {
                if let Some(staged) = activation.as_ref() {
                    host.remove_module_activation(staged);
                }
                host.cleanup_module_activation(&mut activation);
                return Err(ModuleLoadError::Source {
                    location: source.diagnostic_name.clone(),
                    message: error.to_string(),
                });
            }
        };
        let instance = ModuleInstance {
            key: source.key.clone(),
            diagnostic_name: source.diagnostic_name.clone(),
            program: program.program().clone(),
            exports: Value::Map(Rc::new(Vec::new())),
            metadata: vm.module_metadata().to_vec(),
            live_exports: vm.live_exported_values(program.program()),
        };
        self.state
            .instances
            .borrow_mut()
            .insert(source.key.clone(), instance.clone());
        if let Err(error) = vm.run_module(&program) {
            self.state.instances.borrow_mut().remove(&source.key);
            if let Some(staged) = activation.as_ref() {
                host.remove_module_activation(staged);
            }
            host.cleanup_module_activation(&mut activation);
            return Err(ModuleLoadError::Source {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            });
        }
        let instance = ModuleInstance {
            exports: vm.exported_values(program.program()),
            metadata: vm.module_metadata().to_vec(),
            ..instance
        };
        self.state
            .instances
            .borrow_mut()
            .insert(source.key.clone(), instance.clone());
        if let Some(activation) = activation {
            host.retain_module_activation(&source, activation);
        }
        Ok(instance)
    }

    fn compile_cached_or_source(
        &self,
        resolver: &dyn ModuleResolver,
        source: &ModuleSource,
        module_name: &str,
    ) -> Result<Program, ModuleLoadError> {
        if let Some(program) = self.state.compiled.borrow().get(&source.key) {
            return Ok(program.clone());
        }
        let mut program = self
            .compile_source_for_module(
                resolver,
                &source.key,
                &source.diagnostic_name,
                &source.text,
                module_name != "slug.builtin",
            )
            .map_err(|error| ModuleLoadError::Source {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            })?;
        program.set_module_name(module_name);
        program.set_module_key(source.key.clone());
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key.clone(), program.semantic_snapshot().clone());
        self.state
            .compiled
            .borrow_mut()
            .insert(source.key.clone(), program.clone());
        Ok(program)
    }

    fn virtual_builtin_module<H: ModuleRuntimeHost>(&self, host: &H) -> ModuleInstance {
        let key = ModuleKey::new("<slug.builtin>");
        if let Some(instance) = self.state.instances.borrow().get(&key) {
            return instance.clone();
        }
        let exports = Value::Map(Rc::new(
            host.builtin_globals()
                .into_iter()
                .map(|(name, value)| (Value::string(name), value))
                .collect(),
        ));
        let mut program = Program::new();
        program.set_module_name("slug.builtin");
        let instance = ModuleInstance {
            key: key.clone(),
            diagnostic_name: "<slug.builtin>".into(),
            program,
            exports: exports.clone(),
            metadata: Vec::new(),
            live_exports: exports,
        };
        self.state
            .instances
            .borrow_mut()
            .insert(key, instance.clone());
        instance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClutchPluginSource {
    Host {
        root: PathBuf,
        entry: String,
        module_names: Vec<String>,
    },
    Native {
        root: PathBuf,
        library: PathBuf,
        abi: String,
        module_names: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct ModuleInstance {
    pub key: ModuleKey,
    pub diagnostic_name: String,
    pub program: Program,
    pub exports: Value,
    pub metadata: Vec<ModuleDeclaration>,
    pub(crate) live_exports: Value,
}

impl ModuleLoader {
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self::with_configuration(source_root, library_root, Configuration::default())
    }

    /// Creates a loader that shares one immutable configuration store with its modules.
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

    /// Creates a loader with an explicit local clutch repository.
    #[must_use]
    pub fn with_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        clutch_repository: ClutchRepository,
    ) -> Self {
        Self::with_configuration_and_clutch_repository(
            source_root,
            library_root,
            Configuration::default(),
            clutch_repository,
        )
    }

    /// Creates a loader with shared configuration and an explicit clutch repository.
    #[must_use]
    pub fn with_configuration_and_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        configuration: Configuration,
        clutch_repository: ClutchRepository,
    ) -> Self {
        Self {
            state: Rc::new(ModuleLoaderState {
                source_root: source_root.into(),
                library_root,
                clutch_repository,
                runtime: ModuleRuntime::new(),
                services: ModuleRuntimeServices {
                    configuration,
                    native_globals: RefCell::new(HashMap::new()),
                    foreign_functions: RefCell::new(HashMap::new()),
                    native_resources: native_resource_registry(),
                    warnings: RefCell::new(Vec::new()),
                    shutdown_errors: RefCell::new(Vec::new()),
                },
                activation_sources: RefCell::new(HashMap::new()),
                active_clutch_plugins: RefCell::new(HashMap::new()),
            }),
        }
    }

    /// The immutable configuration shared by the program module and loaded modules.
    #[must_use]
    pub fn configuration(&self) -> &Configuration {
        &self.state.services.configuration
    }

    /// Loads a dotted module name without exposing file-system operations to Slug code.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid name, an unavailable module, or a host read failure.
    pub fn load(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleSource, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.resolve(ModuleRequest::new(importer.as_ref(), name))
    }
}

impl ModuleResolver for ModuleLoader {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        let relative = module_path(request.name)?;
        let mut candidates = Vec::new();
        if let Some(importer) = request
            .importer
            .map(ModuleKey::as_str)
            .map(Path::new)
            .and_then(Path::parent)
        {
            candidates.push(importer.join(&relative));
        }
        candidates.push(self.state.source_root.join(&relative));
        if let Some(library_root) = &self.state.library_root {
            candidates.push(library_root.join(&relative));
        }
        for path in &candidates {
            match fs::read_to_string(path) {
                Ok(text) => {
                    return Ok(ModuleSource {
                        key: ModuleKey::new(path.to_string_lossy()),
                        diagnostic_name: path.to_string_lossy().into_owned(),
                        text,
                        activation: None,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ModuleLoadError::Read {
                        location: path.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                }
            }
        }
        if let Some(root) = self.state.clutch_repository.provider(request.name) {
            let module = clutch::load_module(root, request.name).map_err(|message| {
                ModuleLoadError::Clutch {
                    location: root.to_string_lossy().into_owned(),
                    message,
                }
            })?;
            let text = fs::read_to_string(&module.path).map_err(|error| ModuleLoadError::Read {
                location: module.path.to_string_lossy().into_owned(),
                message: error.to_string(),
            })?;
            let activation = match (module.plugin_entry, module.native_plugin) {
                (Some(entry), None) => Some(ClutchPluginSource::Host {
                    root: module.root,
                    entry,
                    module_names: vec![request.name.into()],
                }),
                (None, Some(native)) => Some(ClutchPluginSource::Native {
                    root: module.root,
                    library: native.library,
                    abi: native.abi,
                    module_names: module.module_names,
                }),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("clutch manifest validation is inconsistent"),
            };
            let lease = activation.as_ref().map(|plugin| {
                let lease = ModuleActivation::new(plugin.root().to_string_lossy().into_owned());
                self.state
                    .activation_sources
                    .borrow_mut()
                    .insert(lease.clone(), plugin.clone());
                lease
            });
            return Ok(ModuleSource {
                key: ModuleKey::new(module.path.to_string_lossy()),
                diagnostic_name: module.path.to_string_lossy().into_owned(),
                text,
                activation: lease,
            });
        }
        Err(ModuleLoadError::NotFound {
            name: request.name.into(),
            searched: candidates
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        })
    }
}

impl ModuleLoader {
    /// Loads and compiles a module, returning a cached program for repeat requests.
    ///
    /// # Errors
    ///
    /// Returns an error for loader failures or invalid module source.
    pub fn compile(&self, importer: Option<&Path>, name: &str) -> Result<Program, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.state
            .runtime
            .compile(self, ModuleRequest::new(importer.as_ref(), name))
    }

    /// Compiles source while resolving cached semantic snapshots for static imports.
    ///
    /// # Errors
    ///
    /// Returns a checked source error for invalid syntax or semantics.
    pub fn compile_source(&self, path: &str, source: &str) -> Result<Program, SourceError> {
        self.state.runtime.compile_source_for_module(
            self,
            &ModuleKey::new(path),
            path,
            source,
            true,
        )
    }

    #[doc(hidden)]
    pub fn compile_interactive_forms(
        &self,
        path: &str,
        source: &str,
        state: &InteractiveCompilerState,
    ) -> Result<Vec<InteractiveCompilation>, SourceError> {
        self.state
            .runtime
            .compile_interactive_forms(self, path, source, state)
    }

    #[must_use]
    pub fn cached_module_count(&self) -> usize {
        self.state.runtime.state.compiled.borrow().len()
    }

    /// Compiles and initializes one isolated module instance.
    ///
    /// # Errors
    ///
    /// Returns checked loader, source, or module-runtime failures.
    pub fn initialize(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleInstance, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.initialize_request(ModuleRequest::new(importer.as_ref(), name))
    }

    pub(crate) fn initialize_request(
        &self,
        request: ModuleRequest<'_>,
    ) -> Result<ModuleInstance, ModuleLoadError> {
        self.state.runtime.initialize(self, request)
    }

    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.runtime.state.instances.borrow().len()
    }

    /// Returns and clears module warnings accumulated during evaluation.
    #[must_use]
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.services.warnings.borrow_mut())
    }

    pub(crate) fn warn(&self, message: impl Into<String>) {
        self.state
            .services
            .warnings
            .borrow_mut()
            .push(message.into());
    }

    pub(crate) fn define_native(&self, name: String, value: Value) {
        self.state
            .services
            .native_globals
            .borrow_mut()
            .insert(name, value);
    }

    pub(crate) fn builtin_globals(&self) -> HashMap<String, Value> {
        self.state
            .services
            .foreign_functions
            .borrow()
            .iter()
            .filter(|((module, _), _)| module == "slug.builtin")
            .map(|((_, name), function)| (name.clone(), Value::Native(function.clone())))
            .collect()
    }

    pub(crate) fn define_foreign_batch(
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
        let mut registry = self.state.services.foreign_functions.borrow_mut();
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
        let mut registry = self.state.services.foreign_functions.borrow_mut();
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

    pub(crate) fn foreign(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.state
            .services
            .foreign_functions
            .borrow()
            .get(&(module.into(), name.into()))
            .cloned()
    }

    pub(crate) fn native_resources(&self) -> NativeResourceRegistry {
        self.state.services.native_resources.clone()
    }

    /// Finalizes native resources and releases every clutch-owned registration.
    ///
    /// A host must not execute additional work through a loader after shutdown.
    pub fn shutdown(&self) {
        self.state.services.shutdown_errors.borrow_mut().extend(
            self.state
                .services
                .native_resources
                .finalize_all_for_shutdown(),
        );
        let plugins = std::mem::take(&mut *self.state.active_clutch_plugins.borrow_mut());
        for (_, mut plugin) in plugins {
            self.remove_foreign_batch(&plugin.functions);
            if let Err(error) = plugin.cleanup() {
                self.state.services.shutdown_errors.borrow_mut().push(error);
            }
        }
    }

    /// Returns and clears failures reported by best-effort plugin shutdown.
    #[must_use]
    pub fn take_shutdown_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.services.shutdown_errors.borrow_mut())
    }

    fn stage_clutch_plugin(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<StagedClutchPlugin>, ModuleLoadError> {
        let Some(lease) = &source.activation else {
            return Ok(None);
        };
        if self
            .state
            .active_clutch_plugins
            .borrow()
            .contains_key(lease)
        {
            return Ok(None);
        }
        let plugin = self
            .state
            .activation_sources
            .borrow()
            .get(lease)
            .cloned()
            .ok_or_else(|| ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: "module activation lease is no longer available".into(),
            })?;
        let mut registrar = clutch::ClutchPluginRegistrar::new(plugin.module_names().to_vec());
        let result = match &plugin {
            ClutchPluginSource::Host { root, entry, .. } => {
                let initializer = self.state.clutch_repository.plugin(entry).ok_or_else(|| {
                    ModuleLoadError::Clutch {
                        location: root.to_string_lossy().into_owned(),
                        message: format!("plugin entry `{entry}` is not configured by the host"),
                    }
                })?;
                initializer(&mut registrar).map_err(|error| ModuleLoadError::Clutch {
                    location: root.to_string_lossy().into_owned(),
                    message: format!("plugin initialization failed: {error}"),
                })
            }
            ClutchPluginSource::Native {
                root, library, abi, ..
            } => {
                if abi == crate::ffi_prototype::ABI_PROFILE {
                    let module = FfiPrototypeLibrary::load(library).map_err(|error| {
                        ModuleLoadError::Clutch {
                            location: library.to_string_lossy().into_owned(),
                            message: format!("cannot load native plugin: {error}"),
                        }
                    })?;
                    module
                        .stage(&mut registrar)
                        .map_err(|error| ModuleLoadError::Clutch {
                            location: root.to_string_lossy().into_owned(),
                            message: format!("native plugin initialization failed: {error}"),
                        })?;
                    registrar
                        .set_cleanup_operation(move || {
                            module.shutdown();
                            Ok(())
                        })
                        .map_err(|error| ModuleLoadError::Clutch {
                            location: root.to_string_lossy().into_owned(),
                            message: format!("cannot retain native plugin cleanup: {error}"),
                        })
                } else {
                    Err(ModuleLoadError::Clutch {
                        location: root.to_string_lossy().into_owned(),
                        message: format!("unsupported native ABI `{abi}`"),
                    })
                }
            }
        };
        if let Err(error) = result {
            let mut staged = registrar.finish();
            let _ = staged.cleanup();
            return Err(error);
        }
        Ok(Some(registrar.finish()))
    }

    fn cleanup_plugin(plugin: &mut Option<StagedClutchPlugin>) {
        if let Some(plugin) = plugin {
            let _ = plugin.cleanup();
        }
    }
}

impl ModuleRuntimeHost for ModuleLoader {
    type Activation = StagedClutchPlugin;

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.builtin_globals()
    }

    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError> {
        self.stage_clutch_plugin(source)
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
        Self::cleanup_plugin(activation);
    }

    fn retain_module_activation(&self, source: &ModuleSource, activation: Self::Activation) {
        if let Some(lease) = &source.activation {
            self.state
                .active_clutch_plugins
                .borrow_mut()
                .insert(lease.clone(), activation);
        }
    }

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: std::rc::Rc<dyn VmHost> = std::rc::Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for ModuleLoader {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let importer = importer.map(ModuleKey::new);
        let instance = self
            .initialize_request(ModuleRequest::new(importer.as_ref(), name))
            .map_err(|error| match error {
                ModuleLoadError::NotFound { .. } => VmHostError::not_found(error.to_string()),
                error => VmHostError::new(error.to_string()),
            })?;
        Ok(VmModuleExports {
            exports: instance.live_exports,
        })
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.builtin_globals()
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        self.state.services.native_globals.borrow().clone()
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.foreign(module, name)
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        self.define_foreign_batch(functions)
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.define_native(name, value);
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.native_resources()
    }

    fn configuration(&self) -> &Configuration {
        self.configuration()
    }

    fn warn(&self, message: String) {
        self.warn(message);
    }

    fn shutdown(&self) {
        self.shutdown();
    }
}

impl ClutchPluginSource {
    fn root(&self) -> &Path {
        match self {
            Self::Host { root, .. } | Self::Native { root, .. } => root,
        }
    }

    fn module_names(&self) -> &[String] {
        match self {
            Self::Host { module_names, .. } | Self::Native { module_names, .. } => module_names,
        }
    }
}

impl Drop for ModuleLoaderState {
    fn drop(&mut self) {
        self.services
            .shutdown_errors
            .get_mut()
            .extend(self.services.native_resources.finalize_all_for_shutdown());
        for plugin in self.active_clutch_plugins.get_mut().values_mut() {
            if let Err(error) = plugin.cleanup() {
                self.services.shutdown_errors.get_mut().push(error);
            }
        }
    }
}

fn module_path(name: &str) -> Result<PathBuf, ModuleLoadError> {
    let mut path = PathBuf::new();
    for part in name.split('.') {
        if part.is_empty()
            || !part
                .chars()
                .all(|value| value == '_' || value.is_ascii_alphanumeric())
        {
            return Err(ModuleLoadError::InvalidName(name.into()));
        }
        path.push(part);
    }
    path.set_extension("slug");
    Ok(path)
}
