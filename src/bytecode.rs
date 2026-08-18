use std::collections::HashMap;

use crate::Value;

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
#[derive(Clone, Debug)]
pub enum MatchPattern {
    Literal(Value),
    Wildcard,
    Binding,
    List {
        items: Vec<MatchPattern>,
        rest: bool,
    },
    Map {
        entries: Vec<(String, MatchPattern)>,
        rest: bool,
        exact: bool,
    },
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
    DefineGlobal(String),
    SetGlobal(String),
    MakeClosure {
        chunk: usize,
        captures: Vec<Capture>,
    },
    List(usize),
    Map(usize),
    GetIndex,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Negate,
    Not,
    Equal,
    Greater,
    Less,
    Jump(usize),
    JumpIfFalse(usize),
    Call(usize),
    TryMatch {
        pattern: MatchPattern,
        bindings: usize,
    },
    MatchFailure,
    Throw,
    EnterScope,
    LeaveScope,
    Defer {
        mode: DeferMode,
    },
    Recur(usize),
    Return,
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
}
