use slug_native_manifest::NativeManifest;
use slug_vm::{NativeDescriptorError, NativeFunction};
use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

/// An explicit host-owned mapping from module names to exploded clutch roots.
///
/// Hosts may construct this mapping directly or load it from a local clutch
/// repository manifest.
/// A clutch root and the path of one of its test files within it.
type TestFile = (PathBuf, String);

#[derive(Clone, Default)]
pub struct ClutchRepository {
    providers: HashMap<String, PathBuf>,
    /// Each indexed clutch's test files, by import name: the clutch root and
    /// the file's path in it, which is read only when the module resolves.
    tests: HashMap<String, TestFile>,
    plugin_initializers: HashMap<String, ClutchPluginInitializer>,
}

impl fmt::Debug for ClutchRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClutchRepository")
            .field("providers", &self.providers)
            .field("tests", &self.tests)
            .field("plugin_initializer_count", &self.plugin_initializers.len())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClutchRepositoryError {
    InvalidModuleName(String),
    DuplicateProvider { name: String },
    DuplicatePlugin { entry: String },
    Read { path: PathBuf, message: String },
    Manifest { path: PathBuf, message: String },
}

impl fmt::Display for ClutchRepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModuleName(name) => write!(f, "invalid module name `{name}`"),
            Self::DuplicateProvider { name } => {
                write!(f, "module `{name}` has more than one clutch provider")
            }
            Self::DuplicatePlugin { entry } => {
                write!(f, "clutch plugin entry `{entry}` is already defined")
            }
            Self::Read { path, message } => {
                write!(
                    f,
                    "cannot read clutch repository {}: {message}",
                    path.display()
                )
            }
            Self::Manifest { path, message } => {
                write!(f, "invalid clutch manifest {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ClutchRepositoryError {}

impl ClutchRepository {
    /// Creates an explicit repository index without inspecting clutch contents.
    ///
    /// # Errors
    ///
    /// Returns an error when a module name is invalid or appears more than once.
    pub fn new(
        providers: impl IntoIterator<Item = (String, PathBuf)>,
    ) -> Result<Self, ClutchRepositoryError> {
        let mut indexed = HashMap::new();
        for (name, path) in providers {
            if !valid_module_name(&name) {
                return Err(ClutchRepositoryError::InvalidModuleName(name));
            }
            if indexed.insert(name.clone(), path).is_some() {
                return Err(ClutchRepositoryError::DuplicateProvider { name });
            }
        }
        Ok(Self {
            providers: indexed,
            tests: HashMap::new(),
            plugin_initializers: HashMap::new(),
        })
    }

    /// Loads providers explicitly selected by `root/manifest.toml`.
    ///
    /// The repository manifest maps each module identity to a direct relative
    /// `.clutch` directory. The selected clutch must in turn declare that
    /// identity in its own manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when either manifest cannot be read or is invalid, or
    /// an indexed clutch is absent or disagrees with the repository index.
    pub fn from_manifest(root: impl AsRef<Path>) -> Result<Self, ClutchRepositoryError> {
        let root =
            fs::canonicalize(root.as_ref()).map_err(|error| ClutchRepositoryError::Read {
                path: root.as_ref().into(),
                message: error.to_string(),
            })?;
        if !root.is_dir() {
            return Err(ClutchRepositoryError::Read {
                path: root,
                message: "clutch repository must be a directory".into(),
            });
        }
        let repository_manifest_path = root.join("manifest.toml");
        let repository_source = fs::read_to_string(&repository_manifest_path).map_err(|error| {
            ClutchRepositoryError::Read {
                path: repository_manifest_path.clone(),
                message: error.to_string(),
            }
        })?;
        let indexed = parse_repository_manifest(&repository_source).map_err(|message| {
            ClutchRepositoryError::Manifest {
                path: repository_manifest_path,
                message,
            }
        })?;
        let mut providers = HashMap::new();
        let mut tests = HashMap::new();
        let mut clutches = HashSet::new();
        for (name, directory) in indexed {
            let path = clutch_directory(&root, &directory).map_err(|message| {
                ClutchRepositoryError::Manifest {
                    path: root.join("manifest.toml"),
                    message,
                }
            })?;
            let manifest_path = path.join("clutch.toml");
            let source = fs::read_to_string(&manifest_path).map_err(|error| {
                ClutchRepositoryError::Read {
                    path: manifest_path.clone(),
                    message: error.to_string(),
                }
            })?;
            let manifest =
                parse_manifest(&source).map_err(|message| ClutchRepositoryError::Manifest {
                    path: manifest_path,
                    message,
                })?;
            if !manifest.modules.contains_key(&name) {
                return Err(ClutchRepositoryError::Manifest {
                    path: path.join("clutch.toml"),
                    message: format!("clutch does not provide indexed module `{name}`"),
                });
            }
            if clutches.insert(path.clone()) {
                for (test, file) in test_modules(&path, &manifest.tests).map_err(|message| {
                    ClutchRepositoryError::Manifest {
                        path: path.join("clutch.toml"),
                        message,
                    }
                })? {
                    if tests.insert(test.clone(), file).is_some() {
                        return Err(ClutchRepositoryError::DuplicateProvider { name: test });
                    }
                }
            }
            if providers.insert(name.clone(), path).is_some() {
                return Err(ClutchRepositoryError::DuplicateProvider { name });
            }
        }
        if let Some(name) = tests.keys().find(|test| providers.contains_key(*test)) {
            return Err(ClutchRepositoryError::DuplicateProvider { name: name.clone() });
        }
        Ok(Self {
            providers,
            tests,
            plugin_initializers: HashMap::new(),
        })
    }

    /// Registers one host-provided initializer for a manifest plugin entry.
    ///
    /// The entry is an opaque host configuration key. It is never interpreted
    /// as a native-library path by the clutch loader.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry is empty or already configured.
    pub fn define_plugin(
        &mut self,
        entry: impl Into<String>,
        initializer: impl Fn(&mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> + 'static,
    ) -> Result<(), ClutchRepositoryError> {
        let entry = entry.into();
        if entry.trim().is_empty() || self.plugin_initializers.contains_key(&entry) {
            return Err(ClutchRepositoryError::DuplicatePlugin { entry });
        }
        self.plugin_initializers.insert(entry, Rc::new(initializer));
        Ok(())
    }

    #[must_use]
    #[doc(hidden)]
    pub fn provider(&self, name: &str) -> Option<&PathBuf> {
        self.providers.get(name)
    }

    /// The root of the indexed clutch whose directory holds `path`, such as
    /// a module's source file, a test file, or a program in the clutch.
    #[must_use]
    pub fn clutch_containing(&self, path: &Path) -> Option<PathBuf> {
        // An entry program's source need not be a file, but its directory is.
        let path = fs::canonicalize(path).ok().or_else(|| {
            let parent = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            Some(fs::canonicalize(parent).ok()?.join(path.file_name()?))
        })?;
        self.providers
            .values()
            .filter_map(|root| fs::canonicalize(root).ok())
            .find(|root| path.starts_with(root))
    }

    /// The names of every module a clutch in this repository provides.
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    /// The file of the test module an indexed clutch declares under `name`,
    /// or `None` when no indexed clutch declares one. The inner error reports
    /// a declared file that escapes its clutch or cannot be accessed.
    #[must_use]
    pub fn test_module(&self, name: &str) -> Option<Result<PathBuf, String>> {
        let (root, file) = self.tests.get(name)?;
        Some(
            source_path(root, file)
                .map_err(|message| message.replace("module source", "test file")),
        )
    }

    #[must_use]
    #[doc(hidden)]
    pub fn plugin(&self, entry: &str) -> Option<ClutchPluginInitializer> {
        self.plugin_initializers.get(entry).cloned()
    }
}

fn parse_repository_manifest(source: &str) -> Result<HashMap<String, String>, String> {
    let value = source
        .parse::<toml::Value>()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "repository manifest root must be a table".to_string())?;
    require_keys(table, &["modules"], "repository manifest")?;
    let modules = table_value(table, "modules", "repository manifest")?;
    let modules = modules
        .as_table()
        .ok_or_else(|| "`modules` in repository manifest must be a table".to_string())?;
    if modules.is_empty() {
        return Err("`modules` in repository manifest must provide at least one module".into());
    }
    let mut indexed = HashMap::new();
    for (name, directory) in modules {
        if !valid_module_name(name) {
            return Err(format!("invalid module name `{name}`"));
        }
        let directory = directory.as_str().ok_or_else(|| {
            format!("repository module `{name}` must name a clutch directory string")
        })?;
        validate_clutch_directory(directory)?;
        indexed.insert(name.clone(), directory.to_owned());
    }
    Ok(indexed)
}

fn clutch_directory(root: &Path, directory: &str) -> Result<PathBuf, String> {
    validate_clutch_directory(directory)?;
    let path = root.join(directory);
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot access clutch directory {}: {error}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "clutch directory `{directory}` escapes the repository root"
        ));
    }
    if !canonical.is_dir() {
        return Err(format!("clutch directory `{directory}` is not a directory"));
    }
    Ok(canonical)
}

