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
    GetSlice {
        has_start: bool,
        has_end: bool,
        has_step: bool,
    },
    Add,
    /// Numeric-only addition selected from compiler type facts.
    AddNum,
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
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfProvided {
        slot: usize,
        target: usize,
    },
    Call(usize),
    /// A source call whose arguments are syntactically positional.
    CallPositional(usize),
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
