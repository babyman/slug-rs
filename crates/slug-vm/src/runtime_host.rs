//! VM callbacks supplied by the source frontend or an embedding host.
//!
//! This is an intentionally unstable, in-process seam. It lets the VM execute
//! bytecode without knowing how modules, configuration, or native descriptors
//! are assembled. It is not a general embedding API.

use std::collections::HashMap;

use crate::{NativeDescriptorError, NativeFunction, NativeResourceRegistry, Value};

/// Checked failure returned by a VM host callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VmHostError {
    message: String,
    not_found: bool,
}

impl VmHostError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            not_found: false,
        }
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            not_found: true,
        }
    }

    #[must_use]
    pub fn is_not_found(&self) -> bool {
        self.not_found
    }
}

impl std::fmt::Display for VmHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for VmHostError {}

/// Live exports returned by an initialized source module.
#[derive(Clone, Debug)]
pub struct VmModuleExports {
    pub exports: Value,
}

/// Runtime configuration callbacks required by `cfg` and typed entrypoints.
///
/// Concrete hosts collect and interpret configuration before constructing a VM.
/// The VM only requests a value using a Slug fallback shape or receives prepared
/// entrypoint arguments. This trait is intentionally unstable.
pub trait VmConfiguration {
    /// Resolves one configuration key, returning `fallback` when it is absent.
    fn resolve(&self, key: &str, fallback: &Value) -> Value;

    /// Returns the unmodified arguments after the entry program name.
    fn arguments(&self) -> &[String];

    /// Returns prepared program options and positional arguments as Slug values.
    fn argument_map(&self) -> Value;
}

/// A configuration callback with no selected values or program arguments.
///
/// This is useful for in-memory hosts and tests that deliberately do not
/// collect configuration from the process or filesystem.
#[derive(Clone, Debug, Default)]
pub struct EmptyVmConfiguration;

impl VmConfiguration for EmptyVmConfiguration {
    fn resolve(&self, _: &str, fallback: &Value) -> Value {
        fallback.clone()
    }

    fn arguments(&self) -> &[String] {
        &[]
    }

    fn argument_map(&self) -> Value {
        Value::Map(
            vec![
                (Value::string("options"), Value::Map(Vec::new().into())),
                (Value::string("positional"), Value::list(Vec::new())),
            ]
            .into(),
        )
    }
}

/// Host services required while a VM executes installed bytecode.
///
/// The frontend owns source compilation and module graph identity; a concrete
/// host owns resolution, configuration, native registrations, and cleanup.
/// Implementations must report failures through [`VmHostError`] rather than
/// panicking. This trait is intentionally unstable.
pub trait VmHost {
    /// Initializes a module and returns its live exports.
    ///
    /// # Errors
    ///
    /// Returns a checked host failure when the module is unavailable or cannot
    /// be initialized.
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError>;

    /// Values exposed by the host-provided `slug.builtin` module.
    fn builtin_globals(&self) -> HashMap<String, Value>;

    /// Explicit native globals inherited by module VMs created by this host.
    fn native_globals(&self) -> HashMap<String, Value>;

    /// Looks up a module-qualified native declaration.
    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction>;

    /// Registers native descriptors atomically for later foreign lookup.
    ///
    /// # Errors
    ///
    /// Returns a descriptor error without publishing any supplied function
    /// when the host rejects the registration batch.
    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError>;

    /// Retains a native global for module VMs created by this host.
    fn define_native_global(&self, name: String, value: Value);

    /// Native-resource lifetime registry shared by VMs for this host.
    fn native_resources(&self) -> NativeResourceRegistry;

    /// Immutable configuration callbacks visible to `cfg` and typed entrypoints.
    fn configuration(&self) -> &dyn VmConfiguration;

    /// Receives non-fatal runtime warnings such as shadowed imports.
    fn warn(&self, message: String);

    /// Releases host-owned import activations during VM shutdown.
    fn shutdown(&self);
}
