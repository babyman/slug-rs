use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

use crate::{
    Configuration, ModuleDeclaration, NativeDescriptorError, NativeFunction, Program, SourceError,
    Value, Vm, VmHost, VmHostError, VmModuleExports,
    source::{
        InteractiveCompilation, InteractiveCompilerState, compile_with_resolver,
        environment::ModuleSnapshot,
    },
};
use slug_loader::{ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource};
use slug_vm::{NativeResourceRegistry, native_resource_registry};

/// Host-owned roots used to load Slug module source.
#[derive(Clone)]
pub struct ModuleHost {
    state: Rc<ModuleHostState>,
}

struct ModuleHostState {
    resolver: Rc<dyn ModuleResolver>,
    graph: ModuleGraph,
    services: ModuleRuntimeServices,
}

/// Runtime services shared by a module graph, independent of desktop lookup.
#[derive(Debug)]
struct ModuleRuntimeServices {
    configuration: Configuration,
    native_globals: RefCell<HashMap<String, Value>>,
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    native_resources: slug_vm::NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    shutdown_errors: RefCell<Vec<String>>,
}

/// The storage-independent state shared by all requests for one module graph.
///
/// Resolver and host services remain on `ModuleHost` while this type is
/// extracted; keeping the graph caches together makes that boundary explicit.
#[derive(Clone, Debug)]
pub struct ModuleGraph {
    state: Rc<ModuleGraphState>,
}

#[derive(Debug)]
struct ModuleGraphState {
    compiled: RefCell<HashMap<ModuleKey, Program>>,
    semantic_snapshots: RefCell<HashMap<ModuleKey, ModuleSnapshot>>,
    resolving_snapshots: RefCell<HashSet<ModuleKey>>,
    instances: RefCell<HashMap<ModuleKey, ModuleInstance>>,
}

/// The non-resolution services module initialization needs from its host.
///
/// This keeps graph state independent from the desktop loader. In particular,
/// `ModuleGraph` never inspects an activation's storage representation or
/// native registrations; it only sequences their lifecycle around module
/// compilation and execution.
///
/// This is an intentionally unstable composition seam. A frontend graph host
/// supplies VM services and owns activation transactions; a resolver supplies
/// module source. Desktop policy does not belong to the graph itself.
pub trait ModuleGraphHost: ModuleResolver {
    type Activation;

    fn builtin_globals(&self) -> HashMap<String, Value>;

    /// Stages any resolver-owned activation required by `source`.
    ///
    /// # Errors
    ///
    /// Returns a checked error when the activation cannot be prepared.
    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError>;

