//! Slug's clean-room bytecode runtime.
//!
//! `Program`, `Chunk`, and `Op` are a public but unstable in-process Rust
//! embedding and testing surface. They are a compiler-to-VM boundary, designed
//! for clarity and validation rather than persistence or cross-version use.
//! They are distinct from the future portable `.cslug` compiled-module format
//! documented in `docs/reference/compiled-artifacts.md`.

mod bytecode;
mod collections;
mod conformance;
mod fixture;
mod native;
mod runtime_host;
mod scheduler_signal;
mod value;
mod vm;

#[doc(hidden)]
pub use bytecode::PackedOpcode;
/// Experimental in-process bytecode construction and inspection types.
///
/// These types are public so Rust hosts and integration tests can construct
/// programs for checked execution. Their layouts, variants, constructors, and
/// semantics may change in any pre-release version; do not serialize them or
/// treat them as a stable Rust API. `.cslug` is the future portable contract.
pub use bytecode::{
    BytecodeLayoutMetrics, CallArgumentKind, CallableIdentity, Capture, CaptureListId, Chunk,
    Constant, DeferMode, Entrypoint, EntrypointArguments, ForeignResourceSignature, GlobalNameId,
    Instruction, MatchMapKey, MatchPattern, MatchPatternId, MatchRest, MatchType,
    ModuleDeclaration, ModuleTag, Op, ParameterSignature, Program, ProgramBuilder, SchemaField,
    SchemaFieldsId, SelectCase, SourceId, SourceSpan, SpanId, StructFieldsId,
};
pub use conformance::FixtureRunner;
pub use fixture::{FixtureMetadata, FixtureMetadataError, FixtureOutcome};
#[doc(hidden)]
pub use native::native_resource_registry;
pub use native::{
    NativeArity, NativeCall, NativeChannelProducer, NativeDescriptorError, NativeEnumCase,
    NativeError, NativeFunction, NativeModule, NativeOwnedValue, NativeProducerStatus,
    NativeResourceRegistry, NativeResourceType, NativeSendValue, NativeStatus, NativeValueKind,
    NativeValueRef,
};
/// Experimental VM-to-host callbacks. This trait is not a stable embedding API.
pub use runtime_host::{
    EmptyVmConfiguration, VmConfiguration, VmHost, VmHostError, VmModuleExports,
};
#[doc(hidden)]
pub use slug_loader::{
    ModuleActivation, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
};
#[cfg(feature = "concurrency")]
pub use value::Task;
pub use value::{
    Builtin, Channel, Closure, EnumValue, StructField, StructSchema, StructValue, Value, ValueKind,
};
#[cfg(not(feature = "concurrency"))]
#[doc(hidden)]
pub use vm::InteractiveExecution;
#[cfg(feature = "concurrency")]
#[doc(hidden)]
pub use vm::InteractiveTask;
#[cfg(feature = "metrics")]
pub use vm::VmMetrics;
pub use vm::{
    CallFrame, InstalledProgram, InteractiveEnvironment, NativeErrorDetails, RuntimeError,
    RuntimeErrorKind, Vm, VmLayoutMetrics, VmProgress, VmResult,
};
