use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Component, Path, PathBuf},
};

const EXPERIMENTAL_RUNTIME_VERSION: Version = Version::new(0, 1, 0);
const EXPERIMENTAL_PLUGIN_API: &str = "rust-facade-0";

/// An explicit host-owned mapping from module names to exploded clutch roots.
///
/// The local clutch experiment does not scan directories or install packages.
/// Hosts and tests supply this mapping directly.
#[derive(Clone, Debug, Default)]
pub struct ClutchRepository {
    providers: HashMap<String, PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClutchRepositoryError {
    InvalidModuleName(String),
    DuplicateProvider { name: String },
}

impl fmt::Display for ClutchRepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModuleName(name) => write!(f, "invalid module name `{name}`"),
            Self::DuplicateProvider { name } => {
                write!(f, "module `{name}` has more than one clutch provider")
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
        Ok(Self { providers: indexed })
    }

    #[must_use]
    pub(crate) fn provider(&self, name: &str) -> Option<&PathBuf> {
        self.providers.get(name)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ClutchModule {
    pub path: PathBuf,
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
    let source = manifest
        .modules
        .get(name)
        .ok_or_else(|| format!("clutch does not provide indexed module `{name}`"))?;
    let source_path = source_path(&root, source)?;
    Ok(ClutchModule { path: source_path })
}

#[derive(Debug)]
struct ClutchManifest {
    modules: HashMap<String, String>,
}

fn parse_manifest(source: &str) -> Result<ClutchManifest, String> {
    let value = source
        .parse::<toml::Value>()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    let table = value
        .as_table()
        .ok_or_else(|| "manifest root must be a table".to_string())?;
    require_keys(
        table,
        &["format", "clutch", "requires", "modules", "plugins"],
        "manifest",
    )?;
    let format = integer(table, "format", "manifest")?;
    if format != 0 {
        return Err(format!("unsupported clutch manifest format `{format}`"));
    }
    validate_clutch(table)?;
    validate_requirements(table)?;
    if let Some(plugins) = table.get("plugins") {
        let plugins = plugins
            .as_table()
            .ok_or_else(|| "`plugins` must be a table".to_string())?;
        if !plugins.is_empty() {
            return Err(
                "native clutch plugins are not supported by the source-only experiment".into(),
            );
        }
    }
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
        if entry.contains_key("plugin") {
            return Err(format!(
                "module `{name}` requires a native plugin, which is not supported by the source-only experiment"
            ));
        }
        let path = string(entry, "source", &format!("module `{name}`"))?;
        result.insert(name.clone(), path.to_owned());
    }
    Ok(ClutchManifest { modules: result })
}

fn validate_clutch(table: &toml::map::Map<String, toml::Value>) -> Result<(), String> {
    let clutch = table_value(table, "clutch", "manifest")?
        .as_table()
        .ok_or_else(|| "`clutch` must be a table".to_string())?;
    require_keys(clutch, &["publisher", "name", "version"], "clutch")?;
    for key in ["publisher", "name", "version"] {
        if string(clutch, key, "clutch")?.is_empty() {
            return Err(format!("`clutch.{key}` must not be empty"));
        }
    }
    Ok(())
}

fn validate_requirements(table: &toml::map::Map<String, toml::Value>) -> Result<(), String> {
    let requires = table_value(table, "requires", "manifest")?
        .as_table()
        .ok_or_else(|| "`requires` must be a table".to_string())?;
    require_keys(requires, &["runtime", "plugin_api"], "requires")?;
    let runtime = string(requires, "runtime", "requires")?;
    if !runtime_matches(runtime, EXPERIMENTAL_RUNTIME_VERSION)? {
        return Err(format!(
            "runtime requirement `{runtime}` is incompatible with experimental runtime {EXPERIMENTAL_RUNTIME_VERSION}"
        ));
    }
    let plugin_api = string(requires, "plugin_api", "requires")?;
    if plugin_api != EXPERIMENTAL_PLUGIN_API {
        return Err(format!(
            "plugin API requirement `{plugin_api}` is incompatible with `{EXPERIMENTAL_PLUGIN_API}`"
        ));
    }
    Ok(())
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

fn integer(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<i64, String> {
    table_value(table, key, context)?
        .as_integer()
        .ok_or_else(|| format!("`{key}` in {context} must be an integer"))
}

fn valid_module_name(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|value| value == '_' || value.is_ascii_alphanumeric())
    })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn runtime_matches(requirement: &str, runtime: Version) -> Result<bool, String> {
    let clauses = requirement.split(',').map(str::trim).collect::<Vec<_>>();
    if clauses.is_empty() || clauses.iter().any(|clause| clause.is_empty()) {
        return Err("runtime requirement must contain one or more version clauses".into());
    }
    clauses.into_iter().try_fold(true, |matches, clause| {
        let (operator, version) = [">=", "<=", ">", "<", "="]
            .into_iter()
            .find_map(|operator| {
                clause
                    .strip_prefix(operator)
                    .map(|version| (operator, version))
            })
            .ok_or_else(|| format!("invalid runtime requirement clause `{clause}`"))?;
        let version = parse_version(version.trim())?;
        Ok(matches
            && match operator {
                ">=" => runtime >= version,
                "<=" => runtime <= version,
                ">" => runtime > version,
                "<" => runtime < version,
                "=" => runtime == version,
                _ => unreachable!("operators are selected from a fixed list"),
            })
    })
}

fn parse_version(value: &str) -> Result<Version, String> {
    let values = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid runtime version `{value}`"))?;
    let [major, minor, patch] = values.as_slice() else {
        return Err(format!(
            "runtime version `{value}` must have major, minor, and patch"
        ));
    };
    Ok(Version::new(*major, *minor, *patch))
}
