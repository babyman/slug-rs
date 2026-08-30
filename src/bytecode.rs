use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    mem,
    sync::Arc,
};

use crate::{
    Value,
    source::environment::{CallableIdentity, ModuleSnapshot},
};

/// A source position attached to an instruction for language diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceSpan {
    pub path: Arc<str>,
    pub line: u32,
    pub column: u32,
}

impl SourceSpan {
    #[must_use]
    pub fn new(path: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            path: Arc::from(path.into()),
            line,
            column,
        }
    }
}

/// Private source-path table index used by bytecode span metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(u32);

impl SourceId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Private source-span table index used by bytecode instructions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpanId(u32);

impl SpanId {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

macro_rules! metadata_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name(u32);

        impl $name {
            #[must_use]
            pub const fn new(index: u32) -> Self {
                Self(index)
            }

            fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

metadata_id!(GlobalNameId);
metadata_id!(CaptureListId);
metadata_id!(SchemaFieldsId);
metadata_id!(StructFieldsId);
metadata_id!(MatchPatternId);

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
    /// Private canonical callable identity for a resolved foreign binding.
    pub(crate) foreign_callable_identity: Option<CallableIdentity>,
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
    Constrained {
        pattern: Box<MatchPattern>,
        constraint: MatchType,
    },
}

/// Private runtime-checkable type form used by source match patterns.
#[derive(Clone, Debug)]
pub enum MatchType {
    Any,
    Nil,
    Bool,
    Num,
    Str,
    Bytes,
    List(Option<Box<MatchType>>),
    Map(Option<(Box<MatchType>, Box<MatchType>)>),
    Function,
    Task,
    Channel,
    Schema,
    Struct(Option<usize>),
    Union(Vec<MatchType>),
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
    CallSelected {
        kinds: Vec<CallArgumentKind>,
        identity: usize,
    },
    PipelineCall(Vec<CallArgumentKind>),
    PipelineCallSelected {
        kinds: Vec<CallArgumentKind>,
        identity: usize,
    },
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StackValue {
    Unknown,
    MatchBinding,
    MatchResult,
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
    spans: Vec<SourceSpan>,
    span_ids: HashMap<SourceSpan, SpanId>,
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
    callable_identities: Vec<CallableIdentity>,
    sources: Vec<Arc<str>>,
    source_ids: HashMap<Arc<str>, SourceId>,
    spans: Vec<SourceSpan>,
    span_ids: HashMap<SourceSpan, SpanId>,
    global_names: Vec<String>,
    capture_lists: Vec<Vec<Capture>>,
    schema_fields: Vec<Vec<SchemaField>>,
    struct_fields: Vec<Vec<String>>,
    match_patterns: Vec<MatchPattern>,
}

/// Layout measurements for private bytecode metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeLayoutMetrics {
    pub instructions: usize,
    pub instruction_bytes: usize,
    pub span_table_entries: usize,
    pub inline_span_bytes: usize,
    pub compressed_span_map_bytes: usize,
}

impl Program {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_chunk(&mut self, mut chunk: Chunk) -> usize {
        let span_remap = chunk
            .spans
            .drain(..)
            .map(|span| self.intern_span(span))
            .collect::<Vec<_>>();
        for instruction in &mut chunk.code {
            if let Some(span) = instruction.span {
                instruction.span = span_remap.get(span.index()).copied().or(Some(span));
            }
            self.pool_instruction_metadata(&mut instruction.op);
        }
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

    /// Number of interned source paths used by private bytecode metadata.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Number of interned source spans used by private bytecode metadata.
    #[must_use]
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// Returns deterministic layout measurements for private bytecode metadata.
    #[must_use]
    pub fn layout_metrics(&self) -> BytecodeLayoutMetrics {
        let instructions = self.chunks.iter().map(|chunk| chunk.code.len()).sum();
        let span_runs = self
            .chunks
            .iter()
            .map(|chunk| {
                let mut runs = 0usize;
                let mut previous = None;
                for instruction in &chunk.code {
                    if instruction.span != previous {
                        runs += 1;
                        previous = instruction.span;
                    }
                }
                runs
            })
            .sum::<usize>();
        BytecodeLayoutMetrics {
            instructions,
            instruction_bytes: instructions * std::mem::size_of::<Instruction>(),
            span_table_entries: self.spans.len(),
            inline_span_bytes: instructions * std::mem::size_of::<Option<SpanId>>(),
            compressed_span_map_bytes: span_runs * (std::mem::size_of::<u32>() * 2),
        }
    }

