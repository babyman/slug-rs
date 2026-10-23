//! Desktop external-import resolution and import-scoped activation.
//!
//! Callers provide already-selected project and library roots. This crate does
//! not discover process environment, command-line arguments, or configuration.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use slug_loader::{
    ForeignDeclaration, ModuleActivation as ModuleActivationLease, ModuleActivationTransaction,
    ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource, module_path,
    suggest_module_names,
};
use slug_vm::{NativeDescriptorError, NativeFunction, Value};
#[doc(hidden)]
pub mod clutch;
#[allow(unsafe_code)]
mod dynamic_library;
#[allow(unsafe_code)]
mod typed_native;

use clutch::StagedClutchPlugin;
pub use clutch::{
    ClutchPluginInitializer, ClutchPluginRegistrar, ClutchRepository, ClutchRepositoryError,
};
pub use dynamic_library::NativeLibraryError;
use slug_native_adapter::TextResults;

/// Filesystem resolver for an explicitly configured desktop module search path.
#[derive(Clone, Debug)]
pub struct DesktopResolver {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
    clutch_repository: ClutchRepository,
    activation_sources: Rc<RefCell<HashMap<ModuleActivationLease, ClutchPluginSource>>>,
    activation_state: Rc<RefCell<DesktopActivationState>>,
    /// Text-result switches of each staged typed activation.
    typed_text_results: Rc<RefCell<HashMap<ModuleActivationLease, TextResults>>>,
    /// Whether indexed clutches' test modules resolve, which only the user
    /// running slug enables.
    test_modules: Cell<bool>,
}

#[derive(Debug, Default)]
struct DesktopActivationState {
    foreign_functions: HashMap<(String, String), NativeFunction>,
    active_plugins: HashMap<ModuleActivationLease, DesktopActivation>,
    /// Activations registered while their module initializes, before
    /// `retain`. A sibling module imported during that initialization shares
    /// the activation instead of staging the clutch again.
    registered: HashSet<ModuleActivationLease>,
    shutdown_errors: Vec<String>,
}

impl Drop for DesktopActivationState {
    fn drop(&mut self) {
        let plugins = std::mem::take(&mut self.active_plugins);
        for (_, mut activation) in plugins {
            remove_foreign_batch(&mut self.foreign_functions, &activation.plugin.functions);
            if let Err(error) = activation.plugin.cleanup() {
                self.shutdown_errors.push(error);
            }
        }
    }
}

/// Staged desktop-native registrations owned by one imported Clutch module.
#[derive(Debug)]
struct DesktopActivation {
    resolver: DesktopResolver,
    lease: Option<ModuleActivationLease>,
    diagnostic_name: String,
    plugin: StagedClutchPlugin,
}

impl ModuleActivationTransaction for DesktopActivation {
    fn register(&self) -> Result<(), ModuleLoadError> {
        self.resolver
            .define_foreign_batch(self.plugin.functions.clone())
            .map_err(|error| ModuleLoadError::Clutch {
                location: self.diagnostic_name.clone(),
                message: error.to_string(),
            })?;
        if let Some(lease) = &self.lease {
            self.resolver
                .activation_state
                .borrow_mut()
                .registered
                .insert(lease.clone());
        }
        Ok(())
    }

    fn rollback(&self) {
        if let Some(lease) = &self.lease {
            self.resolver
                .activation_state
                .borrow_mut()
                .registered
                .remove(lease);
        }
        remove_foreign_batch(
            &mut self
                .resolver
                .activation_state
                .borrow_mut()
                .foreign_functions,
            &self.plugin.functions,
        );
    }

    fn cleanup(&mut self) {
        if let Some(lease) = &self.lease {
            self.resolver
                .activation_state
                .borrow_mut()
                .registered
                .remove(lease);
        }
        if let Err(error) = self.plugin.cleanup() {
            self.resolver
                .activation_state
                .borrow_mut()
                .shutdown_errors
                .push(error);
        }
    }

    fn retain(self: Box<Self>) {
        let mut activation = *self;
        let Some(lease) = activation.lease.take() else {
            activation.cleanup();
            return;
        };
        let state = activation.resolver.activation_state.clone();
        let mut state = state.borrow_mut();
        state.registered.remove(&lease);
        state.active_plugins.insert(lease, activation);
    }
}

