use crate::{NativeDescriptorError, NativeFunction};
use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

/// An explicit host-owned mapping from module names to exploded clutch roots.
///
/// Hosts may construct this mapping directly or discover its providers from
/// one local exploded-clutch directory.
#[derive(Clone, Default)]
pub struct ClutchRepository {
    providers: HashMap<String, PathBuf>,
    plugin_initializers: HashMap<String, ClutchPluginInitializer>,
}

impl fmt::Debug for ClutchRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClutchRepository")
            .field("providers", &self.providers)
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
            plugin_initializers: HashMap::new(),
        })
    }

    /// Discovers exploded clutch directories immediately inside `root`.
    ///
    /// Each directory ending in `.clutch` contributes every module named in
    /// its manifest. Duplicate module providers are rejected.
    ///
    /// # Errors
    ///
    /// Returns an error when the repository cannot be read, a clutch manifest
    /// is invalid, or two clutches provide the same module.
    pub fn from_directory(root: impl AsRef<Path>) -> Result<Self, ClutchRepositoryError> {
        let root = root.as_ref();
        let entries = fs::read_dir(root).map_err(|error| ClutchRepositoryError::Read {
            path: root.into(),
            message: error.to_string(),
        })?;
        let mut providers = HashMap::new();
        for entry in entries {
            let entry = entry.map_err(|error| ClutchRepositoryError::Read {
                path: root.into(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("clutch") {
                continue;
            }
            if !path.is_dir() {
                return Err(ClutchRepositoryError::Read {
                    path,
                    message: "clutch provider must be a directory".into(),
                });
            }
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
            for name in manifest.modules.into_keys() {
                if providers.insert(name.clone(), path.clone()).is_some() {
                    return Err(ClutchRepositoryError::DuplicateProvider { name });
                }
            }
        }
        Ok(Self {
            providers,
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
    pub(crate) fn provider(&self, name: &str) -> Option<&PathBuf> {
        self.providers.get(name)
    }

    #[must_use]
    pub(crate) fn plugin(&self, entry: &str) -> Option<ClutchPluginInitializer> {
        self.plugin_initializers.get(entry).cloned()
    }
}

/// Initializes one manifest-selected plugin within one module-bound scope.
pub type ClutchPluginInitializer =
    Rc<dyn Fn(&mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError>>;

/// A module-bound, staged registrar exposed to experimental Rust clutch plugins.
pub struct ClutchPluginRegistrar {
    module_name: String,
    functions: Vec<NativeFunction>,
    cleanup: Option<fn()>,
}

impl ClutchPluginRegistrar {
    pub(crate) fn new(module_name: impl Into<String>) -> Self {
        Self {
            module_name: module_name.into(),
            functions: Vec::new(),
            cleanup: None,
        }
    }

    /// Returns the only source module this plugin invocation may serve.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.module_name
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
        if function.module_name() != self.module_name {
            return Err(NativeDescriptorError::new(format!(
                "clutch plugin for module `{}` cannot register foreign function `{}`",
                self.module_name,
                function.qualified_name()
            )));
        }
        if self
            .functions
            .iter()
            .any(|existing| existing.name() == function.name())
        {
            return Err(NativeDescriptorError::new(format!(
                "clutch plugin foreign function `{}.{}` is already staged",
                self.module_name,
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
        if self.cleanup.replace(cleanup).is_some() {
            return Err(NativeDescriptorError::new(
                "clutch plugin cleanup hook is already registered",
            ));
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> StagedClutchPlugin {
        StagedClutchPlugin {
            functions: self.functions,
            cleanup: self.cleanup,
        }
    }
}

#[derive(Debug)]
pub(crate) struct StagedClutchPlugin {
    pub functions: Vec<NativeFunction>,
    cleanup: Option<fn()>,
}

impl StagedClutchPlugin {
    pub(crate) fn cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ClutchModule {
    pub root: PathBuf,
    pub path: PathBuf,
    pub plugin_entry: Option<String>,
}

pub(crate) fn load_module(root: &Path, name: &str) -> Result<ClutchModule, String> {
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
    Ok(ClutchModule {
        root,
        path: source_path,
        plugin_entry: module.plugin_entry.clone(),
    })
}

#[derive(Debug)]
struct ClutchManifest {
    modules: HashMap<String, ClutchModuleEntry>,
}

#[derive(Debug)]
struct ClutchModuleEntry {
    source: String,
    plugin_entry: Option<String>,
}

fn parse_manifest(source: &str) -> Result<ClutchManifest, String> {
    let value = source
        .parse::<toml::Value>()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "manifest root must be a table".to_string())?;
    require_keys(table, &["modules"], "manifest")?;
    let modules = table_value(table, "modules", "manifest")?;
    let modules = modules
        .as_table()
        .ok_or_else(|| "`modules` must be a table".to_string())?;
    if modules.is_empty() {
        return Err("`modules` must provide at least one module".into());
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
    Ok(ClutchManifest { modules: result })
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
