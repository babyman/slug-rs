//! Slug's clean-room bytecode runtime.
//!
//! `Program`, `Chunk`, and `Op` are a public but unstable in-process Rust
//! embedding and testing surface. They are a compiler-to-VM boundary, designed
//! for clarity and validation rather than persistence or cross-version use.
//! They are distinct from the future portable `.cslug` compiled-module format
//! documented in `docs/reference/compiled-artifacts.md`.

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

/// Experimental in-process bytecode construction and inspection types.
///
/// These types are public so Rust hosts and integration tests can construct
/// programs for checked execution. Their layouts, variants, constructors, and
/// semantics may change in any pre-release version; do not serialize them or
/// treat them as a stable Rust API. `.cslug` is the future portable contract.
pub use bytecode::{
    BytecodeLayoutMetrics, CallArgumentKind, Capture, CaptureListId, Chunk, Constant, DeferMode,
    GlobalNameId, Instruction, MatchMapKey, MatchPattern, MatchPatternId, MatchRest, MatchType,
    ModuleDeclaration, ModuleTag, Op, ParameterSignature, Program, SchemaField, SchemaFieldsId,
    SelectCase, SourceId, SourceSpan, SpanId, StructFieldsId,
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
#[cfg(feature = "metrics")]
pub use vm::VmMetrics;
pub use vm::{
    CallFrame, NativeErrorDetails, RuntimeError, RuntimeErrorKind, Vm, VmLayoutMetrics, VmResult,
};