fn validate_clutch_directory(directory: &str) -> Result<(), String> {
    let path = Path::new(directory);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("clutch")
    {
        return Err(format!(
            "repository clutch directory `{directory}` must be a direct relative `.clutch` directory"
        ));
    }
    Ok(())
}

/// Initializes one manifest-selected plugin within one clutch-owned scope.
pub type ClutchPluginInitializer =
    Rc<dyn Fn(&mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError>>;

/// A clutch-bound, staged registrar exposed to experimental Rust clutch plugins.
pub struct ClutchPluginRegistrar {
    module_names: HashSet<String>,
    primary_module_name: String,
    functions: Vec<NativeFunction>,
    cleanup: Option<Box<dyn FnOnce() -> Result<(), String>>>,
}

impl ClutchPluginRegistrar {
    #[doc(hidden)]
    pub fn new(module_names: impl IntoIterator<Item = String>) -> Self {
        let module_names = module_names.into_iter().collect::<HashSet<_>>();
        let primary_module_name = module_names
            .iter()
            .next()
            .cloned()
            .expect("clutch plugin registrar requires at least one module");
        Self {
            module_names,
            primary_module_name,
            functions: Vec::new(),
            cleanup: None,
        }
    }

    /// Returns whether this clutch owns a module identity.
    #[must_use]
    pub fn serves_module(&self, module_name: &str) -> bool {
        self.module_names.contains(module_name)
    }

    /// Returns this registrar's primary module for legacy single-module host plugins.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.primary_module_name
    }

    /// Stages a module-qualified foreign function for publication.
    ///
    /// # Errors
    ///
    /// Returns an error when the function belongs to another module or
    /// duplicates an already staged function name.
    pub fn define_foreign(
        &mut self,
        function: NativeFunction,
    ) -> Result<(), NativeDescriptorError> {
        if !self.serves_module(function.module_name()) {
            return Err(NativeDescriptorError::new(format!(
                "clutch plugin cannot register foreign function `{}` outside its modules",
                function.qualified_name()
            )));
        }
        if self.functions.iter().any(|existing| {
            existing.module_name() == function.module_name() && existing.name() == function.name()
        }) {
            return Err(NativeDescriptorError::new(format!(
                "clutch plugin foreign function `{}.{}` is already staged",
                function.module_name(),
                function.name()
            )));
        }
        self.functions.push(function);
        Ok(())
    }

    /// Sets the one cleanup hook run when a staged plugin fails or its loader drops.
    ///
    /// # Errors
    ///
    /// Returns an error when a cleanup hook is already registered.
    pub fn set_cleanup(&mut self, cleanup: fn()) -> Result<(), NativeDescriptorError> {
        self.set_cleanup_operation(move || {
            cleanup();
            Ok(())
        })
    }

    /// Sets an owned one-shot cleanup operation for this plugin.
    ///
    /// The operation runs once on rollback or shutdown. Its failure is
    /// recorded without preventing other plugins from being finalized.
    ///
    /// # Errors
    ///
    /// Returns an error when a cleanup operation has already been registered.
    pub fn set_cleanup_operation(
        &mut self,
        cleanup: impl FnOnce() -> Result<(), String> + 'static,
    ) -> Result<(), NativeDescriptorError> {
        if self.cleanup.replace(Box::new(cleanup)).is_some() {
            return Err(NativeDescriptorError::new(
                "clutch plugin cleanup hook is already registered",
            ));
        }
        Ok(())
    }

    #[doc(hidden)]
    #[must_use]
    pub fn finish(self) -> StagedClutchPlugin {
        StagedClutchPlugin {
            functions: self.functions,
            cleanup: self.cleanup,
        }
    }
}