    fn intern_span(&mut self, span: SourceSpan) -> SpanId {
        let source = self.intern_source(span.path);
        let span = SourceSpan {
            path: self.sources[source.index()].clone(),
            line: span.line,
            column: span.column,
        };
        if let Some(id) = self.span_ids.get(&span) {
            return *id;
        }
        let id = SpanId(
            u32::try_from(self.spans.len()).expect("private program has too many source spans"),
        );
        self.spans.push(span.clone());
        self.span_ids.insert(span, id);
        id
    }

    fn intern_source(&mut self, path: Arc<str>) -> SourceId {
        if let Some(id) = self.source_ids.get(&path) {
            return *id;
        }
        let id = SourceId(
            u32::try_from(self.sources.len()).expect("private program has too many source paths"),
        );
        self.sources.push(path.clone());
        self.source_ids.insert(path, id);
        id
    }

    pub(crate) fn span(&self, id: SpanId) -> Option<&SourceSpan> {
        self.spans.get(id.index())
    }

    pub(crate) fn global_name(&self, id: GlobalNameId) -> Option<&str> {
        self.global_names.get(id.index()).map(String::as_str)
    }

    pub(crate) fn capture_list(&self, id: CaptureListId) -> Option<&[Capture]> {
        self.capture_lists.get(id.index()).map(Vec::as_slice)
    }

    pub(crate) fn schema_fields(&self, id: SchemaFieldsId) -> Option<&[SchemaField]> {
        self.schema_fields.get(id.index()).map(Vec::as_slice)
    }

    pub(crate) fn struct_fields(&self, id: StructFieldsId) -> Option<&[String]> {
        self.struct_fields.get(id.index()).map(Vec::as_slice)
    }

    pub(crate) fn match_pattern(&self, id: MatchPatternId) -> Option<&MatchPattern> {
        self.match_patterns.get(id.index())
    }

    fn pool_instruction_metadata(&mut self, op: &mut Op) {
        let pooled = match op {
            Op::GetGlobal(name) => Some(Op::GetGlobalPooled(
                self.intern_global_name(mem::take(name)),
            )),
            Op::DefineGlobal(name) => Some(Op::DefineGlobalPooled(
                self.intern_global_name(mem::take(name)),
            )),
            Op::SetGlobal(name) => Some(Op::SetGlobalPooled(
                self.intern_global_name(mem::take(name)),
            )),
            Op::MakeClosure { chunk, captures } => Some(Op::MakeClosurePooled {
                chunk: *chunk,
                captures: self.push_capture_list(mem::take(captures)),
            }),
            Op::StructSchema(fields) => Some(Op::StructSchemaPooled(
                self.push_schema_fields(mem::take(fields)),
            )),
            Op::Struct(fields) => {
                Some(Op::StructPooled(self.push_struct_fields(mem::take(fields))))
            }
            Op::StructCopy(fields) => Some(Op::StructCopyPooled(
                self.push_struct_fields(mem::take(fields)),
            )),
            Op::TryMatch {
                pattern,
                bindings,
                operands,
            } => Some(Op::TryMatchPooled {
                pattern: self.push_match_pattern(mem::replace(pattern, MatchPattern::Wildcard)),
                bindings: *bindings,
                operands: *operands,
            }),
            _ => None,
        };
        if let Some(pooled) = pooled {
            *op = pooled;
        }
    }

