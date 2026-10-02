//! Storage-independent import request, identity, source, and activation
//! contracts.
//!
//! This crate deliberately has no desktop policy or VM construction.

use std::{fmt, path::PathBuf};

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

    /// Stages host capabilities required by a resolved module.
    ///
    /// Resolvers without import-scoped activation keep the default. A resolver
    /// that returns an activation lease must override this method rather than
    /// leaving the frontend to know its storage representation.
    ///
    /// # Errors
    ///
    /// Returns a checked error when this resolver cannot stage the source's
    /// activation lease.
    fn stage_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Box<dyn ModuleActivationTransaction>>, ModuleLoadError> {
        if source.activation.is_some() {
            return Err(ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: "module activation requires resolver support".into(),
            });
        }
        Ok(None)
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

    use super::{ModuleLoadError, module_path};

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
}