#[doc(hidden)]
pub struct StagedClutchPlugin {
    pub functions: Vec<NativeFunction>,
    cleanup: Option<Box<dyn FnOnce() -> Result<(), String>>>,
}

impl fmt::Debug for StagedClutchPlugin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagedClutchPlugin")
            .field("functions", &self.functions)
            .field("has_cleanup", &self.cleanup.is_some())
            .finish()
    }
}

impl StagedClutchPlugin {
    #[doc(hidden)]
    pub fn cleanup(&mut self) -> Result<(), String> {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup()
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct ClutchModule {
    pub root: PathBuf,
    pub path: PathBuf,
    pub plugin_entry: Option<String>,
    pub native_plugin: Option<NativeClutchPlugin>,
    pub host_plugin_module_names: Vec<String>,
    pub module_names: Vec<String>,
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct NativeClutchPlugin {
    pub library: PathBuf,
    /// The clutch's typed bindings, called through a generated shim.
    pub manifest: Rc<NativeManifest>,
}

#[doc(hidden)]
pub fn load_module(root: &Path, name: &str) -> Result<ClutchModule, String> {
    if root.extension().and_then(|extension| extension.to_str()) != Some("clutch") {
        return Err("clutch root must end in `.clutch`".into());
    }
    let root = fs::canonicalize(root)
        .map_err(|error| format!("cannot access clutch root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!("clutch root {} is not a directory", root.display()));
    }
    let manifest_path = root.join("clutch.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest = parse_manifest(&manifest)?;
    let module = manifest
        .modules
        .get(name)
        .ok_or_else(|| format!("clutch does not provide indexed module `{name}`"))?;
    let source_path = source_path(&root, &module.source)?;
    let native_plugin = manifest
        .native
        .as_ref()
        .map(|native| {
            Ok::<NativeClutchPlugin, String>(NativeClutchPlugin {
                library: native_library_path(&root, &native.libraries)?,
                manifest: native.manifest.clone(),
            })
        })
        .transpose()?;
    let host_plugin_module_names = module.plugin_entry.as_ref().map_or_else(Vec::new, |entry| {
        manifest
            .modules
            .iter()
            .filter(|(_, module)| module.plugin_entry.as_ref() == Some(entry))
            .map(|(name, _)| name.clone())
            .collect()
    });
    Ok(ClutchModule {
        root,
        path: source_path,
        plugin_entry: module.plugin_entry.clone(),
        native_plugin,
        host_plugin_module_names,
        module_names: manifest.modules.into_keys().collect(),
    })
}

#[derive(Debug)]
struct ClutchManifest {
    modules: HashMap<String, ClutchModuleEntry>,
    native: Option<NativeClutch>,
    /// The clutch's test files, relative to its root, from `[tests] files`.
    tests: Vec<String>,
}

#[derive(Debug)]
struct ClutchModuleEntry {
    source: String,
    plugin_entry: Option<String>,
}

#[derive(Debug)]
struct NativeClutch {
    manifest: Rc<NativeManifest>,
    libraries: HashMap<String, String>,
}

fn parse_manifest(source: &str) -> Result<ClutchManifest, String> {
    let value = source
        .parse::<toml::Value>()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "manifest root must be a table".to_string())?;
    require_keys(table, &["modules", "native", "tests"], "manifest")?;
    let modules = table_value(table, "modules", "manifest")?;
    let modules = modules
        .as_table()
        .ok_or_else(|| "`modules` must be a table".to_string())?;
    if modules.is_empty() {
        return Err("`modules` must provide at least one module".into());
    }
    let native = table.get("native").map(parse_native).transpose()?;
    if let Some(NativeClutch { manifest, .. }) = &native
        && let Some(module) = manifest
            .functions
            .iter()
            .map(|binding| &binding.module)
            .chain(manifest.resources.iter().map(|resource| &resource.module))
            .find(|module| !modules.contains_key(module.as_str()))
    {
        return Err(format!(
            "native bindings name module `{module}`, which the clutch does not provide"
        ));
    }
    let mut result = HashMap::new();
    for (name, entry) in modules {
        if !valid_module_name(name) {
            return Err(format!("invalid module name `{name}`"));
        }
        let entry = entry
            .as_table()
            .ok_or_else(|| format!("module `{name}` must be a table"))?;
        require_keys(entry, &["source", "plugin"], &format!("module `{name}`"))?;
        let path = string(entry, "source", &format!("module `{name}`"))?;
        let plugin_entry = entry
            .get("plugin")
            .map(|plugin| {
                let plugin = plugin
                    .as_str()
                    .ok_or_else(|| format!("`plugin` in module `{name}` must be a string"))?;
                if plugin.trim().is_empty() {
                    return Err(format!("`plugin` in module `{name}` must not be empty"));
                }
                Ok(plugin.to_owned())
            })
            .transpose()?;
        result.insert(
            name.clone(),
            ClutchModuleEntry {
                source: path.to_owned(),
                plugin_entry,
            },
        );
    }
    let tests = table
        .get("tests")
        .map(parse_tests)
        .transpose()?
        .unwrap_or_default();
    Ok(ClutchManifest {
        modules: result,
        native,
        tests,
    })
}

fn parse_tests(value: &toml::Value) -> Result<Vec<String>, String> {
    let table = value
        .as_table()
        .ok_or_else(|| "`tests` must be a table".to_string())?;
    require_keys(table, &["files"], "`tests`")?;
    let files = table_value(table, "files", "`tests`")?
        .as_array()
        .ok_or_else(|| "`files` in `tests` must be an array of paths".to_string())?;
    files
        .iter()
        .map(|file| {
            file.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "`files` in `tests` must be an array of paths".to_string())
        })
        .collect()
}

/// Each test file's import name, with the clutch root and the file's path:
/// the clutch's name, its directory name without `.clutch`, followed by the
/// file's path without `.slug`, with `/` written as `.`, so
/// `tests/string.slug` in `slug.core.clutch` is `slug.core.tests.string`.
/// Only the path's form is checked here; the file is read when the module
/// resolves, as a module's source is.
fn test_modules(root: &Path, files: &[String]) -> Result<Vec<(String, TestFile)>, String> {
    let clutch = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".clutch"))
        .ok_or_else(|| format!("clutch root {} must end in `.clutch`", root.display()))?;
    files
        .iter()
        .map(|file| {
            let relative = Path::new(file);
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || !relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                return Err(format!("test file `{file}` escapes the clutch root"));
            }
            if relative
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("slug")
            {
                return Err(format!("test file `{file}` must name a `.slug` file"));
            }
            let module = Path::new(file)
                .with_extension("")
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(".");
            let name = format!("{clutch}.{module}");
            if !valid_module_name(&name) {
                return Err(format!(
                    "test file `{file}` would be module `{name}`, which is not a valid module name"
                ));
            }
            Ok((name, (root.to_path_buf(), file.clone())))
        })
        .collect()
}

