use std::collections::HashMap;

use crate::Value;

use super::{
    metadata::{Constant, ParameterSignature, SourceSpan, SpanId},
    op::{Instruction, Op},
};

/// The installation-time bytecode form.  It deliberately keeps the source
/// builder's rich `Op` values out of the executable instruction stream.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedOpcode {
    Constant,
    Interpolate,
    Nil,
    True,
    False,
    Pop,
    Duplicate,
    GetLocal,
    SetLocal,
    GetCapture,
    SetCapture,
    GetGlobal,
    NotImplemented,
    DefineGlobal,
    CombineOverloads,
    DefineMapGlobals,
    RecordModuleTag,
    SetGlobal,
    MakeClosure,
    List,
    ListSpread,
    Map,
    StructSchema,
    Struct,
    StructCopy,
    GetIndex,
    GetSlice,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    ListAppend,
    ListPrepend,
    Negate,
    Not,
    BitNot,
    Equal,
    Greater,
    Less,
    GuardGreater,
    GuardLess,
    Jump,
    JumpIfFalse,
    JumpIfProvided,
    Call,
    CallPositional,
    CallSpread,
    CallSelected,
    PipelineCall,
    PipelineCallSelected,
    Import,
    Spawn,
    Nursery,
    Select,
    SelectApply,
    TryMatch,
    MatchFailure,
    Throw,
    EnterScope,
    LeaveScope,
    Defer,
    Recur,
    RecurPositional,
    Return,
}

/// Fixed-width executable instruction. `a`, `b`, and `c` are opcode-specific
/// private operands; rich data lives in `Program` pools.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedInstruction {
    pub(crate) opcode: PackedOpcode,
    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) c: u32,
    pub(crate) span: Option<SpanId>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledChunk {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) parameters: Vec<ParameterSignature>,
    pub(crate) callable_identity: Option<usize>,
    pub(crate) locals: usize,
    pub(crate) constants: Vec<Constant>,
    pub(crate) code: Vec<PackedInstruction>,
    pub(crate) invalid_instructions: HashMap<usize, String>,
}

/// Independently callable code and its constant pool.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub name: String,
    pub arity: usize,
    pub parameters: Vec<ParameterSignature>,
    pub(crate) callable_identity: Option<usize>,
    /// Number of frame-local slots, including parameters.
    pub locals: usize,
    pub constants: Vec<Constant>,
    pub code: Vec<Instruction>,
    pub(crate) spans: Vec<SourceSpan>,
    pub(crate) span_ids: HashMap<SourceSpan, SpanId>,
}

impl Chunk {
    #[must_use]
    pub fn new(name: impl Into<String>, arity: usize) -> Self {
        Self {
            name: name.into(),
            arity,
            parameters: Vec::new(),
            callable_identity: None,
            locals: arity,
            constants: Vec::new(),
            code: Vec::new(),
            spans: Vec::new(),
            span_ids: HashMap::new(),
        }
    }

    pub fn constant(&mut self, value: Value) -> usize {
        self.constants.push(Constant::Value(value));
        self.constants.len() - 1
    }

    pub fn emit(&mut self, op: Op) -> &mut Self {
        self.code.push(Instruction::new(op));
        self
    }

    pub fn emit_at(&mut self, op: Op, span: SourceSpan) -> &mut Self {
        let span = self.intern_span(span);
        self.code.push(Instruction::new(op).at(span));
        self
    }

    fn intern_span(&mut self, span: SourceSpan) -> SpanId {
        if let Some(id) = self.span_ids.get(&span) {
            return *id;
        }
        let id = SpanId(
            u32::try_from(self.spans.len()).expect("private chunk has too many source spans"),
        );
        self.spans.push(span.clone());
        self.span_ids.insert(span, id);
        id
    }
}
