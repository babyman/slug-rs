//! Storage-independent import request, identity, source, and activation
//! contracts.
//!
//! This crate deliberately has no desktop policy or VM construction.

use std::fmt;

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
