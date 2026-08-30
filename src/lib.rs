//! Slug's clean-room bytecode runtime.
//!
//! This crate deliberately keeps its VM bytecode an implementation detail.
//! `Program`, `Chunk`, and `Op` are a compiler-to-VM boundary, designed for
//! clarity and validation rather than persistence. They are distinct from the
//! future portable `.cslug` compiled-module format documented in
//! `docs/compiled-artifacts.md`.

mod bytecode;
mod configuration;
mod conformance;
mod fixture;
mod module;
mod native;
mod scheduler_signal;
mod source;
mod value;
mod vm;

pub use bytecode::{
    CallArgumentKind, Capture, Chunk, Constant, DeferMode, Instruction, MatchMapKey, MatchPattern,
    MatchRest, MatchType, ModuleDeclaration, ModuleTag, Op, ParameterSignature, Program,
    SchemaField, SelectCase, SourceSpan,
};
pub use configuration::{Configuration, ConfigurationValue};
pub use conformance::FixtureRunner;
pub use fixture::{FixtureMetadata, FixtureMetadataError, FixtureOutcome};
pub use module::{ModuleInstance, ModuleLoadError, ModuleLoader, ModuleSource};
pub use native::{
    NativeArity, NativeCall, NativeChannelProducer, NativeDescriptorError, NativeError,
    NativeFunction, NativeModule, NativeOwnedValue, NativeProducerStatus, NativeResourceType,
    NativeSendValue, NativeStatus, NativeValueKind, NativeValueRef,
};
pub use source::{SourceError, SourceErrorKind, compile, compile_type_checked};
pub use value::{Builtin, Channel, Closure, StructField, StructSchema, StructValue, Task, Value};
pub use vm::{
    CallFrame, NativeErrorDetails, RuntimeError, RuntimeErrorKind, Vm, VmMetrics, VmResult,
};