fn parse_native(value: &toml::Value) -> Result<NativeClutch, String> {
    let table = value
        .as_table()
        .ok_or_else(|| "`native` must be a table".to_string())?;
    let manifest = NativeManifest::parse(table).map_err(|error| error.to_string())?;
    let libraries = manifest
        .libraries
        .iter()
        .map(|(platform, library)| (platform.clone(), library.clone()))
        .collect();
    Ok(NativeClutch {
        libraries: validate_libraries(libraries)?,
        manifest: Rc::new(manifest),
    })
}

fn validate_libraries(
    libraries: HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    if libraries.is_empty() {
        return Err("`libraries` in native must provide at least one platform".into());
    }
    if let Some(platform) = libraries
        .keys()
        .find(|platform| !supported_platform(platform))
    {
        return Err(format!("unsupported native platform `{platform}`"));
    }
    Ok(libraries)
}

fn native_library_path(
    root: &Path,
    libraries: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    let platform = current_platform()?;
    let library = libraries
        .get(platform)
        .ok_or_else(|| format!("native clutch has no library for platform `{platform}`"))?;
    file_path(root, library, "native library")
}

#[doc(hidden)]
pub fn current_platform() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-aarch64"),
        ("macos", "x86_64") => Ok("macos-x86_64"),
        ("linux", "x86_64") => Ok("linux-x86_64"),
        ("windows", "x86_64") => Ok("windows-x86_64"),
        (os, architecture) => Err(format!(
            "native clutch is unsupported on platform `{os}-{architecture}`"
        )),
    }
}