    fn intern_global_name(&mut self, name: String) -> GlobalNameId {
        if let Some(index) = self
            .global_names
            .iter()
            .position(|existing| existing == &name)
        {
            return GlobalNameId(u32::try_from(index).expect("private program has too many names"));
        }
        let id = GlobalNameId(
            u32::try_from(self.global_names.len()).expect("private program has too many names"),
        );
        self.global_names.push(name);
        id
    }

    fn push_capture_list(&mut self, captures: Vec<Capture>) -> CaptureListId {
        let id = CaptureListId(
            u32::try_from(self.capture_lists.len())
                .expect("private program has too many capture lists"),
        );
        self.capture_lists.push(captures);
        id
    }

    fn push_schema_fields(&mut self, fields: Vec<SchemaField>) -> SchemaFieldsId {
        let id = SchemaFieldsId(
            u32::try_from(self.schema_fields.len())
                .expect("private program has too many schema field lists"),
        );
        self.schema_fields.push(fields);
        id
    }

    fn push_struct_fields(&mut self, fields: Vec<String>) -> StructFieldsId {
        let id = StructFieldsId(
            u32::try_from(self.struct_fields.len())
                .expect("private program has too many struct field lists"),
        );
        self.struct_fields.push(fields);
        id
    }

    fn push_match_pattern(&mut self, pattern: MatchPattern) -> MatchPatternId {
        let id = MatchPatternId(
            u32::try_from(self.match_patterns.len())
                .expect("private program has too many match patterns"),
        );
        self.match_patterns.push(pattern);
        id
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

    pub(crate) fn callable_identity(&self, index: usize) -> Option<&CallableIdentity> {
        self.callable_identities.get(index)
    }

    pub(crate) fn set_callable_identities(&mut self, identities: Vec<CallableIdentity>) {
        self.callable_identities = identities;
    }

    /// Sets the module name used by module-relative host services.
    pub fn set_module_name(&mut self, module_name: impl Into<String>) {
        self.module_name = module_name.into();
    }

    pub(crate) fn validate(&self, entry: usize) -> Result<(), String> {
        for (chunk_index, chunk) in self.chunks.iter().enumerate() {
            if chunk.locals < chunk.arity {
                return Err(format!(
                    "function `{}` has {} local slots for {} parameters",
                    chunk.name, chunk.locals, chunk.arity
                ));
            }
            if !chunk.parameters.is_empty() && chunk.parameters.len() != chunk.arity {
                return Err(format!(
                    "function `{}` has {} parameter metadata entries for {} parameters",
                    chunk.name,
                    chunk.parameters.len(),
                    chunk.arity
                ));
            }
            for constant in &chunk.constants {
                if let Constant::Function(target) = constant
                    && self.chunk(*target).is_none()
                {
                    return Err(format!(
                        "function `{}` references missing function chunk {target}",
                        chunk.name
                    ));
                }
            }
            for (instruction_index, instruction) in chunk.code.iter().enumerate() {
                if let Some(span) = instruction.span
                    && self.span(span).is_none()
                {
                    return Err(format!(
                        "function `{}` instruction {instruction_index} references missing source span {}",
                        chunk.name,
                        span.index()
                    ));
                }
                self.validate_op(chunk_index, instruction_index, chunk, &instruction.op)?;
            }
            let initial_stack = if chunk_index == entry {
                0
            } else {
                chunk
                    .arity
                    .checked_add(1)
                    .ok_or_else(|| format!("function `{}` has too many parameters", chunk.name))?
            };
            self.validate_stack(chunk, initial_stack)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn validate_op(
        &self,
        _chunk_index: usize,
        instruction_index: usize,
        chunk: &Chunk,
        op: &Op,
    ) -> Result<(), String> {
        let location = || format!("function `{}` instruction {instruction_index}", chunk.name);
        match op {
            Op::Constant(index) if chunk.constants.get(*index).is_none() => Err(format!(
                "{} references missing constant {index}",
                location()
            )),
            Op::GetLocal(slot) | Op::SetLocal(slot) | Op::JumpIfProvided { slot, .. }
                if *slot >= chunk.locals =>
            {
                Err(format!("{} references missing local {slot}", location()))
            }
            Op::Jump(target) | Op::JumpIfFalse(target) | Op::JumpIfProvided { target, .. }
                if *target >= chunk.code.len() =>
            {
                Err(format!(
                    "{} jumps to missing instruction {target}",
                    location()
                ))
            }
            Op::MakeClosure {
                chunk: target,
                captures,
            } if self.chunk(*target).is_none() => Err(format!(
                "{} references missing function chunk {target}",
                location()
            )),
            Op::MakeClosure { captures, .. }
                if captures.iter().any(
                    |capture| matches!(capture, Capture::Local(slot) if *slot >= chunk.locals),
                ) =>
            {
                Err(format!("{} captures a missing local", location()))
            }
            Op::GetGlobalPooled(id) | Op::DefineGlobalPooled(id) | Op::SetGlobalPooled(id)
                if self.global_name(*id).is_none() =>
            {
                Err(format!(
                    "{} references missing global name metadata",
                    location()
                ))
            }
            Op::MakeClosurePooled {
                chunk: target,
                captures,
            } if self.chunk(*target).is_none() => Err(format!(
                "{} references missing function chunk {target}",
                location()
            )),
            Op::MakeClosurePooled { captures, .. } if self.capture_list(*captures).is_none() => {
                Err(format!(
                    "{} references missing capture metadata",
                    location()
                ))
            }
            Op::MakeClosurePooled { captures, .. }
                if self.capture_list(*captures).is_some_and(|captures| {
                    captures.iter().any(
                        |capture| matches!(capture, Capture::Local(slot) if *slot >= chunk.locals),
                    )
                }) =>
            {
                Err(format!("{} captures a missing local", location()))
            }
            Op::StructSchemaPooled(id) if self.schema_fields(*id).is_none() => Err(format!(
                "{} references missing schema field metadata",
                location()
            )),
            Op::StructPooled(id) | Op::StructCopyPooled(id)
                if self.struct_fields(*id).is_none() =>
            {
                Err(format!(
                    "{} references missing struct field metadata",
                    location()
                ))
            }
            Op::CallSelected { identity, .. } | Op::PipelineCallSelected { identity, .. }
                if self.callable_identity(*identity).is_none() =>
            {
                Err("selected callable identity does not exist".into())
            }
            Op::RecordModuleTag {
                declaration, tag, ..
            } if self
                .declarations
                .get(*declaration)
                .is_none_or(|declaration| declaration.tags.get(*tag).is_none()) =>
            {
                Err(format!(
                    "{} references missing module tag metadata",
                    location()
                ))
            }
            Op::Select(cases) if cases.is_empty() => {
                Err(format!("{} has no select cases", location()))
            }
            Op::TryMatch {
                pattern,
                bindings,
                operands,
            } => {
                if operands.checked_add(1).is_none() || bindings.checked_add(1).is_none() {
                    return Err("match stack count is too large".into());
                }
                Self::validate_pattern(pattern, *operands)?;
                if Self::pattern_bindings(pattern) != Some(*bindings) {
                    return Err("match pattern binding count is invalid".into());
                }
                Ok(())
            }
            Op::TryMatchPooled {
                pattern,
                bindings,
                operands,
            } => {
                if operands.checked_add(1).is_none() || bindings.checked_add(1).is_none() {
                    return Err("match stack count is too large".into());
                }
                let pattern = self.match_pattern(*pattern).ok_or_else(|| {
                    format!("{} references missing match pattern metadata", location())
                })?;
                Self::validate_pattern(pattern, *operands)?;
                if Self::pattern_bindings(pattern) != Some(*bindings) {
                    return Err("match pattern binding count is invalid".into());
                }
                Ok(())
            }
            Op::GetGlobal(_)
            | Op::DefineGlobal(_)
            | Op::SetGlobal(_)
            | Op::MakeClosure { .. }
            | Op::StructSchema(_)
            | Op::Struct(_)
            | Op::StructCopy(_) => Err(format!("{} retains unpooled opcode metadata", location())),
            _ => Ok(()),
        }
    }

    fn validate_pattern(pattern: &MatchPattern, operands: usize) -> Result<(), String> {
        let operand = |index: usize| {
            (index < operands)
                .then_some(())
                .ok_or_else(|| format!("match pattern operand {index} does not exist"))
        };
        match pattern {
            MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Binding => Ok(()),
            MatchPattern::Pinned(index) => operand(*index),
            MatchPattern::At(pattern) => Self::validate_pattern(pattern, operands),
            MatchPattern::Alternatives(patterns) => patterns
                .iter()
                .try_for_each(|pattern| Self::validate_pattern(pattern, operands)),
            MatchPattern::List { items, .. } => items
                .iter()
                .try_for_each(|pattern| Self::validate_pattern(pattern, operands)),
            MatchPattern::Map { entries, .. } => entries.iter().try_for_each(|(key, pattern)| {
                if let MatchMapKey::Operand(index) = key {
                    operand(*index)?;
                }
                Self::validate_pattern(pattern, operands)
            }),
            MatchPattern::Constrained {
                pattern,
                constraint,
            } => {
                Self::validate_pattern(pattern, operands)?;
                Self::validate_match_type(constraint, operands)
            }
        }
    }

    fn validate_match_type(kind: &MatchType, operands: usize) -> Result<(), String> {
        match kind {
            MatchType::List(Some(element)) => Self::validate_match_type(element, operands),
            MatchType::Map(Some((key, value))) => {
                Self::validate_match_type(key, operands)?;
                Self::validate_match_type(value, operands)
            }
            MatchType::Struct(Some(index)) => (*index < operands)
                .then_some(())
                .ok_or_else(|| format!("match pattern operand {index} does not exist")),
            MatchType::Union(members) => members
                .iter()
                .try_for_each(|member| Self::validate_match_type(member, operands)),
            _ => Ok(()),
        }
    }

    fn pattern_bindings(pattern: &MatchPattern) -> Option<usize> {
        match pattern {
            MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Pinned(_) => Some(0),
            MatchPattern::Binding => Some(1),
            MatchPattern::At(pattern) => Self::pattern_bindings(pattern)?.checked_add(1),
            MatchPattern::Alternatives(patterns) => {
                let mut counts = patterns.iter().map(Self::pattern_bindings);
                let count = counts.next()??;
                counts.all(|next| next == Some(count)).then_some(count)
            }
            MatchPattern::List { items, rest } => items.iter().try_fold(
                usize::from(*rest == MatchRest::Binding),
                |count, pattern| count.checked_add(Self::pattern_bindings(pattern)?),
            ),
            MatchPattern::Map { entries, rest, .. } => entries.iter().try_fold(
                usize::from(*rest == MatchRest::Binding),
                |count, (_, pattern)| count.checked_add(Self::pattern_bindings(pattern)?),
            ),
            MatchPattern::Constrained { pattern, .. } => Self::pattern_bindings(pattern),
        }
    }

    fn validate_stack(&self, chunk: &Chunk, initial_stack: usize) -> Result<(), String> {
        let mut stacks = vec![None; chunk.code.len()];
        let mut pending = VecDeque::new();
        if !chunk.code.is_empty() {
            stacks[0] = Some(vec![StackValue::Unknown; initial_stack]);
            pending.push_back(0usize);
        }
        while let Some(index) = pending.pop_front() {
            let stack = stacks[index].as_ref().expect("queued stack state exists");
            let instruction = &chunk.code[index];
            let Some(next_stack) = self.apply_stack_effect(stack, &instruction.op) else {
                let (pops, _) = self.stack_effect(&instruction.op);
                return Err(format!(
                    "function `{}` instruction {index} requires {pops} stack values, has {}",
                    chunk.name,
                    stack.len()
                ));
            };
            let successors: Vec<usize> = match &instruction.op {
                Op::Return | Op::Throw | Op::MatchFailure | Op::NotImplemented => Vec::new(),
                Op::Recur(_) => vec![0],
                Op::Jump(target) => vec![*target],
                Op::JumpIfFalse(target) | Op::JumpIfProvided { target, .. } => [
                    Some(*target),
                    (index + 1 < chunk.code.len()).then_some(index + 1),
                ]
                .into_iter()
                .flatten()
                .collect(),
                _ => (index + 1 < chunk.code.len())
                    .then_some(index + 1)
                    .into_iter()
                    .collect(),
            };
            for successor in successors {
                let successor_stack = if matches!(instruction.op, Op::Recur(_)) {
                    vec![StackValue::Unknown; initial_stack]
                } else {
                    next_stack.clone()
                };
                if let Some(existing) = &mut stacks[successor] {
                    if Self::merge_stack(existing, successor_stack) {
                        pending.push_back(successor);
                    }
                } else {
                    stacks[successor] = Some(successor_stack);
                    pending.push_back(successor);
                }
            }
        }
        Ok(())
    }

    fn apply_stack_effect(&self, stack: &[StackValue], op: &Op) -> Option<Vec<StackValue>> {
        let (pops, pushes) = self.stack_effect(op);
        let remaining = stack.len().checked_sub(pops)?;
        let mut next = stack[..remaining].to_vec();
        match op {
            Op::TryMatch { bindings, .. } | Op::TryMatchPooled { bindings, .. } => {
                next.extend((0..*bindings).map(|_| StackValue::MatchBinding));
                next.push(StackValue::MatchResult);
            }
            _ => next.extend((0..pushes).map(|_| StackValue::Unknown)),
        }
        Some(next)
    }

    fn merge_stack(existing: &mut Vec<StackValue>, incoming: Vec<StackValue>) -> bool {
        if incoming.len() < existing.len() {
            *existing = incoming;
            return true;
        }
        if incoming.len() != existing.len() {
            return false;
        }
        let mut changed = false;
        for (existing, incoming) in existing.iter_mut().zip(incoming) {
            if *existing != incoming && *existing != StackValue::Unknown {
                *existing = StackValue::Unknown;
                changed = true;
            }
        }
        changed
    }

    #[allow(clippy::too_many_lines)]
    fn stack_effect(&self, op: &Op) -> (usize, usize) {
        match op {
            Op::Constant(_)
            | Op::Nil
            | Op::True
            | Op::False
            | Op::GetLocal(_)
            | Op::GetCapture(_)
            | Op::GetGlobal(_)
            | Op::GetGlobalPooled(_)
            | Op::MakeClosure { .. }
            | Op::MakeClosurePooled { .. } => (0, 1),
            Op::Interpolate(parts) => (parts.len().saturating_sub(1), 1),
            Op::Pop
            | Op::SetLocal(_)
            | Op::SetCapture(_)
            | Op::DefineGlobal(_)
            | Op::DefineGlobalPooled(_)
            | Op::SetGlobal(_)
            | Op::SetGlobalPooled(_)
            | Op::DefineMapGlobals
            | Op::Defer { .. }
            | Op::Throw
            | Op::Return => (1, 0),
            Op::Duplicate => (1, 2),
            Op::RecordModuleTag { arguments, .. } => (*arguments, 0),
            Op::List(count) => (*count, 1),
            Op::ListSpread(spreads) => (spreads.len(), 1),
            Op::Map(count) => (count.saturating_mul(2), 1),
            Op::StructSchema(fields) => {
                (fields.iter().filter(|field| field.has_default).count(), 1)
            }
            Op::StructSchemaPooled(id) => (
                self.schema_fields(*id)
                    .expect("validated schema field metadata")
                    .iter()
                    .filter(|field| field.has_default)
                    .count(),
                1,
            ),
            Op::Struct(fields) | Op::StructCopy(fields) => (fields.len() + 1, 1),
            Op::StructPooled(id) | Op::StructCopyPooled(id) => (
                self.struct_fields(*id)
                    .expect("validated struct field metadata")
                    .len()
                    + 1,
                1,
            ),
            Op::CombineOverloads
            | Op::GetIndex
            | Op::Add
            | Op::Subtract
            | Op::Multiply
            | Op::Divide
            | Op::Modulo
            | Op::BitAnd
            | Op::BitOr
            | Op::BitXor
            | Op::ShiftLeft
            | Op::ShiftRight
            | Op::ListAppend
            | Op::ListPrepend
            | Op::Equal
            | Op::Greater
            | Op::Less => (2, 1),
            Op::GetSlice {
                has_start,
                has_end,
                has_step,
            } => (
                1 + usize::from(*has_start) + usize::from(*has_end) + usize::from(*has_step),
                1,
            ),
            Op::Negate | Op::Not | Op::BitNot | Op::Spawn | Op::SelectApply => (1, 1),
            Op::Jump(_)
            | Op::JumpIfFalse(_)
            | Op::JumpIfProvided { .. }
            | Op::EnterScope
            | Op::LeaveScope
            | Op::MatchFailure
            | Op::NotImplemented => (0, 0),
            Op::Call(count) => (count.checked_add(1).unwrap_or(0), 1),
            Op::CallSpread(kinds) | Op::CallSelected { kinds, .. } => (kinds.len() + 1, 1),
            Op::PipelineCall(kinds) | Op::PipelineCallSelected { kinds, .. } => {
                (kinds.len() + 2, 1)
            }
            Op::Import(kinds) => (kinds.len(), 1),
            Op::Nursery { has_limit } => (1 + usize::from(*has_limit), 1),
            Op::Select(cases) => (
                cases
                    .iter()
                    .map(|case| match case {
                        SelectCase::Receive { has_handler }
                        | SelectCase::After { has_handler }
                        | SelectCase::Await { has_handler } => 1 + usize::from(*has_handler),
                        SelectCase::Send { has_handler } => 2 + usize::from(*has_handler),
                        SelectCase::Default { has_handler } => usize::from(*has_handler),
                    })
                    .sum(),
                1,
            ),
            Op::TryMatch {
                bindings, operands, ..
            }
            | Op::TryMatchPooled {
                bindings, operands, ..
            } => (operands + 1, bindings + 1),
            Op::Recur(kinds) => (kinds.len(), 0),
        }
    }

    pub(crate) fn resolve_opcode<'a>(&'a self, op: &'a Op) -> Cow<'a, Op> {
        match op {
            Op::GetGlobalPooled(id) => Cow::Owned(Op::GetGlobal(
                self.global_name(*id)
                    .expect("validated global name metadata")
                    .into(),
            )),
            Op::DefineGlobalPooled(id) => Cow::Owned(Op::DefineGlobal(
                self.global_name(*id)
                    .expect("validated global name metadata")
                    .into(),
            )),
            Op::SetGlobalPooled(id) => Cow::Owned(Op::SetGlobal(
                self.global_name(*id)
                    .expect("validated global name metadata")
                    .into(),
            )),
            Op::MakeClosurePooled { chunk, captures } => Cow::Owned(Op::MakeClosure {
                chunk: *chunk,
                captures: self
                    .capture_list(*captures)
                    .expect("validated capture metadata")
                    .to_vec(),
            }),
            Op::StructSchemaPooled(id) => Cow::Owned(Op::StructSchema(
                self.schema_fields(*id)
                    .expect("validated schema field metadata")
                    .to_vec(),
            )),
            Op::StructPooled(id) => Cow::Owned(Op::Struct(
                self.struct_fields(*id)
                    .expect("validated struct field metadata")
                    .to_vec(),
            )),
            Op::StructCopyPooled(id) => Cow::Owned(Op::StructCopy(
                self.struct_fields(*id)
                    .expect("validated struct field metadata")
                    .to_vec(),
            )),
            Op::TryMatchPooled {
                pattern,
                bindings,
                operands,
            } => Cow::Owned(Op::TryMatch {
                pattern: self
                    .match_pattern(*pattern)
                    .expect("validated match pattern metadata")
                    .clone(),
                bindings: *bindings,
                operands: *operands,
            }),
            _ => Cow::Borrowed(op),
        }
    }
}
