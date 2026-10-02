//! Source syntax, semantic analysis, lowering, and module-graph ownership.
//!
//! The public surface is deliberately pre-release. It is the one-way source
//! frontend for the private bytecode runtime, not a second stable embedding API.

mod module;
mod source;

pub use module::{ModuleGraph, ModuleGraphHost, ModuleHost, ModuleInstance};
pub use source::{
    InteractiveCompilation, InteractiveCompilerState, SourceError, SourceErrorKind,
    SourceReadiness, compile, compile_interactive_forms, source_readiness,
};
