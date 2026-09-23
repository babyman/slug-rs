use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    ClutchRepository, Configuration, FfiPrototypeLibrary, ModuleDeclaration, NativeDescriptorError,
    NativeFunction, Program, SourceError, Value, Vm,
    clutch::{self, StagedClutchPlugin},
    native::{NativeResourceRegistry, native_resource_registry},
    source::{
        InteractiveCompilation, InteractiveCompilerState, compile_with_resolver,
        environment::ModuleSnapshot,
    },
};

/// Opaque host-defined identity for one resolved module.
///
/// The VM compares identities for module ownership and cache lookup but does
/// not interpret their storage-specific representation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ModuleKey(String);

impl ModuleKey {
    #[must_use]
    pub fn new(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A logical module request issued by source compilation or runtime import.
#[derive(Clone, Copy, Debug)]
pub struct ModuleRequest<'a> {
    pub importer: Option<&'a ModuleKey>,
    pub name: &'a str,
}

impl<'a> ModuleRequest<'a> {
    #[must_use]
    pub const fn new(importer: Option<&'a ModuleKey>, name: &'a str) -> Self {
        Self { importer, name }
    }
}

/// Resolves logical module identities into host-provided source.
pub trait ModuleResolver {
    /// Resolves one requested module without exposing host storage to the VM.
    ///
    /// # Errors
    ///
    /// Returns a checked failure when the name is invalid, unavailable, or its
    /// host-provided source cannot be read.
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError>;
}

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
    configuration: Configuration,
    runtime: ModuleRuntime,
    native_globals: RefCell<HashMap<String, Value>>,
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    active_clutch_plugins: RefCell<HashMap<PathBuf, StagedClutchPlugin>>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSource {
    /// Stable host identity used by future filesystem-free module caches.
    pub key: ModuleKey,
    /// Host-provided label used in source and module diagnostics.
    pub diagnostic_name: String,
    pub text: String,
    path: PathBuf,
    activation: Option<ClutchPluginSource>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleLoadError {
    InvalidName(String),
    NotFound { name: String, searched: Vec<String> },
    Read { location: String, message: String },
    Source { location: String, message: String },
    Clutch { location: String, message: String },
}

impl fmt::Display for ModuleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(f, "invalid module name `{name}`"),
            Self::NotFound { name, .. } => write!(f, "module `{name}` was not found"),
            Self::Read { location, message } => write!(f, "cannot read {location}: {message}"),
            Self::Source { location, message } => {
                write!(f, "cannot compile {location}: {message}")
            }
            Self::Clutch { location, message } => {
                write!(f, "cannot load clutch {location}: {message}")
            }
        }
    }
}

