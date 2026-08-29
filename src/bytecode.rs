use std::collections::HashMap;

use crate::{Value, source::environment::ModuleSnapshot};

/// A source position attached to an instruction for language diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

impl SourceSpan {
    #[must_use]
    pub fn new(path: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            path: path.into(),
            line,
            column,
        }
    }
}

/// A literal embedded in a bytecode chunk.
#[derive(Clone, Debug)]
pub enum Constant {
    Value(Value),
    Function(usize),
}

/// The enclosing slot from which a closure captures a value.
#[derive(Clone, Debug)]
pub enum Capture {
    Local(usize),
    Capture(usize),
}

/// The subset of source patterns lowered by the current compiler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchRest {
    None,
    Discard,
    Binding,
}

#[derive(Clone, Debug)]
pub enum MatchMapKey {
    String(String),
    Operand(usize),
}

#[derive(Clone, Debug)]
pub struct SchemaField {
    pub name: String,
    pub has_default: bool,
}

/// Private callable metadata used by source-call binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterSignature {
    pub name: String,
    pub has_default: bool,
    pub variadic: bool,
}

/// Retained evaluated tag metadata for a top-level source declaration.
#[derive(Clone, Debug)]
pub struct ModuleTag {
    pub name: String,
    pub arguments: Vec<Value>,
}

/// Retained source metadata for a top-level declaration.
#[derive(Clone, Debug)]
pub struct ModuleDeclaration {
    pub bindings: Vec<String>,
    pub mutable: bool,
    pub exported: bool,
    /// Whether this declaration must be supplied by the module-qualified host registry.
    pub foreign: bool,
    /// The inclusive declared call-arity range for a foreign binding. `None`
    /// denotes a variadic declaration.
    pub foreign_arity: Option<(usize, Option<usize>)>,
    pub documentation: Option<String>,
    pub tags: Vec<ModuleTag>,
}

/// The subset of source patterns lowered by the current compiler.
#[derive(Clone, Debug)]
pub enum MatchPattern {
    Literal(Value),
    Wildcard,
    Binding,
    Pinned(usize),
    At(Box<MatchPattern>),
    Alternatives(Vec<MatchPattern>),
    List {
        items: Vec<MatchPattern>,
        rest: MatchRest,
    },
    Map {
        entries: Vec<(MatchMapKey, MatchPattern)>,
        rest: MatchRest,
        exact: bool,
    },
    Struct {
        schema: usize,
        fields: Vec<(String, MatchPattern)>,
    },
}

/// One source-order case consumed by the private select instruction.
#[derive(Clone, Debug)]
pub enum SelectCase {
    Receive { has_handler: bool },
    Send { has_handler: bool },
    After { has_handler: bool },
    Await { has_handler: bool },
    Default { has_handler: bool },
}

/// One VM instruction. Opcode numbers are intentionally not stable.
#[derive(Clone, Debug)]
pub struct Instruction {
    pub op: Op,
    pub span: Option<SourceSpan>,
}

impl Instruction {
    #[must_use]
    pub fn new(op: Op) -> Self {
        Self { op, span: None }
    }

    #[must_use]
    pub fn at(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }
}

/// Stack-machine operations emitted by a future Slug compiler.
#[derive(Clone, Debug)]
pub enum Op {
    Constant(usize),
    Interpolate(Vec<String>),
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
    NotImplemented,
    DefineGlobal(String),
    /// Defines globals from the string keys of the map on top of the stack.
    DefineMapGlobals,
    RecordModuleTag {
        declaration: usize,
        tag: usize,
        arguments: usize,
    },
    SetGlobal(String),
    MakeClosure {
        chunk: usize,
        captures: Vec<Capture>,
    },
    List(usize),
    ListSpread(Vec<bool>),
    Map(usize),
    StructSchema(Vec<SchemaField>),
    Struct(Vec<String>),
    StructCopy(Vec<String>),
    GetIndex,
    GetSlice {
        has_start: bool,
        has_end: bool,
        has_step: bool,
    },
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
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfProvided {
        slot: usize,
        target: usize,
    },
    Call(usize),
    CallSpread(Vec<CallArgumentKind>),
    PipelineCall(Vec<CallArgumentKind>),
    Import(Vec<CallArgumentKind>),
    Spawn,
    Nursery {
        has_limit: bool,
    },
    Select(Vec<SelectCase>),
    /// Applies the selected case's optional handler to its result.
    SelectApply,
    TryMatch {
        pattern: MatchPattern,
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

/// Independently callable code and its constant pool.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub name: String,
    pub arity: usize,
    pub parameters: Vec<ParameterSignature>,
    /// Number of frame-local slots, including parameters.
    pub locals: usize,
    pub constants: Vec<Constant>,
    pub code: Vec<Instruction>,
}

impl Chunk {
    #[must_use]
    pub fn new(name: impl Into<String>, arity: usize) -> Self {
        Self {
            name: name.into(),
            arity,
            parameters: Vec::new(),
            locals: arity,
            constants: Vec::new(),
            code: Vec::new(),
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
        self.code.push(Instruction::new(op).at(span));
        self
    }
}

/// All code available to a VM invocation.
#[derive(Clone, Debug, Default)]
pub struct Program {
    chunks: Vec<Chunk>,
    names: HashMap<String, usize>,
    bindings: Vec<String>,
    declarations: Vec<ModuleDeclaration>,
    exports: Vec<String>,
    has_entrypoint: bool,
    module_name: String,
    semantic_snapshot: ModuleSnapshot,
}

impl Program {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_chunk(&mut self, chunk: Chunk) -> usize {
        let index = self.chunks.len();
        self.names.insert(chunk.name.clone(), index);
        self.chunks.push(chunk);
        index
    }

    #[must_use]
    pub fn chunk(&self, index: usize) -> Option<&Chunk> {
        self.chunks.get(index)
    }

    #[must_use]
    pub fn find_chunk(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Names declared for export by a compiled source module.
    #[must_use]
    pub fn exports(&self) -> &[String] {
        &self.exports
    }

    /// Statically knowable top-level bindings in a source module.
    #[must_use]
    pub fn bindings(&self) -> &[String] {
        &self.bindings
    }

    #[must_use]
    pub fn declarations(&self) -> &[ModuleDeclaration] {
        &self.declarations
    }

    /// Whether this program declares a local top-level zero-argument `main`.
    #[must_use]
    pub fn has_entrypoint(&self) -> bool {
        self.has_entrypoint
    }

    /// Fully-qualified module name used for module-relative host services.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    pub(crate) fn set_bindings(&mut self, bindings: Vec<String>) {
        self.bindings = bindings;
    }

    pub(crate) fn set_declarations(&mut self, declarations: Vec<ModuleDeclaration>) {
        self.declarations = declarations;
    }

    pub(crate) fn set_exports(&mut self, exports: Vec<String>) {
        self.exports = exports;
    }

    pub(crate) fn set_has_entrypoint(&mut self, has_entrypoint: bool) {
        self.has_entrypoint = has_entrypoint;
    }

    pub(crate) fn semantic_snapshot(&self) -> &ModuleSnapshot {
        &self.semantic_snapshot
    }

    pub(crate) fn set_semantic_snapshot(&mut self, snapshot: ModuleSnapshot) {
        self.semantic_snapshot = snapshot;
    }

    /// Sets the module name used by module-relative host services.
    pub fn set_module_name(&mut self, module_name: impl Into<String>) {
        self.module_name = module_name.into();
    }
}
