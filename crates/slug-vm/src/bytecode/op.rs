use alloc::{string::String, vec::Vec};

use super::metadata::{
    CallArgumentsId, Capture, CaptureListId, GlobalNameId, InterpolationId, ListSpreadId,
    MatchPattern, MatchPatternId, SchemaField, SchemaFieldsId, SelectCase, SelectCasesId,
    SelectedCallId, SpanId, StructFieldsId,
};

/// One VM instruction. Opcode numbers are intentionally not stable.
#[derive(Clone, Debug)]
pub struct Instruction {
    pub op: Op,
    pub span: Option<SpanId>,
}

impl Instruction {
    #[must_use]
    pub fn new(op: Op) -> Self {
        Self { op, span: None }
    }

    #[must_use]
    pub fn at(mut self, span: SpanId) -> Self {
        self.span = Some(span);
        self
    }
}

/// Stack-machine operations emitted by a future Slug compiler.
#[derive(Clone, Debug)]
pub enum Op {
    Constant(usize),
    Interpolate(Vec<String>),
    InterpolatePooled(InterpolationId),
    Nil,
    True,
    False,
    Pop,
    Duplicate,
    GetLocal(usize),
    SetLocal(usize),
    GetCapture(usize),
    SetCapture(usize),
    GetGlobal(String),
    GetGlobalPooled(GlobalNameId),
    NotImplemented,
    DefineGlobal(String),
    DefineGlobalPooled(GlobalNameId),
    /// Combines the existing callable value above the new callable below it.
    CombineOverloads,
    /// Defines globals from the string keys of the map on top of the stack.
    DefineMapGlobals,
    RecordModuleTag {
        declaration: usize,
        tag: usize,
        arguments: usize,
    },
    SetGlobal(String),
    SetGlobalPooled(GlobalNameId),
    MakeClosure {
        chunk: usize,
        captures: Vec<Capture>,
    },
    MakeClosurePooled {
        chunk: usize,
        captures: CaptureListId,
    },
    List(usize),
    ListSpread(Vec<bool>),
    ListSpreadPooled(ListSpreadId),
    Map(usize),
    StructSchema(Vec<SchemaField>),
    StructSchemaPooled(SchemaFieldsId),
    Struct(Vec<String>),
    StructPooled(StructFieldsId),
    StructCopy(Vec<String>),
    StructCopyPooled(StructFieldsId),
    GetIndex,
    /// `GetIndex` whose key is a value constant of the current chunk.
    GetIndexConstant(usize),
    GetSlice {
        has_start: bool,
        has_end: bool,
        has_step: bool,
    },
    Add,
    /// Numeric-only addition selected from compiler type facts.
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
    Jump(usize),
    JumpIfFalse(usize),
    /// Pops the condition, then jumps when it is falsy.
    JumpIfFalsePop(usize),
    /// Pushes `operator` applied to a local and a value constant: the fused
    /// form of `GetLocal(slot)`, `Constant(constant)`, `operator`.
    LocalConstantBinary {
        slot: usize,
        constant: usize,
        operator: BinaryOperator,
    },
    JumpIfProvided {
        slot: usize,
        target: usize,
    },
    Call(usize),
    /// A source call whose arguments are syntactically positional.
    CallPositional(usize),
    /// A positional call of a module-level name, whose `count` arguments are
    /// on the stack without the callee: it reads the name itself.
    CallGlobal {
        name: String,
        count: usize,
    },
    CallGlobalPooled {
        name: GlobalNameId,
        count: usize,
    },
    CallSpread(Vec<CallArgumentKind>),
    CallSpreadPooled(CallArgumentsId),
    CallSelected {
        kinds: Vec<CallArgumentKind>,
        identity: usize,
    },
    CallSelectedPooled(SelectedCallId),
    PipelineCall(Vec<CallArgumentKind>),
    PipelineCallPooled(CallArgumentsId),
    PipelineCallSelected {
        kinds: Vec<CallArgumentKind>,
        identity: usize,
    },
    PipelineCallSelectedPooled(SelectedCallId),
    Import(Vec<CallArgumentKind>),
    ImportPooled(CallArgumentsId),
    Spawn,
    Nursery {
        has_limit: bool,
    },
    Select(Vec<SelectCase>),
    SelectPooled(SelectCasesId),
    /// Applies the selected case's optional handler to its result.
    SelectApply,
    TryMatch {
        pattern: MatchPattern,
        bindings: usize,
        operands: usize,
    },
    TryMatchPooled {
        pattern: MatchPatternId,
        bindings: usize,
        operands: usize,
    },
    /// Matches the value under its `operands` without consuming it. A match
    /// pushes the pattern's bindings; a failure pushes nothing and jumps to
    /// `target`.
    MatchCase {
        pattern: MatchPattern,
        operands: usize,
        target: usize,
    },
    MatchCasePooled {
        pattern: MatchPatternId,
        operands: usize,
        target: usize,
    },
    MatchFailure,
    Throw,
    EnterScope,
    LeaveScope,
    Defer {
        mode: DeferMode,
    },
    Recur(Vec<CallArgumentKind>),
    RecurPooled(CallArgumentsId),
    RecurPositional(usize),
    Return,
}

/// The source ordering and expansion mode for a dynamic call argument.
#[derive(Clone, Debug)]
pub enum CallArgumentKind {
    Positional,
    Spread,
    Named(String),
}

/// The condition under which a deferred action runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferMode {
    Always,
    Success,
    Error,
}

/// An operator a fused `LocalConstantBinary` instruction can apply. Each one
/// behaves exactly like its standalone instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
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
    Equal,
    Greater,
    GreaterNum,
    Less,
    LessNum,
}

impl BinaryOperator {
    const ALL: [Self; 15] = [
        Self::Add,
        Self::AddNum,
        Self::Subtract,
        Self::SubtractNum,
        Self::Multiply,
        Self::MultiplyNum,
        Self::Divide,
        Self::DivideNum,
        Self::Modulo,
        Self::ModuloNum,
        Self::Equal,
        Self::Greater,
        Self::GreaterNum,
        Self::Less,
        Self::LessNum,
    ];

    /// The fusable operator a standalone instruction performs, if any.
    #[must_use]
    pub fn of(op: &Op) -> Option<Self> {
        Some(match op {
            Op::Add => Self::Add,
            Op::AddNum => Self::AddNum,
            Op::Subtract => Self::Subtract,
            Op::SubtractNum => Self::SubtractNum,
            Op::Multiply => Self::Multiply,
            Op::MultiplyNum => Self::MultiplyNum,
            Op::Divide => Self::Divide,
            Op::DivideNum => Self::DivideNum,
            Op::Modulo => Self::Modulo,
            Op::ModuloNum => Self::ModuloNum,
            Op::Equal => Self::Equal,
            Op::Greater => Self::Greater,
            Op::GreaterNum => Self::GreaterNum,
            Op::Less => Self::Less,
            Op::LessNum => Self::LessNum,
            _ => return None,
        })
    }

    pub(crate) fn code(self) -> u32 {
        Self::ALL
            .iter()
            .position(|operator| *operator == self)
            .and_then(|index| u32::try_from(index).ok())
            .expect("operator is listed")
    }

    pub(crate) fn from_code(code: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }
}