fn supported_platform(platform: &str) -> bool {
    matches!(
        platform,
        "macos-aarch64" | "macos-x86_64" | "linux-x86_64" | "windows-x86_64"
    )
}

/// The largest asset a clutch may ship, in bytes.
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

/// Reads the asset `name`, a valid asset name, from the `assets` folder of
/// the clutch at `root`. Each segment must name a directory entry exactly,
/// case included, even on a file system that ignores case, so a program
/// that builds on one host builds on every other; and the file must stay in
/// the folder once links resolve.
#[doc(hidden)]
pub fn read_asset(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    let clutch = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let folder = root.join("assets");
    let mut path = folder.clone();
    for segment in name.split('/') {
        let entries = fs::read_dir(&path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !entries.iter().any(|entry| entry == segment) {
            let lower = segment.to_lowercase();
            return Err(
                match entries.iter().find(|entry| entry.to_lowercase() == lower) {
                    Some(entry) => format!(
                        "no asset `{name}` in {clutch}: `{entry}` differs from `{segment}` only in case"
                    ),
                    None => format!("no asset `{name}` in {clutch}'s assets folder"),
                },
            );
        }
        path.push(segment);
    }
    let read_error =
        |error: std::io::Error| format!("cannot read asset `{name}` in {clutch}: {error}");
    let folder = fs::canonicalize(&folder).map_err(read_error)?;
    let path = fs::canonicalize(&path).map_err(read_error)?;
    if !path.starts_with(&folder) {
        return Err(format!(
            "asset `{name}` in {clutch} links outside its assets folder"
        ));
    }
    let metadata = fs::metadata(&path).map_err(read_error)?;
    if !metadata.is_file() {
        return Err(format!("asset `{name}` in {clutch} is not a file"));
    }
    if metadata.len() > MAX_ASSET_BYTES {
        return Err(format!(
            "asset `{name}` in {clutch} is {} bytes, more than the {MAX_ASSET_BYTES}-byte limit",
            metadata.len()
        ));
    }
    fs::read(&path).map_err(read_error)
}

fn source_path(root: &Path, source: &str) -> Result<PathBuf, String> {
    let relative = Path::new(source);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "module source path `{source}` escapes the clutch root"
        ));
    }
    if relative
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("slug")
    {
        return Err(format!(
            "module source path `{source}` must name a `.slug` file"
        ));
    }
    let path = root.join(relative);
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot access module source {}: {error}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "module source path `{source}` escapes the clutch root"
        ));
    }
    if !canonical.is_file() {
        return Err(format!("module source path `{source}` is not a file"));
    }
    Ok(canonical)
}