impl std::error::Error for ModuleLoadError {}

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
                configuration,
                runtime: ModuleRuntime::new(),
                native_globals: RefCell::new(HashMap::new()),
                foreign_functions: RefCell::new(HashMap::new()),
                active_clutch_plugins: RefCell::new(HashMap::new()),
                native_resources: native_resource_registry(),
                warnings: RefCell::new(Vec::new()),
                shutdown_errors: RefCell::new(Vec::new()),
            }),
        }
    }

    /// The immutable configuration shared by the program module and loaded modules.
    #[must_use]
    pub fn configuration(&self) -> &Configuration {
        &self.state.configuration
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
                        path: path.clone(),
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
            return Ok(ModuleSource {
                key: ModuleKey::new(module.path.to_string_lossy()),
                diagnostic_name: module.path.to_string_lossy().into_owned(),
                path: module.path,
                text,
                activation: match (module.plugin_entry, module.native_plugin) {
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
                    (Some(_), Some(_)) => {
                        unreachable!("clutch manifest validation is inconsistent")
                    }
                },
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

    fn compile_source_for_module(
        &self,
        key: &ModuleKey,
        diagnostic_name: &str,
        source: &str,
        include_implicit_builtins: bool,
    ) -> Result<Program, SourceError> {
        self.state.runtime.compile_source_for_module(
            self,
            key,
            diagnostic_name,
            source,
            include_implicit_builtins,
        )
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
        let source = match self.resolve(request) {
            Ok(source) => source,
            Err(ModuleLoadError::NotFound { .. })
                if request.name == "slug.builtin" && !self.builtin_globals().is_empty() =>
            {
                return Ok(self.virtual_builtin_module());
            }
            Err(error) => return Err(error),
        };
        if let Some(instance) = self.state.runtime.state.instances.borrow().get(&source.key) {
            return Ok(instance.clone());
        }
        let mut plugin = self.stage_clutch_plugin(&source)?;
        let program = self.cached_or_compile(&source, request.name, &mut plugin)?;
        if let Some(staged) = plugin.as_ref()
            && let Err(error) = self.define_foreign_batch(staged.functions.clone())
        {
            Self::cleanup_plugin(&mut plugin);
            return Err(ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            });
        }
        let mut vm = Vm::with_module_bindings(self, program.bindings());
        let program = match vm.install_named(program, "main") {
            Ok(program) => program,
            Err(error) => {
                if let Some(staged) = plugin.as_ref() {
                    self.remove_foreign_batch(&staged.functions);
                }
                Self::cleanup_plugin(&mut plugin);
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
            .runtime
            .state
            .instances
            .borrow_mut()
            .insert(source.key.clone(), instance.clone());
        if let Err(error) = vm.run_module(&program) {
            self.state
                .runtime
                .state
                .instances
                .borrow_mut()
                .remove(&source.key);
            if let Some(staged) = plugin.as_ref() {
                self.remove_foreign_batch(&staged.functions);
            }
            Self::cleanup_plugin(&mut plugin);
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
            .runtime
            .state
            .instances
            .borrow_mut()
            .insert(source.key.clone(), instance.clone());
        if let Some(plugin) = plugin {
            let root = source
                .activation
                .as_ref()
                .map_or_else(|| source.path.clone(), |plugin| plugin.root().to_path_buf());
            self.state
                .active_clutch_plugins
                .borrow_mut()
                .insert(root, plugin);
        }
        Ok(instance)
    }

    fn cached_or_compile(
        &self,
        source: &ModuleSource,
        module_name: &str,
        plugin: &mut Option<StagedClutchPlugin>,
    ) -> Result<Program, ModuleLoadError> {
        if let Some(program) = self.state.runtime.state.compiled.borrow().get(&source.key) {
            return Ok(program.clone());
        }
        let program = self
            .compile_source_for_module(
                &source.key,
                &source.diagnostic_name,
                &source.text,
                module_name != "slug.builtin",
            )
            .map_err(|error| ModuleLoadError::Source {
                location: source.diagnostic_name.clone(),
                message: error.to_string(),
            });
        if program.is_err() {
            Self::cleanup_plugin(plugin);
        }
        let mut program = program?;
        program.set_module_name(module_name);
        program.set_module_key(source.key.clone());
        self.state
            .runtime
            .state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key.clone(), program.semantic_snapshot().clone());
        self.state
            .runtime
            .state
            .compiled
            .borrow_mut()
            .insert(source.key.clone(), program.clone());
        Ok(program)
    }

    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.runtime.state.instances.borrow().len()
    }

    /// Returns and clears module warnings accumulated during evaluation.
    #[must_use]
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.warnings.borrow_mut())
    }

    pub(crate) fn warn(&self, message: impl Into<String>) {
        self.state.warnings.borrow_mut().push(message.into());
    }

    pub(crate) fn define_native(&self, name: String, value: Value) {
        self.state.native_globals.borrow_mut().insert(name, value);
    }

    pub(crate) fn native_globals(&self) -> HashMap<String, Value> {
        self.state.native_globals.borrow().clone()
    }

    pub(crate) fn builtin_globals(&self) -> HashMap<String, Value> {
        self.state
            .foreign_functions
            .borrow()
            .iter()
            .filter(|((module, _), _)| module == "slug.builtin")
            .map(|((_, name), function)| (name.clone(), Value::Native(function.clone())))
            .collect()
    }

    pub(crate) fn define_foreign(
        &self,
        function: NativeFunction,
    ) -> Result<(), NativeDescriptorError> {
        self.define_foreign_batch(vec![function])
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

    pub(crate) fn foreign(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.state
            .foreign_functions
            .borrow()
            .get(&(module.into(), name.into()))
            .cloned()
    }

    pub(crate) fn native_resources(&self) -> NativeResourceRegistry {
        self.state.native_resources.clone()
    }

    /// Finalizes native resources and releases every clutch-owned registration.
    ///
    /// A host must not execute additional work through a loader after shutdown.
    pub fn shutdown(&self) {
        self.state
            .shutdown_errors
            .borrow_mut()
            .extend(self.state.native_resources.finalize_all_for_shutdown());
        let plugins = std::mem::take(&mut *self.state.active_clutch_plugins.borrow_mut());
        for (_, mut plugin) in plugins {
            self.remove_foreign_batch(&plugin.functions);
            if let Err(error) = plugin.cleanup() {
                self.state.shutdown_errors.borrow_mut().push(error);
            }
        }
    }

    /// Returns and clears failures reported by best-effort plugin shutdown.
    #[must_use]
    pub fn take_shutdown_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.shutdown_errors.borrow_mut())
    }

    fn virtual_builtin_module(&self) -> ModuleInstance {
        let key = ModuleKey::new("<slug.builtin>");
        if let Some(instance) = self.state.runtime.state.instances.borrow().get(&key) {
            return instance.clone();
        }
        let exports = Value::Map(Rc::new(
            self.builtin_globals()
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
            .runtime
            .state
            .instances
            .borrow_mut()
            .insert(key, instance.clone());
        instance
    }

    fn stage_clutch_plugin(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<StagedClutchPlugin>, ModuleLoadError> {
        let Some(plugin) = &source.activation else {
            return Ok(None);
        };
        if self
            .state
            .active_clutch_plugins
            .borrow()
            .contains_key(plugin.root())
        {
            return Ok(None);
        }
        let mut registrar = clutch::ClutchPluginRegistrar::new(plugin.module_names().to_vec());
        let result = match plugin {
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
        self.shutdown_errors
            .get_mut()
            .extend(self.native_resources.finalize_all_for_shutdown());
        for plugin in self.active_clutch_plugins.get_mut().values_mut() {
            if let Err(error) = plugin.cleanup() {
                self.shutdown_errors.get_mut().push(error);
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
