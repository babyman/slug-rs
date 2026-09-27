//! Source syntax, semantic analysis, lowering, and module-graph ownership.
//!
//! The public surface is deliberately pre-release. It is the one-way source
//! frontend for the private bytecode runtime, not a second stable embedding API.

mod host;
mod module;
mod source;

pub use host::{DesktopLoader, build_default_host_vm, default_library_root};
pub use module::{ModuleInstance, ModuleLoader};
pub use source::{
    InteractiveCompilation, InteractiveCompilerState, SourceError, SourceErrorKind,
    SourceReadiness, compile, compile_interactive_forms, source_readiness,
};

// These aliases retain the existing source implementation's runtime inputs
// while ownership moves here. They are private to this crate.
pub(crate) use slug_loader::{
    ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
};
pub(crate) use slug_vm::{
    CallArgumentKind, CallableIdentity, Capture, Chunk, ClutchRepository, ClutchRepositoryError,
    Configuration, DeferMode, Entrypoint, EntrypointArguments, EnumValue, FfiPrototypeLibrary,
    ForeignResourceSignature, MatchMapKey, MatchPattern, MatchRest, MatchType, ModuleDeclaration,
    ModuleTag, NativeDescriptorError, NativeFunction, Op, ParameterSignature, Program,
    ProgramBuilder, SchemaField, SelectCase, SourceSpan, Value, Vm, VmHost, VmHostError,
    VmModuleExports,
};