fn file_path(root: &Path, source: &str, kind: &str) -> Result<PathBuf, String> {
    let relative = contained_relative_path(source, kind)?;
    let path = root.join(relative);
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("cannot access {kind} {}: {error}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!("{kind} path `{source}` escapes the clutch root"));
    }
    if !canonical.is_file() {
        return Err(format!("{kind} path `{source}` is not a file"));
    }
    Ok(canonical)
}

fn contained_relative_path<'a>(source: &'a str, kind: &str) -> Result<&'a Path, String> {
    let relative = Path::new(source);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("{kind} path `{source}` escapes the clutch root"));
    }
    Ok(relative)
}

fn require_keys(
    table: &toml::map::Map<String, toml::Value>,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    let allowed = allowed.iter().copied().collect::<HashSet<_>>();
    if let Some(key) = table.keys().find(|key| !allowed.contains(key.as_str())) {
        return Err(format!("unknown key `{key}` in {context}"));
    }
    Ok(())
}

fn table_value<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<&'a toml::Value, String> {
    table
        .get(key)
        .ok_or_else(|| format!("missing `{key}` in {context}"))
}

fn string<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    table_value(table, key, context)?
        .as_str()
        .ok_or_else(|| format!("`{key}` in {context} must be a string"))
}

fn valid_module_name(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|value| value == '_' || value.is_ascii_alphanumeric())
    })
}