impl DesktopResolver {
    /// Creates a resolver using caller-selected project and library roots.
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self::with_clutch_repository(source_root, library_root, ClutchRepository::default())
    }

    /// Creates a resolver with an explicit desktop Clutch repository.
    #[must_use]
    pub fn with_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        clutch_repository: ClutchRepository,
    ) -> Self {
        Self {
            source_root: source_root.into(),
            library_root,
            clutch_repository,
            activation_sources: Rc::new(RefCell::new(HashMap::new())),
            activation_state: Rc::new(RefCell::new(DesktopActivationState::default())),
            typed_text_results: Rc::new(RefCell::new(HashMap::new())),
            test_modules: Cell::new(false),
        }
    }

    /// Lets indexed clutches' test modules resolve. They see their clutch's
    /// unexported declarations, so a host enables them only when its user
    /// grants that.
    pub fn enable_test_modules(&self) {
        self.test_modules.set(true);
    }

    /// Stages the native registrations required by a resolved module.
    ///
    /// # Errors
    ///
    /// Returns a checked error when an activation lease is invalid or its
    /// plugin cannot be prepared.
    fn stage_module_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<DesktopActivation>, ModuleLoadError> {
        let Some(lease) = &source.activation else {
            return Ok(None);
        };
        {
            let state = self.activation_state.borrow();
            if state.active_plugins.contains_key(lease) || state.registered.contains(lease) {
                return Ok(None);
            }
        }
        let plugin = self
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
                let initializer = self.clutch_repository.plugin(entry).ok_or_else(|| {
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
                root,
                library,
                manifest,
                ..
            } => self.stage_typed(lease, root, library, manifest, &mut registrar),
        };
        if let Err(error) = result {
            let mut staged = registrar.finish();
            let _ = staged.cleanup();
            return Err(error);
        }
        Ok(Some(DesktopActivation {
            resolver: self.clone(),
            lease: source.activation.clone(),
            diagnostic_name: source.diagnostic_name.clone(),
            plugin: registrar.finish(),
        }))
    }

    fn stage_typed(
        &self,
        lease: &ModuleActivationLease,
        root: &Path,
        library: &Path,
        manifest: &slug_native_manifest::NativeManifest,
        registrar: &mut clutch::ClutchPluginRegistrar,
    ) -> Result<(), ModuleLoadError> {
        let installed =
            typed_native::load(library, manifest).map_err(|message| ModuleLoadError::Clutch {
                location: library.to_string_lossy().into_owned(),
                message: format!("cannot load typed native library: {message}"),
            })?;
        self.typed_text_results
            .borrow_mut()
            .insert(lease.clone(), installed.text_results);
        let clutch_error = |message: String| ModuleLoadError::Clutch {
            location: root.to_string_lossy().into_owned(),
            message,
        };
        installed.functions.into_iter().try_for_each(|function| {
            registrar
                .define_foreign(function)
                .map_err(|error| clutch_error(error.to_string()))
        })?;
        // Functions a VM still holds after shutdown must not enter the library.
        let activity = installed.activity;
        registrar
            .set_cleanup_operation(move || {
                activity.deactivate();
                Ok(())
            })
            .map_err(|error| clutch_error(format!("cannot retain native plugin cleanup: {error}")))
    }

    /// Checks a clutch module's `foreign` declarations against its typed
    /// native bindings. Modules of other clutch forms are not checked.
    ///
    /// # Errors
    ///
    /// Returns a checked clutch error naming the first mismatched declaration.
    pub fn check_foreign_declarations(
        &self,
        source: &ModuleSource,
        module_name: &str,
        declarations: &[ForeignDeclaration],
    ) -> Result<(), ModuleLoadError> {
        let Some(lease) = &source.activation else {
            return Ok(());
        };
        let Some(ClutchPluginSource::Native { manifest, .. }) =
            self.activation_sources.borrow().get(lease).cloned()
        else {
            return Ok(());
        };
        let non_null_text =
            slug_native_manifest::check_foreign_declarations(&manifest, module_name, declarations)
                .map_err(|message| ModuleLoadError::Clutch {
                    location: source.diagnostic_name.clone(),
                    message,
                })?;
        if let Some(text_results) = self.typed_text_results.borrow().get(lease) {
            for name in non_null_text {
                // The check above found this binding in the same manifest.
                let _ = text_results.reject_null(module_name, &name);
            }
        }
        Ok(())
    }

    /// Returns plugin-provided functions for one module-qualified declaration.
    #[must_use]
    pub fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.activation_state
            .borrow()
            .foreign_functions
            .get(&(module.into(), name.into()))
            .cloned()
    }

    /// Returns native functions exported through the virtual builtin module.
    #[must_use]
    pub fn builtin_globals(&self) -> HashMap<String, Value> {
        self.activation_state
            .borrow()
            .foreign_functions
            .iter()
            .filter(|((module, _), _)| module == "slug.builtin")
            .map(|((_, name), function)| (name.clone(), Value::Native(function.clone())))
            .collect()
    }

    /// Registers foreign declarations atomically for the selected desktop host.
    ///
    /// # Errors
    ///
    /// Returns a descriptor error without retaining any supplied function when
    /// a declaration conflicts with an existing binding.
    pub fn define_foreign_batch(
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
        let mut state = self.activation_state.borrow_mut();
        for (index, key) in keys.iter().enumerate() {
            if state.foreign_functions.contains_key(key) || keys[..index].contains(key) {
                return Err(NativeDescriptorError::new(format!(
                    "foreign binding `{}.{}` is already defined",
                    key.0, key.1
                )));
            }
        }
        for (key, function) in keys.into_iter().zip(functions) {
            state.foreign_functions.insert(key, function);
        }
        Ok(())
    }

    /// Finalizes retained desktop-native activations.
    pub fn shutdown(&self) {
        let mut state = self.activation_state.borrow_mut();
        let plugins = std::mem::take(&mut state.active_plugins);
        for (_, mut activation) in plugins {
            remove_foreign_batch(&mut state.foreign_functions, &activation.plugin.functions);
            if let Err(error) = activation.plugin.cleanup() {
                state.shutdown_errors.push(error);
            }
        }
    }

    /// Resolves `name` when it is an indexed clutch's test module, which
    /// requires test modules to be enabled.
    fn resolve_test_module(&self, name: &str) -> Option<Result<ModuleSource, ModuleLoadError>> {
        let path = self.clutch_repository.test_module(name)?;
        if !self.test_modules.get() {
            return Some(Err(ModuleLoadError::Clutch {
                location: name.into(),
                message: format!(
                    "`{name}` is a clutch test module, which resolves only when slug runs with --allow-unexported"
                ),
            }));
        }
        Some(
            path.map_err(|message| ModuleLoadError::Clutch {
                location: name.into(),
                message,
            })
            .and_then(|path| {
                let text = fs::read_to_string(&path).map_err(|error| ModuleLoadError::Read {
                    location: path.to_string_lossy().into_owned(),
                    message: error.to_string(),
                })?;
                Ok(ModuleSource {
                    key: ModuleKey::new(path.to_string_lossy()),
                    diagnostic_name: path.to_string_lossy().into_owned(),
                    text,
                    activation: None,
                })
            }),
        )
    }
}

