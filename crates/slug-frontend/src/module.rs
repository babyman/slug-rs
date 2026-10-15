use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

use crate::{
    SourceError,
    source::{
        InteractiveCompilation, InteractiveCompilerState, compile_with_resolver,
        environment::ModuleSnapshot, foreign_declarations,
    },
};
use slug_loader::{
    ForeignDeclaration, ModuleActivationTransaction, ModuleKey, ModuleLoadError, ModuleRequest,
    ModuleResolver, ModuleSource,
};
use slug_vm::{
    ModuleDeclaration, NativeDescriptorError, NativeFunction, NativeResourceRegistry, Program,
    Value, Vm, VmConfiguration, VmHost, VmHostError, VmModuleExports,
    cslug::{Artifact, ImportEdge, Module as ArtifactModule},
    native_resource_registry,
};

pub use slug_vm::cslug::ArtifactHost;

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
struct ModuleRuntimeServices {
    configuration: Rc<dyn VmConfiguration>,
    native_globals: RefCell<HashMap<String, Value>>,
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    native_resources: slug_vm::NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    shutdown_errors: RefCell<Vec<String>>,
}

/// Source text retained for one module of a compiled artifact.
pub(crate) struct ArtifactSource {
    pub(crate) diagnostic_name: String,
    pub(crate) text: String,
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
/// supplies VM services, while a resolver owns source and any activation
/// transaction. Desktop policy does not belong to the graph itself.
pub trait ModuleGraphHost: ModuleResolver {
    /// Stages host capabilities required by a resolved module.
    ///
    /// The graph sequences this transaction around module initialization, but
    /// the embedding host owns native registration and activation storage.
    ///
    /// # Errors
    ///
    /// Returns a checked error when the host cannot stage an activation lease.
    fn stage_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Box<dyn ModuleActivationTransaction>>, ModuleLoadError> {
        if source.activation.is_some() {
            return Err(ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: "module activation requires host support".into(),
            });
        }
        Ok(None)
    }

    /// Checks the `foreign` declarations of a module whose source carries an
    /// activation, after it compiles and before its activation registers.
    ///
    /// # Errors
    ///
    /// Returns a checked error when a declaration does not match the native
    /// binding the host will register for it.
    fn check_foreign_declarations(
        &self,
        _source: &ModuleSource,
        _module_name: &str,
        _declarations: &[ForeignDeclaration],
    ) -> Result<(), ModuleLoadError> {
        Ok(())
    }

    fn builtin_globals(&self) -> HashMap<String, Value>;

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
        // The resolver key, rather than the diagnostic label, is the stable
        // identity passed back to the host for runtime relative imports.
        compilation.program.set_module_key(source.key.as_str());
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

    /// Compiles an entry source and every literal import into a closed artifact.
    ///
    /// # Errors
    ///
    /// Returns checked loader or source errors when an entry or static dependency
    /// cannot be resolved and compiled.
    pub fn compile_artifact(
        &self,
        resolver: &dyn ModuleResolver,
        path: &str,
        source: &str,
    ) -> Result<Artifact, ModuleLoadError> {
        self.compile_artifact_with_sources(resolver, path, source)
            .map(|(artifact, _)| artifact)
    }

