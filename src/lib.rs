//! Slug's clean-room bytecode runtime.
//!
//! This crate deliberately keeps its VM bytecode an implementation detail.
//! `Program`, `Chunk`, and `Op` are a compiler-to-VM boundary, designed for
//! clarity and validation rather than persistence. They are distinct from the
//! future portable `.cslug` compiled-module format documented in
//! `docs/compiled-artifacts.md`.

mod bytecode;
mod module;
mod source;
mod value;
mod vm;

pub use bytecode::{
    CallArgumentKind, Capture, Chunk, Constant, DeferMode, Instruction, MatchMapKey, MatchPattern,
    MatchRest, ModuleDeclaration, ModuleTag, Op, ParameterSignature, Program, SchemaField,
    SourceSpan,
};
pub use module::{ModuleInstance, ModuleLoadError, ModuleLoader, ModuleSource};
pub use source::{SourceError, SourceErrorKind, compile, compile_type_checked};
pub use value::{Closure, NativeFunction, StructField, StructSchema, StructValue, Value};
pub use vm::{CallFrame, RuntimeError, RuntimeErrorKind, Vm, VmResult};