fn remove_foreign_batch(
    registry: &mut HashMap<(String, String), NativeFunction>,
    functions: &[NativeFunction],
) {
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

impl ModuleResolver for DesktopResolver {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        let relative = module_path(request.name)?;
        let mut candidates = Vec::new();
        // A bare importer file name has the empty path as its parent, which
        // names no directory; it means the current directory.
        if let Some(importer) = request
            .importer
            .map(ModuleKey::as_str)
            .map(Path::new)
            .and_then(Path::parent)
            .map(|parent| {
                if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                }
            })
        {
            candidates.push((importer, importer.join(&relative)));
        }
        candidates.push((&self.source_root, self.source_root.join(&relative)));
        if let Some(library_root) = &self.library_root {
            candidates.push((library_root, library_root.join(&relative)));
        }
        for (root, path) in &candidates {
            match read_contained_source(root, path) {
                Ok(Some((path, text))) => {
                    return Ok(ModuleSource {
                        key: ModuleKey::new(path.to_string_lossy()),
                        diagnostic_name: path.to_string_lossy().into_owned(),
                        text,
                        activation: None,
                    });
                }
                Err(error) => {
                    return Err(error);
                }
                Ok(None) => {}
            }
        }
        if let Some(root) = self.clutch_repository.provider(request.name) {
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
                    module_names: module.host_plugin_module_names,
                }),
                (None, Some(native)) => Some(ClutchPluginSource::Native {
                    root: module.root,
                    library: native.library,
                    manifest: native.manifest,
                    module_names: module.module_names,
                }),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("clutch manifest validation is inconsistent"),
            };
            let lease = activation.as_ref().map(|plugin| {
                let lease = ModuleActivationLease::new(plugin.activation_identity());
                self.activation_sources
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
        if let Some(source) = self.resolve_test_module(request.name) {
            return source;
        }
        Err(self.not_found(request.name, &candidates, &relative))
    }
}