    /// Publishes one staged activation's native registrations.
    ///
    /// # Errors
    ///
    /// Returns a checked error without publishing a partial registration batch.
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

impl Default for ModuleGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleGraph {
    /// Creates an empty module graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Rc::new(ModuleGraphState {
                compiled: RefCell::new(HashMap::new()),
                semantic_snapshots: RefCell::new(HashMap::new()),
                resolving_snapshots: RefCell::new(HashSet::new()),
                instances: RefCell::new(HashMap::new()),
            }),
        }
    }

    /// Resolves and compiles one module, caching its program in this graph.
    ///
    /// # Errors
    ///
    /// Returns a checked resolver or source error.
    pub fn compile(
        &self,
        resolver: &dyn ModuleResolver,
        request: ModuleRequest<'_>,
    ) -> Result<Program, ModuleLoadError> {
        let source = resolver.resolve(request)?;
        if let Some(program) = self.state.compiled.borrow().get(&source.key) {
            return Ok(program.clone());
        }
        let mut compilation = self
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
        compilation.program.set_module_name(request.name);
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key.clone(), compilation.snapshot);
        self.state
            .compiled
            .borrow_mut()
            .insert(source.key, compilation.program.clone());
        Ok(compilation.program)
    }

    /// Compiles interactive forms using this graph's cached module snapshots.
    ///
    /// # Errors
    ///
    /// Returns a checked source error for invalid syntax or semantics.
    pub fn compile_interactive_forms(
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

    /// Compiles entry source while resolving static imports through `resolver`.
    ///
    /// # Errors
    ///
    /// Returns a checked source error for invalid syntax or semantics.
    pub fn compile_source(
        &self,
        resolver: &dyn ModuleResolver,
        path: &str,
        source: &str,
    ) -> Result<Program, SourceError> {
        self.compile_source_for_module(resolver, &ModuleKey::new(path), path, source, true)
            .map(|compilation| compilation.program)
    }

    fn compile_source_for_module(
        &self,
        resolver: &dyn ModuleResolver,
        key: &ModuleKey,
        diagnostic_name: &str,
        source: &str,
        include_implicit_builtins: bool,
    ) -> Result<crate::source::CompiledSource, SourceError> {
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
            .map(|compilation| compilation.snapshot);
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

    /// Resolves, compiles, and initializes one module instance.
    ///
    /// # Errors
    ///
    /// Returns a checked resolver, source, or module-runtime error.
    pub fn initialize<H: ModuleGraphHost>(
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
        let mut compilation = self
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
        compilation.program.set_module_name(module_name);
        compilation.program.set_module_key(source.key.clone());
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(source.key.clone(), compilation.snapshot);
        self.state
            .compiled
            .borrow_mut()
            .insert(source.key.clone(), compilation.program.clone());
        Ok(compilation.program)
    }

    fn virtual_builtin_module<H: ModuleGraphHost>(&self, host: &H) -> ModuleInstance {
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

    /// Returns the number of compiled module programs held by this graph.
    #[must_use]
    pub fn cached_module_count(&self) -> usize {
        self.state.compiled.borrow().len()
    }

    /// Returns the number of initialized module instances held by this graph.
    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.instances.borrow().len()
    }
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

impl ModuleInstance {
    /// Returns the live export map used by runtime `import()` calls.
    #[must_use]
    pub fn live_exports(&self) -> Value {
        self.live_exports.clone()
    }
}

impl ModuleHost {
    /// Creates a graph host backed by an explicitly supplied import resolver.
    ///
    /// This constructor is intended for restricted and in-memory hosts. The
    /// resolver supplies every external module; desktop filesystem and Clutch
    /// lookup remain unavailable through this path.
    #[must_use]
    pub fn with_resolver(resolver: Rc<dyn ModuleResolver>, configuration: Configuration) -> Self {
        Self {
            state: Rc::new(ModuleHostState {
                resolver,
                graph: ModuleGraph::new(),
                services: ModuleRuntimeServices {
                    configuration,
                    native_globals: RefCell::new(HashMap::new()),
                    foreign_functions: RefCell::new(HashMap::new()),
                    native_resources: native_resource_registry(),
                    warnings: RefCell::new(Vec::new()),
                    shutdown_errors: RefCell::new(Vec::new()),
                },
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

impl ModuleResolver for ModuleHost {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.state.resolver.resolve(request)
    }
}

impl ModuleHost {
    /// Loads and compiles a module, returning a cached program for repeat requests.
    ///
    /// # Errors
    ///
    /// Returns an error for loader failures or invalid module source.
    pub fn compile(&self, importer: Option<&Path>, name: &str) -> Result<Program, ModuleLoadError> {
        let importer = importer.map(|path| ModuleKey::new(path.to_string_lossy()));
        self.state
            .graph
            .compile(self, ModuleRequest::new(importer.as_ref(), name))
    }

    /// Compiles source while resolving cached semantic snapshots for static imports.
    ///
    /// # Errors
    ///
    /// Returns a checked source error for invalid syntax or semantics.
    pub fn compile_source(&self, path: &str, source: &str) -> Result<Program, SourceError> {
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

    #[must_use]
    pub fn cached_module_count(&self) -> usize {
        self.state.graph.cached_module_count()
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
        self.state.graph.initialize(self, request)
    }

    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.graph.initialized_module_count()
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
    }

    /// Returns and clears failures reported by best-effort plugin shutdown.
    #[must_use]
    pub fn take_shutdown_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.state.services.shutdown_errors.borrow_mut())
    }
}

impl ModuleGraphHost for ModuleHost {
    type Activation = ();

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.builtin_globals()
    }

    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Self::Activation>, ModuleLoadError> {
        source.activation.as_ref().map_or_else(
            || Ok(None),
            |lease| {
                Err(ModuleLoadError::Clutch {
                    location: source.diagnostic_name.clone(),
                    message: format!(
                        "module activation lease `{lease:?}` requires an executable host"
                    ),
                })
            },
        )
    }

    fn register_module_activation(
        &self,
        _: &ModuleSource,
        &(): &Self::Activation,
    ) -> Result<(), ModuleLoadError> {
        Ok(())
    }

    fn remove_module_activation(&self, &(): &Self::Activation) {}

    fn cleanup_module_activation(&self, _: &mut Option<Self::Activation>) {}

    fn retain_module_activation(&self, _: &ModuleSource, (): Self::Activation) {}

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: std::rc::Rc<dyn VmHost> = std::rc::Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for ModuleHost {
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

    fn native_resources(&self) -> slug_vm::NativeResourceRegistry {
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

impl Drop for ModuleHostState {
    fn drop(&mut self) {
        self.services
            .shutdown_errors
            .get_mut()
            .extend(self.services.native_resources.finalize_all_for_shutdown());
    }
}
