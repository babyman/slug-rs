//! The typed native binding model for a clutch manifest's `[native]` table.
//!
//! The shim generator, the desktop loader, and static builds all read bindings
//! through this crate, so they derive the same canonical signature from one
//! parsed form. See `docs/decisions/2026-10-11-typed-native-manifest-bindings.md`.

mod signature;

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

pub use signature::{Direction, NativeType, Parameter, Signature};
use signature::{ParsedSignature, ParsedType};

/// Version of the uniform shim entry and table layout.
pub const SHIM_ABI_VERSION: u32 = 1;

/// Maximum argument cells, and separately result cells, for one binding.
pub const MAX_CELLS: usize = 16;

/// Native interface profile a provider requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Core,
    Events,
}

/// One C call bound through a shim: a Slug-visible function or a resource
/// release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    pub module: String,
    /// Shim table name: the Slug function name, or `release:<resource>`.
    pub name: String,
    pub symbol: String,
    pub signature: Signature,
    /// The target cannot be addressed, so the shim skips its exact-type check.
    pub is_macro: bool,
}

impl Binding {
    /// The text a shim stores and the loader compares to detect a stale shim.
    #[must_use]
    pub fn canonical_signature(&self) -> String {
        let mut text = format!(
            "slug-shim/{SHIM_ABI_VERSION};{};{};{};",
            self.module, self.name, self.symbol
        );
        self.signature.write_canonical(&mut text);
        if self.is_macro {
            text.push_str(";macro");
        }
        text
    }
}

/// A handle type and the function that releases a still-open handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resource {
    pub module: String,
    pub name: String,
    /// The pointee type; shims pass `c_type *`.
    pub c_type: String,
    pub release: Binding,
}

/// The parsed typed form of a clutch manifest's `[native]` table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeManifest {
    pub profile: Profile,
    pub headers: Vec<String>,
    pub sources: Vec<String>,
    pub link: Vec<String>,
    /// Platform name to dynamic library path, relative to the clutch.
    pub libraries: BTreeMap<String, String>,
    /// Ordered by module, then resource name.
    pub resources: Vec<Resource>,
    /// Ordered by module, then function name.
    pub functions: Vec<Binding>,
}

/// A malformed typed manifest. `location` names the manifest path of the
/// offending entry, such as `native.functions."slug.io.fs".readLineRaw`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestError {
    pub location: String,
    pub message: String,
}

impl ManifestError {
    fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for ManifestError {}

type Table = toml::map::Map<String, toml::Value>;

impl NativeManifest {
    /// Parses the `[native]` table of a `clutch.toml` source.
    ///
    /// # Errors
    ///
    /// Returns a checked error for invalid TOML, a missing or prototype-form
    /// `[native]` table, or any invalid typed binding.
    pub fn from_clutch_source(source: &str) -> Result<Self, ManifestError> {
        let root = source
            .parse::<Table>()
            .map_err(|error| ManifestError::new("manifest", format!("invalid TOML: {error}")))?;
        let native = root
            .get("native")
            .ok_or_else(|| ManifestError::new("manifest", "missing `native` table"))?;
        let native = native
            .as_table()
            .ok_or_else(|| ManifestError::new("native", "must be a table"))?;
        Self::parse(native)
    }

