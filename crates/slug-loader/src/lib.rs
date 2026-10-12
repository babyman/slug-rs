//! Storage-independent import request, identity, source, and activation
//! contracts.
//!
//! This crate deliberately has no desktop policy or VM construction.

use std::{collections::HashMap, fmt, path::PathBuf};

/// Opaque host-defined identity for one resolved module.
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

/// Source supplied by an external import resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSource {
    /// Stable host identity used by module caches.
    pub key: ModuleKey,
    /// Host-provided label used in source and module diagnostics.
    pub diagnostic_name: String,
    /// Source text to compile.
    pub text: String,
    /// Opaque resolver-owned activation retained while the module is live.
    pub activation: Option<ModuleActivation>,
}

/// Opaque host-owned capability used for import-scoped activation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ModuleActivation(String);

impl ModuleActivation {
    #[must_use]
    pub fn new(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }
}

/// Resolver-owned transaction for one import-scoped activation.
///
/// The frontend sequences this transaction around module initialization without
/// learning how a resolver registers or tears down host capabilities.
pub trait ModuleActivationTransaction {
    /// Publishes the activation's capabilities.
    ///
    /// # Errors
    ///
    /// Returns a checked error without publishing a partial activation.
    fn register(&self) -> Result<(), ModuleLoadError>;

    /// Reverses capabilities published before module initialization failed.
    fn rollback(&self);

    /// Releases an activation that did not become live.
    fn cleanup(&mut self);

    /// Transfers a live activation back to its resolver for shutdown.
    fn retain(self: Box<Self>);
}

/// A `foreign` declaration as written in module source, for hosts that check
/// it against a native binding before the module is installed.
///
/// Type annotations are rendered as canonical text: union members are sorted
/// and joined with `|`, and type arguments are written `name<a,b>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignDeclaration {
    pub name: String,
    pub parameters: Vec<ForeignParameter>,
    /// The result annotation, or `None` when it is omitted.
    pub result: Option<String>,
}

/// One parameter of a [`ForeignDeclaration`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignParameter {
    pub name: String,
    /// The type annotation, or `None` when it is omitted.
    pub annotation: Option<String>,
    pub variadic: bool,
    pub has_default: bool,
}

/// Resolves logical module identities into host-provided source.
pub trait ModuleResolver {
    /// Resolves one requested module without exposing host storage to the
    /// frontend or VM.
    ///
    /// # Errors
    ///
    /// Returns a checked failure when the name is invalid, unavailable, or its
    /// host-provided source cannot be read.
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError>;
}

/// One source module selected by a statically linked host.
///
/// Static providers are selected by their embedding host at construction time.
/// They do not access storage, discover plugins, or load shared libraries.
#[derive(Clone)]
pub struct StaticModuleProvider {
    name: String,
    source: ModuleSource,
}

impl StaticModuleProvider {
    /// Creates a static provider for a valid dotted module name.
    ///
    /// # Errors
    ///
    /// Returns an invalid-name error when `name` is not a valid logical module
    /// name.
    pub fn new(name: impl Into<String>, source: ModuleSource) -> Result<Self, ModuleLoadError> {
        let name = name.into();
        module_path(&name)?;
        Ok(Self { name, source })
    }
}

/// Fixed in-memory registry for source modules supplied by a host.
///
/// This resolver is the storage-independent half of static Clutch providers.
/// A desktop simulator or firmware bootstrap selects its providers explicitly;
/// neither path inherits desktop Clutch discovery or dynamic-library policy.
pub struct StaticModuleResolver {
    providers: HashMap<String, StaticModuleProvider>,
}

impl StaticModuleResolver {
    /// Creates a static registry from explicitly selected providers.
    ///
    /// # Errors
    ///
    /// Returns a checked error when more than one provider uses the same
    /// logical module name.
    pub fn new(
        providers: impl IntoIterator<Item = StaticModuleProvider>,
    ) -> Result<Self, ModuleLoadError> {
        let mut registered = HashMap::new();
        for provider in providers {
            let name = provider.name.clone();
            if registered.insert(name.clone(), provider).is_some() {
                return Err(ModuleLoadError::Clutch {
                    location: name,
                    message: "duplicate static module provider".into(),
                });
            }
        }
        Ok(Self {
            providers: registered,
        })
    }
}

impl ModuleResolver for StaticModuleResolver {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.providers
            .get(request.name)
            .map(|provider| provider.source.clone())
            .ok_or_else(|| ModuleLoadError::NotFound {
                name: request.name.into(),
                searched: vec!["static providers".into()],
            })
    }
}

/// Checked failure while resolving or compiling one imported module.
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

/// Converts a dotted logical module name into its relative source path.
///
/// This validates logical names without selecting a host root or accessing
/// storage, so every filesystem-backed resolver shares the same boundary.
///
/// # Errors
///
/// Returns [`ModuleLoadError::InvalidName`] when a name has an empty or
/// non-identifier segment.
pub fn module_path(name: &str) -> Result<PathBuf, ModuleLoadError> {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
        StaticModuleProvider, StaticModuleResolver, module_path,
    };

    #[test]
    fn maps_valid_dotted_names_to_relative_slug_paths() {
        assert_eq!(
            module_path("example.nested_module").expect("valid module name"),
            PathBuf::from("example/nested_module.slug")
        );
    }

    #[test]
    fn rejects_empty_and_non_identifier_segments() {
        for name in ["", "example..nested", "example/path", "example-name"] {
            assert_eq!(
                module_path(name),
                Err(ModuleLoadError::InvalidName(name.into()))
            );
        }
    }

    #[test]
    fn resolves_only_explicitly_registered_static_modules() {
        let source = ModuleSource {
            key: ModuleKey::new("static:slug.gpio"),
            diagnostic_name: "static:slug.gpio".into(),
            text: "foreign write(name: str, level: bool): nil".into(),
            activation: None,
        };
        let resolver = StaticModuleResolver::new([
            StaticModuleProvider::new("slug.gpio", source).expect("valid static provider")
        ])
        .expect("register static provider");

        let module_source = resolver
            .resolve(ModuleRequest::new(None, "slug.gpio"))
            .expect("resolve registered provider");
        assert_eq!(module_source.key.as_str(), "static:slug.gpio");

        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(None, "slug.unavailable"))
                .expect_err("unregistered static module is unavailable"),
            ModuleLoadError::NotFound {
                name: "slug.unavailable".into(),
                searched: vec!["static providers".into()],
            }
        );
    }
}
