use alloc::{string::String, vec::Vec};

use crate::platform::HashMap;

use crate::Value;

use super::{
    metadata::{Constant, ParameterSignature, SourceSpan, SpanId},
    op::{Instruction, Op},
};

/// The installation-time bytecode form.  It deliberately keeps the source
/// builder's rich `Op` values out of the executable instruction stream.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum PackedOpcode {
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
    AddNum,
    Subtract,
    SubtractNum,
    Multiply,
    MultiplyNum,
    Divide,
    DivideNum,
    Modulo,
    ModuloNum,
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
    GreaterNum,
    Less,
    LessNum,
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
    // Append new opcodes here: a `.cslug` artifact stores the ordinal as its tag.
    GetIndexConstant,
    JumpIfFalsePop,
    LocalConstantBinary,
    MatchCase,
}

impl PackedOpcode {
    pub(crate) fn from_cslug_tag(tag: u8) -> Option<Self> {
        const OPCODES: &[PackedOpcode] = &[
            PackedOpcode::Constant,
            PackedOpcode::Interpolate,
            PackedOpcode::Nil,
            PackedOpcode::True,
            PackedOpcode::False,
            PackedOpcode::Pop,
            PackedOpcode::Duplicate,
            PackedOpcode::GetLocal,
            PackedOpcode::SetLocal,
            PackedOpcode::GetCapture,
            PackedOpcode::SetCapture,
            PackedOpcode::GetGlobal,
            PackedOpcode::NotImplemented,
            PackedOpcode::DefineGlobal,
            PackedOpcode::CombineOverloads,
            PackedOpcode::DefineMapGlobals,
            PackedOpcode::RecordModuleTag,
            PackedOpcode::SetGlobal,
            PackedOpcode::MakeClosure,
            PackedOpcode::List,
            PackedOpcode::ListSpread,
            PackedOpcode::Map,
            PackedOpcode::StructSchema,
            PackedOpcode::Struct,
            PackedOpcode::StructCopy,
            PackedOpcode::GetIndex,
            PackedOpcode::GetSlice,
            PackedOpcode::Add,
            PackedOpcode::AddNum,
            PackedOpcode::Subtract,
            PackedOpcode::SubtractNum,
            PackedOpcode::Multiply,
            PackedOpcode::MultiplyNum,
            PackedOpcode::Divide,
            PackedOpcode::DivideNum,
            PackedOpcode::Modulo,
            PackedOpcode::ModuloNum,
            PackedOpcode::BitAnd,
            PackedOpcode::BitOr,
            PackedOpcode::BitXor,
            PackedOpcode::ShiftLeft,
            PackedOpcode::ShiftRight,
            PackedOpcode::ListAppend,
            PackedOpcode::ListPrepend,
            PackedOpcode::Negate,
            PackedOpcode::Not,
            PackedOpcode::BitNot,
            PackedOpcode::Equal,
            PackedOpcode::Greater,
            PackedOpcode::GreaterNum,
            PackedOpcode::Less,
            PackedOpcode::LessNum,
            PackedOpcode::GuardGreater,
            PackedOpcode::GuardLess,
            PackedOpcode::Jump,
            PackedOpcode::JumpIfFalse,
            PackedOpcode::JumpIfProvided,
            PackedOpcode::Call,
            PackedOpcode::CallPositional,
            PackedOpcode::CallSpread,
            PackedOpcode::CallSelected,
            PackedOpcode::PipelineCall,
            PackedOpcode::PipelineCallSelected,
            PackedOpcode::Import,
            PackedOpcode::Spawn,
            PackedOpcode::Nursery,
            PackedOpcode::Select,
            PackedOpcode::SelectApply,
            PackedOpcode::TryMatch,
            PackedOpcode::MatchFailure,
            PackedOpcode::Throw,
            PackedOpcode::EnterScope,
            PackedOpcode::LeaveScope,
            PackedOpcode::Defer,
            PackedOpcode::Recur,
            PackedOpcode::RecurPositional,
            PackedOpcode::Return,
            PackedOpcode::GetIndexConstant,
            PackedOpcode::JumpIfFalsePop,
            PackedOpcode::LocalConstantBinary,
            PackedOpcode::MatchCase,
        ];
        OPCODES.get(usize::from(tag)).copied()
    }
}