/// How many directories above a searched one to look for a misplaced module.
const MODULE_SEARCH_ANCESTORS: usize = 4;

/// The module's file in a directory above any searched root, where a program
/// in a subdirectory, such as `tests/`, might expect its import to find it.
/// Each is relative to the root it is above, such as `../jobs.slug`.
fn modules_above<'a>(roots: impl Iterator<Item = &'a Path>, relative: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for root in roots {
        // A relative root such as `.` has no ancestors of its own.
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let mut up = PathBuf::new();
        for ancestor in root.ancestors().skip(1).take(MODULE_SEARCH_ANCESTORS) {
            up.push("..");
            if ancestor.join(relative).is_file() {
                let shown = up.join(relative).to_string_lossy().into_owned();
                if !found.contains(&shown) {
                    found.push(shown);
                }
                break;
            }
        }
    }
    found
}

impl DesktopResolver {
    /// The not-found error for `name`, with close module names and any file
    /// of that name above the directories searched.
    fn not_found(
        &self,
        name: &str,
        candidates: &[(&Path, PathBuf)],
        relative: &Path,
    ) -> ModuleLoadError {
        let local = local_module_names(candidates);
        let library_root = self.library_root.as_deref();
        let elsewhere = modules_above(
            candidates
                .iter()
                .filter(|(root, _)| Some(*root) != library_root)
                .map(|(root, _)| *root),
            relative,
        );
        ModuleLoadError::NotFound {
            name: name.into(),
            searched: candidates
                .iter()
                .map(|(_, path)| path.to_string_lossy().into_owned())
                .collect(),
            suggestions: suggest_module_names(
                name,
                local
                    .iter()
                    .map(String::as_str)
                    .chain(self.clutch_repository.module_names()),
            ),
            elsewhere,
        }
    }
}

/// The modules in each searched root's top directory and in the directory
/// each search looked in, as candidates for a not-found suggestion. Reading
/// only those directories keeps a failed import from walking a source tree.
fn local_module_names(candidates: &[(&Path, PathBuf)]) -> Vec<String> {
    let mut names = Vec::new();
    for (root, path) in candidates {
        let mut directories = vec![root.to_path_buf()];
        if let Some(parent) = path.parent()
            && parent != *root
        {
            directories.push(parent.to_path_buf());
        }
        for directory in directories {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "slug") {
                    continue;
                }
                let Ok(relative) = path
                    .with_extension("")
                    .strip_prefix(root)
                    .map(Path::to_path_buf)
                else {
                    continue;
                };
                let name = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(".");
                if module_path(&name).is_ok() {
                    names.push(name);
                }
            }
        }
    }
    names
}

impl DesktopResolver {
    /// Stages the native registrations required by a resolved module.
    ///
    /// The desktop host calls this while it owns the module initialization
    /// lifecycle and native-function registry.
    ///
    /// # Errors
    ///
    /// Returns a checked error when an activation lease cannot be staged.
    pub fn stage_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Box<dyn ModuleActivationTransaction>>, ModuleLoadError> {
        self.stage_module_activation(source).map(|activation| {
            activation
                .map(|activation| Box::new(activation) as Box<dyn ModuleActivationTransaction>)
        })
    }
}

