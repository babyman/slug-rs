//! Public-but-unstable in-process bytecode construction and installation types.
//!
//! The modules separate metadata, builder operations, chunk storage, and
//! program installation/validation. They remain one private bytecode boundary:
//! the portable .cslug contract remains distinct.

mod chunk;
mod metadata;
mod op;
mod program;

pub use chunk::Chunk;
pub use metadata::{
    Capture, CaptureListId, Constant, GlobalNameId, MatchMapKey, MatchPattern, MatchPatternId,
    MatchRest, MatchType, ModuleDeclaration, ModuleTag, ParameterSignature, SchemaField,
    SchemaFieldsId, SelectCase, SourceId, SourceSpan, SpanId, StructFieldsId,
};
pub use op::{CallArgumentKind, DeferMode, Instruction, Op};
pub use program::{BytecodeLayoutMetrics, Program};

pub(crate) use chunk::{CompiledChunk, PackedInstruction, PackedOpcode};
pub(crate) use program::{Entrypoint, EntrypointArguments};
