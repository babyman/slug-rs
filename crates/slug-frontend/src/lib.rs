//! Source syntax, semantic analysis, lowering, and module-graph ownership.
//!
//! The public surface is deliberately pre-release. It is the one-way source
//! frontend for the private bytecode runtime, not a second stable embedding API.

mod module;
mod report;
mod source;

pub use module::{ArtifactHost, ModuleGraph, ModuleGraphHost, ModuleHost, ModuleInstance};
pub use report::{
    CapabilityReport, ForeignCapability, ModuleCapabilities, ReportArity, ReportLocation,
    ReportParameter, ResourceCapability, capability_report,
};
pub use source::{
    InteractiveCompilation, InteractiveCompilerState, SourceDiagnostic, SourceEdit, SourceError,
    SourceErrorCode, SourceErrorKind, SourceFix, SourcePosition, SourceRange, SourceReadiness,
    apply_fix, compile, compile_interactive_forms, foreign_declarations, source_readiness,
};