/// Reads a source candidate only after resolving both it and its selected
/// lookup root. This prevents a symlink below a project, library, or
/// importer-relative directory from widening the configured import boundary.
fn read_contained_source(
    root: &Path,
    path: &Path,
) -> Result<Option<(PathBuf, String)>, ModuleLoadError> {
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ModuleLoadError::Read {
                location: path.to_string_lossy().into_owned(),
                message: error.to_string(),
            });
        }
    };
    let canonical_root = fs::canonicalize(root).map_err(|error| ModuleLoadError::Read {
        location: root.to_string_lossy().into_owned(),
        message: error.to_string(),
    })?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ModuleLoadError::Read {
            location: path.to_string_lossy().into_owned(),
            message: format!(
                "module path escapes configured import root {}",
                canonical_root.display()
            ),
        });
    }
    let text = fs::read_to_string(&canonical).map_err(|error| ModuleLoadError::Read {
        location: canonical.to_string_lossy().into_owned(),
        message: error.to_string(),
    })?;
    Ok(Some((canonical, text)))
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
        manifest: Rc<slug_native_manifest::NativeManifest>,
        module_names: Vec<String>,
    },
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

    fn activation_identity(&self) -> String {
        let root = self.root().to_string_lossy();
        match self {
            Self::Host { entry, .. } => format!("host\0{root}\0{entry}"),
            Self::Native { .. } => format!("native\0{root}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use super::{ClutchRepository, DesktopResolver};
    use slug_loader::{ModuleKey, ModuleRequest, ModuleResolver};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "slug-desktop-resolver-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn resolves_importer_relative_then_project_then_library_modules() {
        let root = root();
        let project = root.join("project");
        let library = root.join("library");
        fs::create_dir_all(project.join("nested")).expect("create project directory");
        fs::create_dir_all(&library).expect("create library directory");
        fs::write(project.join("nested/math.slug"), "relative").expect("write relative module");
        fs::write(project.join("fallback.slug"), "project").expect("write project module");
        fs::write(library.join("library.slug"), "library").expect("write library module");

        let resolver = DesktopResolver::new(&project, Some(library.clone()));
        let importer = ModuleKey::new(project.join("nested/main.slug").to_string_lossy());
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(Some(&importer), "math"))
                .expect("resolve relative module")
                .text,
            "relative"
        );
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(None, "fallback"))
                .expect("resolve project module")
                .text,
            "project"
        );
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(None, "library"))
                .expect("resolve library module")
                .text,
            "library"
        );
        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }

    #[test]
    fn resolves_explicit_clutch_modules_with_activation_leases() {
        let root = root();
        let clutch = root.join("example.clutch");
        fs::create_dir_all(clutch.join("modules")).expect("create clutch module directory");
        fs::write(
            clutch.join("modules/library.slug"),
            "export val answer = 42\n",
        )
        .expect("write clutch module");
        fs::write(
            clutch.join("clutch.toml"),
            "[modules]\n\"example.library\" = { source = \"modules/library.slug\", plugin = \"test.plugin\" }\n",
        )
        .expect("write clutch manifest");
        let repository = ClutchRepository::new(vec![("example.library".into(), clutch)])
            .expect("create clutch repository");
        let resolver = DesktopResolver::with_clutch_repository(&root, None, repository);

        let source = resolver
            .resolve(ModuleRequest::new(None, "example.library"))
            .expect("resolve clutch module");

        assert_eq!(source.text, "export val answer = 42\n");
        assert!(source.activation.is_some());
        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }

    #[test]
    fn resolves_declared_clutch_test_modules_only_when_enabled() {
        let root = root();
        let repository_root = root.join("repository");
        let clutch = repository_root.join("demo.clutch");
        fs::create_dir_all(clutch.join("modules")).expect("create clutch module directory");
        fs::create_dir_all(clutch.join("tests/unit")).expect("create clutch test directory");
        fs::write(clutch.join("modules/lib.slug"), "export val answer = 42\n")
            .expect("write clutch module");
        fs::write(
            clutch.join("tests/unit/lib.slug"),
            "export val checked = true\n",
        )
        .expect("write clutch test");
        fs::write(
            repository_root.join("manifest.toml"),
            "[modules]\n\"demo.lib\" = \"demo.clutch\"\n",
        )
        .expect("write repository manifest");
        let manifest = |files: &str| {
            fs::write(
                clutch.join("clutch.toml"),
                format!(
                    "[modules]\n\"demo.lib\" = {{ source = \"modules/lib.slug\" }}\n\n[tests]\nfiles = [{files}]\n"
                ),
            )
            .expect("write clutch manifest");
        };

        // A test file's name is the clutch's, then its path, without `.slug`.
        manifest("\"tests/unit/lib.slug\"");
        let repository =
            ClutchRepository::from_manifest(&repository_root).expect("load clutch repository");
        let resolver = DesktopResolver::with_clutch_repository(&root, None, repository);
        let error = resolver
            .resolve(ModuleRequest::new(None, "demo.tests.unit.lib"))
            .expect_err("test modules are not enabled");
        assert!(
            error.to_string().contains(
                "`demo.tests.unit.lib` is a clutch test module, which resolves only when slug runs with --allow-unexported"
            ),
            "{error}"
        );
        resolver.enable_test_modules();
        let source = resolver
            .resolve(ModuleRequest::new(None, "demo.tests.unit.lib"))
            .expect("resolve test module");
        assert_eq!(source.text, "export val checked = true\n");
        assert!(source.activation.is_none());

        // A missing test file is reported when the module resolves, not when
        // the repository loads, so it does not break other programs.
        manifest("\"tests/missing.slug\"");
        let repository =
            ClutchRepository::from_manifest(&repository_root).expect("load clutch repository");
        let resolver = DesktopResolver::with_clutch_repository(&root, None, repository);
        resolver.enable_test_modules();
        let error = resolver
            .resolve(ModuleRequest::new(None, "demo.tests.missing"))
            .expect_err("the test file is missing");
        assert!(
            error.to_string().contains("cannot access test file"),
            "{error}"
        );

        // Invalid declarations are manifest errors.
        for (files, message) in [
            (
                "\"../escape.slug\"",
                "test file `../escape.slug` escapes the clutch root",
            ),
            (
                "\"tests/notes.txt\"",
                "test file `tests/notes.txt` must name a `.slug` file",
            ),
            (
                "\"tests/bad-name.slug\"",
                "would be module `demo.tests.bad-name`",
            ),
            (
                "\"lib.slug\"",
                "module `demo.lib` has more than one clutch provider",
            ),
        ] {
            manifest(files);
            let error = ClutchRepository::from_manifest(&repository_root)
                .expect_err("invalid test declaration");
            assert!(error.to_string().contains(message), "{files}: {error}");
        }
        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_modules_outside_configured_roots() {
        let root = root();
        let project = root.join("project");
        let library = root.join("library");
        let outside = root.join("outside");
        fs::create_dir_all(&project).expect("create project directory");
        fs::create_dir_all(project.join("nested")).expect("create importer directory");
        fs::create_dir_all(&library).expect("create library directory");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(outside.join("secret.slug"), "export val secret = 42\n")
            .expect("write outside module");
        symlink(&outside, project.join("project_escape")).expect("link project escape");
        symlink(&outside, project.join("nested/importer_escape")).expect("link importer escape");
        symlink(&outside, library.join("library_escape")).expect("link library escape");

        let resolver = DesktopResolver::new(&project, Some(library));
        for name in ["project_escape.secret", "library_escape.secret"] {
            let error = resolver
                .resolve(ModuleRequest::new(None, name))
                .expect_err("reject configured-root symlink escape");
            assert!(matches!(error, super::ModuleLoadError::Read { .. }));
            assert!(error.to_string().contains("escapes configured import root"));
        }
        let importer = ModuleKey::new(project.join("nested/main.slug").to_string_lossy());
        let error = resolver
            .resolve(ModuleRequest::new(
                Some(&importer),
                "importer_escape.secret",
            ))
            .expect_err("reject importer-relative symlink escape");
        assert!(matches!(error, super::ModuleLoadError::Read { .. }));
        assert!(error.to_string().contains("escapes configured import root"));

        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }
}