    /// Like [`Self::compile_artifact`], also returning each module's source
    /// text at its module index.
    pub(crate) fn compile_artifact_with_sources(
        &self,
        resolver: &dyn ModuleResolver,
        path: &str,
        source: &str,
    ) -> Result<(Artifact, Vec<ArtifactSource>), ModuleLoadError> {
        let mut modules = Vec::new();
        let mut indexes = HashMap::new();
        let mut imports = Vec::new();
        let mut sources = Vec::new();
        let key = ModuleKey::new(path);
        let root = self.collect_artifact_module(
            resolver,
            &key,
            path,
            path,
            source,
            &mut modules,
            &mut indexes,
            &mut imports,
            &mut sources,
        )?;
        let artifact = Artifact {
            root,
            modules,
            imports,
        };
        Ok((artifact, sources))
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_artifact_module(
        &self,
        resolver: &dyn ModuleResolver,
        key: &ModuleKey,
        name: &str,
        diagnostic_name: &str,
        source: &str,
        modules: &mut Vec<ArtifactModule>,
        indexes: &mut HashMap<ModuleKey, usize>,
        imports: &mut Vec<ImportEdge>,
        sources: &mut Vec<ArtifactSource>,
    ) -> Result<usize, ModuleLoadError> {
        if let Some(index) = indexes.get(key) {
            return Ok(*index);
        }
        let mut compilation = self
            .compile_source_for_module(
                resolver,
                key,
                diagnostic_name,
                source,
                name != "slug.builtin",
            )
            .map_err(|error| ModuleLoadError::Source {
                location: diagnostic_name.into(),
                message: error.to_string(),
            })?;
        compilation.program.set_module_name(name);
        compilation.program.set_module_key(key.as_str());
        self.state
            .semantic_snapshots
            .borrow_mut()
            .insert(key.clone(), compilation.snapshot);
        self.state
            .compiled
            .borrow_mut()
            .insert(key.clone(), compilation.program.clone());

        let index = modules.len();
        indexes.insert(key.clone(), index);
        modules.push(ArtifactModule {
            key: key.as_str().into(),
            name: name.into(),
            program: compilation.program,
        });
        sources.push(ArtifactSource {
            diagnostic_name: diagnostic_name.into(),
            text: source.into(),
        });
        for import_name in
            crate::source::static_import_names(diagnostic_name, source).map_err(|error| {
                ModuleLoadError::Source {
                    location: diagnostic_name.into(),
                    message: error.to_string(),
                }
            })?
        {
            let module_source = resolver.resolve(ModuleRequest::new(Some(key), &import_name))?;
            let target = self.collect_artifact_module(
                resolver,
                &module_source.key,
                &import_name,
                &module_source.diagnostic_name,
                &module_source.text,
                modules,
                indexes,
                imports,
                sources,
            )?;
            imports.push(ImportEdge {
                importer: index,
                name: import_name,
                target,
            });
        }
        Ok(index)
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

        let mut activation = host.stage_activation(&source)?;
        let program = match self
            .compile_cached_or_source(host, &source, request.name)
            .and_then(|program| {
                Self::check_foreign_declarations(host, &source, request.name)?;
                Ok(program)
            }) {
            Ok(program) => program,
            Err(error) => {
                if let Some(activation) = activation.as_mut() {
                    activation.cleanup();
                }
                return Err(error);
            }
        };
        if let Some(staged) = activation.as_ref()
            && let Err(error) = staged.register()
        {
            if let Some(activation) = activation.as_mut() {
                activation.cleanup();
            }
            return Err(error);
        }

        let mut vm = host.module_vm(program.bindings());
        let program = match vm.install_named(program, "main") {
            Ok(program) => program,
            Err(error) => {
                if let Some(staged) = activation.as_ref() {
                    staged.rollback();
                }
                if let Some(activation) = activation.as_mut() {
                    activation.cleanup();
                }
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
            exports: Value::map(Vec::new()),
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
                staged.rollback();
            }
            if let Some(activation) = activation.as_mut() {
                activation.cleanup();
            }
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
            activation.retain();
        }
        Ok(instance)
    }

    fn check_foreign_declarations<H: ModuleGraphHost>(
        host: &H,
        source: &ModuleSource,
        module_name: &str,
    ) -> Result<(), ModuleLoadError> {
        if source.activation.is_none() {
            return Ok(());
        }
        let declarations =
            foreign_declarations(&source.diagnostic_name, &source.text).map_err(|error| {
                ModuleLoadError::Source {
                    location: source.diagnostic_name.clone(),
                    message: error.to_string(),
                }
            })?;
        host.check_foreign_declarations(source, module_name, &declarations)
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
        compilation.program.set_module_key(source.key.as_str());
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
        let exports = Value::map(
            host.builtin_globals()
                .into_iter()
                .map(|(name, value)| (Value::string(name), value))
                .collect(),
        );
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
    pub fn with_resolver(
        resolver: Rc<dyn ModuleResolver>,
        configuration: Rc<dyn VmConfiguration>,
    ) -> Self {
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
    pub fn configuration(&self) -> &dyn VmConfiguration {
        self.state.services.configuration.as_ref()
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

    /// Compiles entry source and its resolved static imports into a closed artifact.
    ///
    /// # Errors
    ///
    /// Returns checked loader or source errors when a static dependency is unavailable.
    pub fn compile_artifact(&self, path: &str, source: &str) -> Result<Artifact, ModuleLoadError> {
        self.state.graph.compile_artifact(self, path, source)
    }

    /// Compiles entry source and its static imports and reports their capabilities.
    ///
    /// # Errors
    ///
    /// Returns checked loader or source errors when a static dependency is
    /// unavailable or invalid.
    pub fn capability_report(
        &self,
        path: &str,
        source: &str,
    ) -> Result<crate::CapabilityReport, ModuleLoadError> {
        self.state.graph.capability_report(self, path, source)
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
    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.builtin_globals()
    }

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

    fn configuration(&self) -> &dyn VmConfiguration {
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

#[cfg(test)]
mod tests {
    use super::{ArtifactHost, ModuleHost};
    use slug_loader::{ModuleKey, ModuleSource, StaticModuleProvider, StaticModuleResolver};
    use slug_vm::{EmptyVmConfiguration, Value, cslug};
    use std::rc::Rc;

    fn host() -> ModuleHost {
        let resolver = StaticModuleResolver::new([StaticModuleProvider::new(
            "child",
            ModuleSource {
                key: ModuleKey::new("memory:child"),
                diagnostic_name: "child.slug".into(),
                text: "export val value = 42\n".into(),
                activation: None,
            },
        )
        .expect("valid child provider")])
        .expect("valid resolver");
        ModuleHost::with_resolver(Rc::new(resolver), Rc::new(EmptyVmConfiguration))
    }

    #[test]
    fn executes_decoded_closed_artifact() {
        let artifact = host()
            .compile_artifact(
                "entry.slug",
                "val child = import(\"child\")\nval main = fn() { child.value }\n",
            )
            .expect("compile artifact");
        assert_eq!(artifact.modules.len(), 2);
        assert_eq!(artifact.imports.len(), 1);
        assert_eq!(artifact.imports[0].name, "child");

        let bytes = cslug::encode(&artifact).expect("encode artifact");
        let artifact = cslug::decode(&bytes, cslug::Limits::default()).expect("decode artifact");
        assert_eq!(
            ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
                .expect("valid artifact")
                .run()
                .expect("execute artifact"),
            Value::Int(42)
        );
    }

    #[test]
    fn rejects_imports_absent_from_closed_artifact() {
        let artifact = host()
            .compile_artifact(
                "entry.slug",
                "val name = \"missing\"\nval main = fn() { import(name) }\n",
            )
            .expect("compile artifact");
        let error = ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
            .expect("valid artifact")
            .run()
            .expect_err("missing artifact edge");
        assert!(error.to_string().contains("compiled artifact"));
    }
}