    /// Parses a typed `[native]` table.
    ///
    /// # Errors
    ///
    /// Returns a checked error naming the offending manifest path.
    pub fn parse(native: &Table) -> Result<Self, ManifestError> {
        if native.contains_key("abi") {
            return Err(ManifestError::new(
                "native",
                "`abi` selects the prototype form, which cannot be combined with typed bindings",
            ));
        }
        require_keys(
            native,
            &[
                "profile",
                "headers",
                "sources",
                "link",
                "libraries",
                "resources",
                "functions",
            ],
            "native",
        )?;
        let profile = match required_string(native, "profile", "native")? {
            "core" => Profile::Core,
            "events" => Profile::Events,
            other => {
                return Err(ManifestError::new(
                    "native.profile",
                    format!("unknown profile `{other}`"),
                ));
            }
        };
        let headers = string_list(native, "headers", "native")?;
        let sources = string_list(native, "sources", "native")?;
        let link = string_list(native, "link", "native")?;
        let libraries = optional_table(native, "libraries", "native")?
            .into_iter()
            .flatten()
            .map(|(platform, path)| {
                path.as_str()
                    .map(|path| (platform.clone(), path.to_owned()))
                    .ok_or_else(|| {
                        ManifestError::new(
                            format!("native.libraries.{platform}"),
                            "must be a string",
                        )
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let mut resources = Vec::new();
        for (module, entries) in modules(native, "resources")? {
            for (name, entry) in entries {
                let location = format!("native.resources.\"{module}\".{name}");
                resources.push(parse_resource(module, name, entry, &location)?);
            }
        }
        let mut functions = Vec::new();
        for (module, entries) in modules(native, "functions")? {
            for (name, entry) in entries {
                let location = format!("native.functions.\"{module}\".{name}");
                functions.push(parse_function(module, name, entry, &resources, &location)?);
            }
        }
        // TOML table order depends on crate features; shims must be deterministic.
        resources
            .sort_by(|left, right| (&left.module, &left.name).cmp(&(&right.module, &right.name)));
        functions
            .sort_by(|left, right| (&left.module, &left.name).cmp(&(&right.module, &right.name)));
        if !functions.is_empty() && headers.is_empty() {
            return Err(ManifestError::new(
                "native.headers",
                "must name at least one header declaring the bound symbols",
            ));
        }
        Ok(Self {
            profile,
            headers,
            sources,
            link,
            libraries,
            resources,
            functions,
        })
    }

    /// Every binding a shim table contains: functions, then resource releases.
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.functions
            .iter()
            .chain(self.resources.iter().map(|resource| &resource.release))
    }
}

fn parse_resource(
    module: &str,
    name: &str,
    entry: &toml::Value,
    location: &str,
) -> Result<Resource, ManifestError> {
    if !is_identifier(name) {
        return Err(ManifestError::new(location, "invalid resource name"));
    }
    let entry = entry
        .as_table()
        .ok_or_else(|| ManifestError::new(location, "must be a table"))?;
    require_keys(entry, &["type", "release"], location)?;
    let c_type = required_string(entry, "type", location)?;
    if !is_identifier(c_type) {
        return Err(ManifestError::new(
            format!("{location}.type"),
            format!("`{c_type}` is not a C type name"),
        ));
    }
    let release_location = format!("{location}.release");
    let release = entry
        .get("release")
        .ok_or_else(|| ManifestError::new(location, "missing `release`"))?
        .as_table()
        .ok_or_else(|| ManifestError::new(&release_location, "must be a table"))?;
    require_keys(
        release,
        &["symbol", "signature", "macro"],
        &release_location,
    )?;
    let release = parse_binding(
        module,
        &format!("release:{name}"),
        release,
        &[(name, c_type)],
        &release_location,
    )?;
    let own_handle = NativeType::Handle {
        resource: name.to_owned(),
        c_type: c_type.to_owned(),
    };
    let parameters = &release.signature.parameters;
    if parameters.len() != 1
        || parameters[0].direction != Direction::In
        || parameters[0].native_type != own_handle
    {
        return Err(ManifestError::new(
            format!("{release_location}.signature"),
            format!("a release takes exactly one `handle<{name}>` argument"),
        ));
    }
    if matches!(release.signature.result, Some(NativeType::Cstr { .. })) {
        return Err(ManifestError::new(
            format!("{release_location}.signature"),
            "a release result is ignored, so it cannot be text",
        ));
    }
    Ok(Resource {
        module: module.to_owned(),
        name: name.to_owned(),
        c_type: c_type.to_owned(),
        release,
    })
}

fn parse_function(
    module: &str,
    name: &str,
    entry: &toml::Value,
    resources: &[Resource],
    location: &str,
) -> Result<Binding, ManifestError> {
    if !is_identifier(name) {
        return Err(ManifestError::new(location, "invalid function name"));
    }
    let entry = entry
        .as_table()
        .ok_or_else(|| ManifestError::new(location, "must be a table"))?;
    require_keys(entry, &["symbol", "signature", "macro"], location)?;
    let handles = resources
        .iter()
        .filter(|resource| resource.module == module)
        .map(|resource| (resource.name.as_str(), resource.c_type.as_str()))
        .collect::<Vec<_>>();
    parse_binding(module, name, entry, &handles, location)
}

fn parse_binding(
    module: &str,
    name: &str,
    entry: &Table,
    handles: &[(&str, &str)],
    location: &str,
) -> Result<Binding, ManifestError> {
    let symbol = required_string(entry, "symbol", location)?;
    if !is_identifier(symbol) {
        return Err(ManifestError::new(
            format!("{location}.symbol"),
            format!("`{symbol}` is not a C identifier"),
        ));
    }
    let is_macro = match entry.get("macro") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| ManifestError::new(format!("{location}.macro"), "must be a boolean"))?,
    };
    let signature_location = format!("{location}.signature");
    let text = required_string(entry, "signature", location)?;
    let parsed = signature::parse(text)
        .map_err(|message| ManifestError::new(&signature_location, message))?;
    let signature = resolve(parsed, handles)
        .map_err(|message| ManifestError::new(&signature_location, message))?;
    validate(&signature).map_err(|message| ManifestError::new(&signature_location, message))?;
    Ok(Binding {
        module: module.to_owned(),
        name: name.to_owned(),
        symbol: symbol.to_owned(),
        signature,
        is_macro,
    })
}

/// Resolves each `handle<R>` against the module's `(resource, C type)` pairs.
fn resolve(parsed: ParsedSignature, handles: &[(&str, &str)]) -> Result<Signature, String> {
    let resolve_type = |parsed: ParsedType| -> Result<NativeType, String> {
        Ok(match parsed {
            ParsedType::Plain(native_type) => native_type,
            ParsedType::Cstr { owned } => {
                if let Some(free) = &owned
                    && !is_identifier(free)
                {
                    return Err(format!("`{free}` is not a C identifier"));
                }
                NativeType::Cstr { owned }
            }
            ParsedType::Handle { resource } => {
                let (_, c_type) = handles
                    .iter()
                    .find(|(name, _)| *name == resource)
                    .ok_or_else(|| format!("unknown resource `{resource}` in this module"))?;
                NativeType::Handle {
                    resource,
                    c_type: (*c_type).to_owned(),
                }
            }
        })
    };
    let parameters = parsed
        .parameters
        .into_iter()
        .map(|parameter| {
            Ok(Parameter {
                direction: parameter.direction,
                native_type: resolve_type(parameter.native_type)?,
                name: parameter.name,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let result = parsed.result.map(resolve_type).transpose()?;
    Ok(Signature { parameters, result })
}

fn validate(signature: &Signature) -> Result<(), String> {
    let mut closes = 0usize;
    for parameter in &signature.parameters {
        let native_type = &parameter.native_type;
        match parameter.direction {
            Direction::In => {
                if matches!(native_type, NativeType::Cstr { owned: Some(_) }) {
                    return Err("only a result can be `owned`".into());
                }
            }
            Direction::Out => {
                if matches!(native_type, NativeType::Str | NativeType::Bytes) {
                    return Err("`str` and `bytes` are arguments only".into());
                }
            }
            Direction::Close => {
                if !matches!(native_type, NativeType::Handle { .. }) {
                    return Err("only a `handle` parameter can be `close`".into());
                }
                closes += 1;
            }
        }
    }
    if closes > 1 {
        return Err("at most one parameter can be `close`".into());
    }
    if matches!(signature.result, Some(NativeType::Str | NativeType::Bytes)) {
        return Err("`str` and `bytes` are arguments only".into());
    }
    if signature.argument_cells() > MAX_CELLS {
        return Err(format!("more than {MAX_CELLS} argument cells"));
    }
    if signature.result_cells() > MAX_CELLS {
        return Err(format!("more than {MAX_CELLS} result cells"));
    }
    Ok(())
}

/// Returns each `"module" = { name = entry }` group of a binding table, with
/// validated module names.
fn modules<'a>(native: &'a Table, key: &str) -> Result<Vec<(&'a str, &'a Table)>, ManifestError> {
    let location = format!("native.{key}");
    optional_table(native, key, "native")?
        .into_iter()
        .flatten()
        .map(|(module, entries)| {
            if !is_module_name(module) {
                return Err(ManifestError::new(
                    &location,
                    format!("invalid module name `{module}`"),
                ));
            }
            entries
                .as_table()
                .map(|entries| (module.as_str(), entries))
                .ok_or_else(|| {
                    ManifestError::new(format!("{location}.\"{module}\""), "must be a table")
                })
        })
        .collect()
}

fn require_keys(table: &Table, allowed: &[&str], location: &str) -> Result<(), ManifestError> {
    let allowed = allowed.iter().copied().collect::<HashSet<_>>();
    match table.keys().find(|key| !allowed.contains(key.as_str())) {
        Some(key) => Err(ManifestError::new(location, format!("unknown key `{key}`"))),
        None => Ok(()),
    }
}

fn required_string<'a>(
    table: &'a Table,
    key: &str,
    location: &str,
) -> Result<&'a str, ManifestError> {
    table
        .get(key)
        .ok_or_else(|| ManifestError::new(location, format!("missing `{key}`")))?
        .as_str()
        .ok_or_else(|| ManifestError::new(format!("{location}.{key}"), "must be a string"))
}

fn string_list(table: &Table, key: &str, location: &str) -> Result<Vec<String>, ManifestError> {
    let Some(value) = table.get(key) else {
        return Ok(Vec::new());
    };
    let location = format!("{location}.{key}");
    value
        .as_array()
        .ok_or_else(|| ManifestError::new(&location, "must be an array of strings"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ManifestError::new(&location, "must be an array of strings"))
        })
        .collect()
}

fn optional_table<'a>(
    table: &'a Table,
    key: &str,
    location: &str,
) -> Result<Option<&'a Table>, ManifestError> {
    table
        .get(key)
        .map(|value| {
            value
                .as_table()
                .ok_or_else(|| ManifestError::new(format!("{location}.{key}"), "must be a table"))
        })
        .transpose()
}

fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_module_name(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
    })
}