/// Fixed-width executable instruction. `a`, `b`, and `c` are opcode-specific
/// private operands; rich data lives in `Program` pools.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub struct PackedInstruction {
    #[doc(hidden)]
    pub opcode: PackedOpcode,
    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) c: u32,
    pub(crate) span: Option<SpanId>,
}

impl PackedInstruction {
    pub(crate) const fn from_cslug(
        opcode: PackedOpcode,
        a: u32,
        b: u32,
        c: u32,
        span: Option<SpanId>,
    ) -> Self {
        Self {
            opcode,
            a,
            b,
            c,
            span,
        }
    }

    #[allow(dead_code)] // Consumed by the in-progress `.cslug` encoder.
    pub(crate) const fn cslug_parts(self) -> (PackedOpcode, u32, u32, u32, Option<SpanId>) {
        (self.opcode, self.a, self.b, self.c, self.span)
    }
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct CompiledChunk {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) parameters: Vec<ParameterSignature>,
    /// Whether a call that supplies all `arity` arguments positionally binds
    /// them straight into their slots: true unless a parameter is variadic.
    /// Defaults do not matter, since every parameter is supplied.
    pub(crate) binds_full_positional_calls: bool,
    pub(crate) callable_identity: Option<usize>,
    pub(crate) locals: usize,
    pub(crate) constants: Vec<Constant>,
    #[doc(hidden)]
    pub code: Vec<PackedInstruction>,
    pub(crate) invalid_instructions: HashMap<usize, String>,
    /// For each constant, the map entry position where a `GetIndexConstant`
    /// with that key last found it: checked first, then refreshed by a full
    /// lookup when the key is not there. Lowering gives each occurrence of a
    /// key its own constant, so this is in effect one hint per lookup site;
    /// sites that share a constant still look up correctly.
    pub(crate) index_hints: alloc::boxed::Box<[core::cell::Cell<u32>]>,
}

/// One unused hint per constant.
pub(crate) fn index_hints(constants: usize) -> alloc::boxed::Box<[core::cell::Cell<u32>]> {
    (0..constants).map(|_| core::cell::Cell::new(0)).collect()
}

impl CompiledChunk {
    pub(crate) fn from_cslug(
        name: String,
        arity: usize,
        parameters: Vec<ParameterSignature>,
        callable_identity: Option<usize>,
        locals: usize,
        constants: Vec<Constant>,
        code: Vec<PackedInstruction>,
    ) -> Self {
        Self {
            name,
            arity,
            binds_full_positional_calls: !parameters.iter().any(|parameter| parameter.variadic),
            parameters,
            callable_identity,
            locals,
            index_hints: index_hints(constants.len()),
            constants,
            code,
            invalid_instructions: HashMap::new(),
        }
    }

    #[allow(clippy::type_complexity, dead_code)] // Consumed by the in-progress `.cslug` encoder.
    pub(crate) fn cslug_parts(
        &self,
    ) -> (
        &str,
        usize,
        &[ParameterSignature],
        Option<usize>,
        usize,
        &[Constant],
        &[PackedInstruction],
    ) {
        (
            &self.name,
            self.arity,
            &self.parameters,
            self.callable_identity,
            self.locals,
            &self.constants,
            &self.code,
        )
    }
}

/// Independently callable code and its constant pool.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub name: String,
    pub arity: usize,
    pub parameters: Vec<ParameterSignature>,
    #[doc(hidden)]
    pub callable_identity: Option<usize>,
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
