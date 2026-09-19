use std::{
    collections::{HashMap, VecDeque},
    mem,
    sync::Arc,
};

use crate::source::environment::{CallableIdentity, ModuleSnapshot};

use super::{
    chunk::{Chunk, CompiledChunk, PackedInstruction, PackedOpcode},
    metadata::{
        CallArgumentsId, Capture, CaptureListId, Constant, GlobalNameId, InterpolationId,
        ListSpreadId, MatchMapKey, MatchPattern, MatchPatternId, MatchRest, MatchType,
        ModuleDeclaration, ParameterSignature, SchemaField, SchemaFieldsId, SelectCase,
        SelectCasesId, SelectedCallId, SourceId, SourceSpan, SpanId, StructFieldsId,
    },
    op::{CallArgumentKind, DeferMode, Instruction, Op},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StackValue {
    Unknown,
    MatchBinding,
    MatchResult,
}

#[derive(Clone)]
struct VerificationState {
    stack: Vec<StackValue>,
    scope_depth: usize,
}

/// All code available to a VM invocation.
#[derive(Clone, Debug, Default)]
pub struct Program {
    chunks: Vec<CompiledChunk>,
    names: HashMap<String, usize>,
    bindings: Vec<String>,
    declarations: Vec<ModuleDeclaration>,
    exports: Vec<String>,
    entrypoint: Option<Entrypoint>,
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
    interpolations: Vec<Vec<String>>,
    list_spreads: Vec<Vec<bool>>,
    call_arguments: Vec<Vec<CallArgumentKind>>,
    selected_calls: Vec<(CallArgumentsId, usize)>,
    select_cases: Vec<Vec<SelectCase>>,
}

/// Argument value supplied to a validated program entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntrypointArguments {
    None,
    List,
    Map,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Entrypoint {
    pub(crate) arguments: EntrypointArguments,
    pub(crate) callable_identity: usize,
}

/// Layout measurements for private bytecode metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeLayoutMetrics {
    pub program_inline_bytes: usize,
    pub instructions: usize,
    pub instruction_bytes: usize,
    pub instruction_size_bytes: usize,
    pub chunk_storage_bytes: usize,
    pub constant_pool_slots: usize,
    pub constant_pool_capacity_bytes: usize,
    pub descriptor_capacity_bytes: usize,
    pub metadata_pool_slots: usize,
    pub metadata_pool_capacity_bytes: usize,
    pub source_table_capacity_bytes: usize,
    pub largest_chunk_instructions: usize,
    pub largest_constant_pool: usize,
    pub largest_local_frame: usize,
    pub largest_metadata_pool: usize,
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
        let invalid_instructions = chunk
            .code
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                Self::builder_limit_error(&instruction.op).map(|error| (index, error))
            })
            .collect();
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
        let compiled = CompiledChunk {
            name: chunk.name.clone(),
            arity: chunk.arity,
            parameters: chunk.parameters.clone(),
            callable_identity: chunk.callable_identity,
            locals: chunk.locals,
            constants: chunk.constants.clone(),
            code: chunk.code.iter().map(Self::pack_instruction).collect(),
            invalid_instructions,
        };
        let index = self.chunks.len();
        self.names.insert(compiled.name.clone(), index);
        self.chunks.push(compiled);
        index
    }

    fn builder_limit_error(op: &Op) -> Option<String> {
        match op {
            Op::Call(count) if count.checked_add(1).is_none() => {
                Some("call argument count is too large".into())
            }
            Op::TryMatch {
                bindings, operands, ..
            } if operands.checked_add(1).is_none() || bindings.checked_add(1).is_none() => {
                Some("match stack count is too large".into())
            }
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn chunk(&self, index: usize) -> Option<&CompiledChunk> {
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
        let (chunk_storage_bytes, constant_pool_slots, constant_pool_capacity_bytes) =
            self.chunk_layout_metrics();
        let (metadata_pool_slots, metadata_pool_capacity_bytes) = self.metadata_pool_metrics();
        let largest_metadata_pool = self.largest_metadata_pool();
        let span_runs = self.span_runs();
        let source_table_capacity_bytes = self.source_table_capacity_bytes();
        let descriptor_capacity_bytes = Self::descriptor_capacity_bytes();
        let (largest_chunk_instructions, largest_constant_pool, largest_local_frame) =
            self.largest_chunk_metrics();
        BytecodeLayoutMetrics {
            program_inline_bytes: std::mem::size_of::<Self>(),
            instructions,
            instruction_bytes: instructions * std::mem::size_of::<PackedInstruction>(),
            instruction_size_bytes: std::mem::size_of::<PackedInstruction>(),
            chunk_storage_bytes,
            constant_pool_slots,
            constant_pool_capacity_bytes,
            descriptor_capacity_bytes,
            metadata_pool_slots,
            metadata_pool_capacity_bytes,
            source_table_capacity_bytes,
            largest_chunk_instructions,
            largest_constant_pool,
            largest_local_frame,
            largest_metadata_pool,
            span_table_entries: self.spans.len(),
            inline_span_bytes: instructions * std::mem::size_of::<Option<SpanId>>(),
            compressed_span_map_bytes: span_runs * (std::mem::size_of::<u32>() * 2),
        }
    }

    fn chunk_layout_metrics(&self) -> (usize, usize, usize) {
        self.chunks.iter().fold((0, 0, 0), |totals, chunk| {
            (
                totals.0
                    + chunk.code.capacity() * std::mem::size_of::<PackedInstruction>()
                    + chunk.constants.capacity() * std::mem::size_of::<Constant>()
                    + chunk.parameters.capacity() * std::mem::size_of::<ParameterSignature>()
                    + chunk.name.capacity(),
                totals.1 + chunk.constants.len(),
                totals.2 + chunk.constants.capacity() * std::mem::size_of::<Constant>(),
            )
        })
    }

    const fn descriptor_capacity_bytes() -> usize {
        0
    }

    fn metadata_pool_metrics(&self) -> (usize, usize) {
        let slots = self.callable_identities.len()
            + self.global_names.len()
            + self.capture_lists.len()
            + self.schema_fields.len()
            + self.struct_fields.len()
            + self.match_patterns.len()
            + self.interpolations.len()
            + self.list_spreads.len()
            + self.call_arguments.len()
            + self.selected_calls.len()
            + self.select_cases.len();
        let capacity_bytes = self.callable_identities.capacity()
            * std::mem::size_of::<CallableIdentity>()
            + self.global_names.capacity() * std::mem::size_of::<String>()
            + self.capture_lists.capacity() * std::mem::size_of::<Vec<Capture>>()
            + self.schema_fields.capacity() * std::mem::size_of::<Vec<SchemaField>>()
            + self.struct_fields.capacity() * std::mem::size_of::<Vec<String>>()
            + self.match_patterns.capacity() * std::mem::size_of::<MatchPattern>()
            + self.interpolations.capacity() * std::mem::size_of::<Vec<String>>()
            + self.list_spreads.capacity() * std::mem::size_of::<Vec<bool>>()
            + self.call_arguments.capacity() * std::mem::size_of::<Vec<CallArgumentKind>>()
            + self.selected_calls.capacity() * std::mem::size_of::<(CallArgumentsId, usize)>()
            + self.select_cases.capacity() * std::mem::size_of::<Vec<SelectCase>>();
        (slots, capacity_bytes)
    }

    fn source_table_capacity_bytes(&self) -> usize {
        self.sources.capacity() * std::mem::size_of::<Arc<str>>()
            + self.spans.capacity() * std::mem::size_of::<SourceSpan>()
    }

    fn largest_chunk_metrics(&self) -> (usize, usize, usize) {
        self.chunks.iter().fold((0, 0, 0), |largest, chunk| {
            (
                largest.0.max(chunk.code.len()),
                largest.1.max(chunk.constants.len()),
                largest.2.max(chunk.locals),
            )
        })
    }

    fn largest_metadata_pool(&self) -> usize {
        [
            self.callable_identities.len(),
            self.global_names.len(),
            self.capture_lists.len(),
            self.schema_fields.len(),
            self.struct_fields.len(),
            self.match_patterns.len(),
            self.interpolations.len(),
            self.list_spreads.len(),
            self.call_arguments.len(),
            self.selected_calls.len(),
            self.select_cases.len(),
        ]
        .into_iter()
        .max()
        .unwrap_or_default()
    }

    fn span_runs(&self) -> usize {
        self.chunks
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
            .sum()
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

    pub(crate) fn interpolation(&self, id: InterpolationId) -> Option<&[String]> {
        self.interpolations.get(id.index()).map(Vec::as_slice)
    }
    pub(crate) fn list_spread(&self, id: ListSpreadId) -> Option<&[bool]> {
        self.list_spreads.get(id.index()).map(Vec::as_slice)
    }
    pub(crate) fn call_arguments(&self, id: CallArgumentsId) -> Option<&[CallArgumentKind]> {
        self.call_arguments.get(id.index()).map(Vec::as_slice)
    }
    pub(crate) fn selected_call(&self, id: SelectedCallId) -> Option<(CallArgumentsId, usize)> {
        self.selected_calls.get(id.index()).copied()
    }
    pub(crate) fn select_cases(&self, id: SelectCasesId) -> Option<&[SelectCase]> {
        self.select_cases.get(id.index()).map(Vec::as_slice)
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
            Op::Interpolate(parts) => Some(Op::InterpolatePooled(
                self.push_interpolation(mem::take(parts)),
            )),
            Op::ListSpread(spreads) => Some(Op::ListSpreadPooled(
                self.push_list_spread(mem::take(spreads)),
            )),
            Op::CallSpread(kinds) => Some(Op::CallSpreadPooled(
                self.push_call_arguments(mem::take(kinds)),
            )),
            Op::PipelineCall(kinds) => Some(Op::PipelineCallPooled(
                self.push_call_arguments(mem::take(kinds)),
            )),
            Op::Import(kinds) => Some(Op::ImportPooled(self.push_call_arguments(mem::take(kinds)))),
            Op::Recur(kinds) => Some(Op::RecurPooled(self.push_call_arguments(mem::take(kinds)))),
            Op::CallSelected { kinds, identity } => Some(Op::CallSelectedPooled(
                self.push_selected_call(mem::take(kinds), *identity),
            )),
            Op::PipelineCallSelected { kinds, identity } => Some(Op::PipelineCallSelectedPooled(
                self.push_selected_call(mem::take(kinds), *identity),
            )),
            Op::Select(cases) => Some(Op::SelectPooled(self.push_select_cases(mem::take(cases)))),
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

    fn push_interpolation(&mut self, values: Vec<String>) -> InterpolationId {
        let id = InterpolationId(
            u32::try_from(self.interpolations.len()).expect("too many interpolations"),
        );
        self.interpolations.push(values);
        id
    }

    fn push_list_spread(&mut self, values: Vec<bool>) -> ListSpreadId {
        let id =
            ListSpreadId(u32::try_from(self.list_spreads.len()).expect("too many list spreads"));
        self.list_spreads.push(values);
        id
    }

    fn push_call_arguments(&mut self, values: Vec<CallArgumentKind>) -> CallArgumentsId {
        let id = CallArgumentsId(
            u32::try_from(self.call_arguments.len()).expect("too many call descriptors"),
        );
        self.call_arguments.push(values);
        id
    }

    fn push_selected_call(
        &mut self,
        values: Vec<CallArgumentKind>,
        identity: usize,
    ) -> SelectedCallId {
        let kinds = self.push_call_arguments(values);
        let id = SelectedCallId(
            u32::try_from(self.selected_calls.len()).expect("too many selected calls"),
        );
        self.selected_calls.push((kinds, identity));
        id
    }

    fn push_select_cases(&mut self, values: Vec<SelectCase>) -> SelectCasesId {
        let id =
            SelectCasesId(u32::try_from(self.select_cases.len()).expect("too many select cases"));
        self.select_cases.push(values);
        id
    }

    #[allow(clippy::too_many_lines)]
    fn pack_instruction(instruction: &Instruction) -> PackedInstruction {
        // Builder programs are intentionally malformed-testable; validation
        // still observes the builder form during this transition.
        let operand = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        let (opcode, a, b, c) = match &instruction.op {
            Op::Constant(v) => (PackedOpcode::Constant, operand(*v), 0, 0),
            Op::InterpolatePooled(v) => (PackedOpcode::Interpolate, v.0, 0, 0),
            Op::Nil => (PackedOpcode::Nil, 0, 0, 0),
            Op::True => (PackedOpcode::True, 0, 0, 0),
            Op::False => (PackedOpcode::False, 0, 0, 0),
            Op::Pop => (PackedOpcode::Pop, 0, 0, 0),
            Op::Duplicate => (PackedOpcode::Duplicate, 0, 0, 0),
            Op::GetLocal(v) => (PackedOpcode::GetLocal, operand(*v), 0, 0),
            Op::SetLocal(v) => (PackedOpcode::SetLocal, operand(*v), 0, 0),
            Op::GetCapture(v) => (PackedOpcode::GetCapture, operand(*v), 0, 0),
            Op::SetCapture(v) => (PackedOpcode::SetCapture, operand(*v), 0, 0),
            Op::GetGlobalPooled(v) => (PackedOpcode::GetGlobal, v.0, 0, 0),
            Op::NotImplemented => (PackedOpcode::NotImplemented, 0, 0, 0),
            Op::DefineGlobalPooled(v) => (PackedOpcode::DefineGlobal, v.0, 0, 0),
            Op::CombineOverloads => (PackedOpcode::CombineOverloads, 0, 0, 0),
            Op::DefineMapGlobals => (PackedOpcode::DefineMapGlobals, 0, 0, 0),
            Op::RecordModuleTag {
                declaration,
                tag,
                arguments,
            } => (
                PackedOpcode::RecordModuleTag,
                operand(*declaration),
                operand(*tag),
                operand(*arguments),
            ),
            Op::SetGlobalPooled(v) => (PackedOpcode::SetGlobal, v.0, 0, 0),
            Op::MakeClosurePooled { chunk, captures } => {
                (PackedOpcode::MakeClosure, operand(*chunk), captures.0, 0)
            }
            Op::List(v) => (PackedOpcode::List, operand(*v), 0, 0),
            Op::ListSpreadPooled(v) => (PackedOpcode::ListSpread, v.0, 0, 0),
            Op::Map(v) => (PackedOpcode::Map, operand(*v), 0, 0),
            Op::StructSchemaPooled(v) => (PackedOpcode::StructSchema, v.0, 0, 0),
            Op::StructPooled(v) => (PackedOpcode::Struct, v.0, 0, 0),
            Op::StructCopyPooled(v) => (PackedOpcode::StructCopy, v.0, 0, 0),
            Op::GetIndex => (PackedOpcode::GetIndex, 0, 0, 0),
            Op::GetSlice {
                has_start,
                has_end,
                has_step,
            } => (
                PackedOpcode::GetSlice,
                u32::from(*has_start),
                u32::from(*has_end),
                u32::from(*has_step),
            ),
            Op::Add => (PackedOpcode::Add, 0, 0, 0),
            Op::Subtract => (PackedOpcode::Subtract, 0, 0, 0),
            Op::Multiply => (PackedOpcode::Multiply, 0, 0, 0),
            Op::Divide => (PackedOpcode::Divide, 0, 0, 0),
            Op::Modulo => (PackedOpcode::Modulo, 0, 0, 0),
            Op::BitAnd => (PackedOpcode::BitAnd, 0, 0, 0),
            Op::BitOr => (PackedOpcode::BitOr, 0, 0, 0),
            Op::BitXor => (PackedOpcode::BitXor, 0, 0, 0),
            Op::ShiftLeft => (PackedOpcode::ShiftLeft, 0, 0, 0),
            Op::ShiftRight => (PackedOpcode::ShiftRight, 0, 0, 0),
            Op::ListAppend => (PackedOpcode::ListAppend, 0, 0, 0),
            Op::ListPrepend => (PackedOpcode::ListPrepend, 0, 0, 0),
            Op::Negate => (PackedOpcode::Negate, 0, 0, 0),
            Op::Not => (PackedOpcode::Not, 0, 0, 0),
            Op::BitNot => (PackedOpcode::BitNot, 0, 0, 0),
            Op::Equal => (PackedOpcode::Equal, 0, 0, 0),
            Op::Greater => (PackedOpcode::Greater, 0, 0, 0),
            Op::Less => (PackedOpcode::Less, 0, 0, 0),
            Op::GuardGreater => (PackedOpcode::GuardGreater, 0, 0, 0),
            Op::GuardLess => (PackedOpcode::GuardLess, 0, 0, 0),
            Op::Jump(v) => (PackedOpcode::Jump, operand(*v), 0, 0),
            Op::JumpIfFalse(v) => (PackedOpcode::JumpIfFalse, operand(*v), 0, 0),
            Op::JumpIfProvided { slot, target } => (
                PackedOpcode::JumpIfProvided,
                operand(*slot),
                operand(*target),
                0,
            ),
            Op::Call(v) => (PackedOpcode::Call, operand(*v), 0, 0),
            Op::CallPositional(v) => (PackedOpcode::CallPositional, operand(*v), 0, 0),
            Op::CallSpreadPooled(v) => (PackedOpcode::CallSpread, v.0, 0, 0),
            Op::CallSelectedPooled(v) => (PackedOpcode::CallSelected, v.0, 0, 0),
            Op::PipelineCallPooled(v) => (PackedOpcode::PipelineCall, v.0, 0, 0),
            Op::PipelineCallSelectedPooled(v) => (PackedOpcode::PipelineCallSelected, v.0, 0, 0),
            Op::ImportPooled(v) => (PackedOpcode::Import, v.0, 0, 0),
            Op::Spawn => (PackedOpcode::Spawn, 0, 0, 0),
            Op::Nursery { has_limit } => (PackedOpcode::Nursery, u32::from(*has_limit), 0, 0),
            Op::SelectPooled(v) => (PackedOpcode::Select, v.0, 0, 0),
            Op::SelectApply => (PackedOpcode::SelectApply, 0, 0, 0),
            Op::TryMatchPooled {
                pattern,
                bindings,
                operands,
            } => (
                PackedOpcode::TryMatch,
                pattern.0,
                operand(*bindings),
                operand(*operands),
            ),
            Op::MatchFailure => (PackedOpcode::MatchFailure, 0, 0, 0),
            Op::Throw => (PackedOpcode::Throw, 0, 0, 0),
            Op::EnterScope => (PackedOpcode::EnterScope, 0, 0, 0),
            Op::LeaveScope => (PackedOpcode::LeaveScope, 0, 0, 0),
            Op::Defer { mode } => (
                PackedOpcode::Defer,
                match mode {
                    DeferMode::Always => 0,
                    DeferMode::Success => 1,
                    DeferMode::Error => 2,
                },
                0,
                0,
            ),
            Op::RecurPooled(v) => (PackedOpcode::Recur, v.0, 0, 0),
            Op::RecurPositional(v) => (PackedOpcode::RecurPositional, operand(*v), 0, 0),
            Op::Return => (PackedOpcode::Return, 0, 0, 0),
            _ => unreachable!("all installed metadata must be pooled"),
        };
        PackedInstruction {
            opcode,
            a,
            b,
            c,
            span: instruction.span,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn unpack_instruction(
        instruction: &PackedInstruction,
    ) -> Result<Instruction, String> {
        let n = |value: u32| value as usize;
        let missing = |name: &str| Err(format!("packed instruction references missing {name}"));
        let op = match instruction.opcode {
            PackedOpcode::Constant => Op::Constant(n(instruction.a)),
            PackedOpcode::Interpolate => Op::InterpolatePooled(InterpolationId(instruction.a)),
            PackedOpcode::Nil => Op::Nil,
            PackedOpcode::True => Op::True,
            PackedOpcode::False => Op::False,
            PackedOpcode::Pop => Op::Pop,
            PackedOpcode::Duplicate => Op::Duplicate,
            PackedOpcode::GetLocal => Op::GetLocal(n(instruction.a)),
            PackedOpcode::SetLocal => Op::SetLocal(n(instruction.a)),
            PackedOpcode::GetCapture => Op::GetCapture(n(instruction.a)),
            PackedOpcode::SetCapture => Op::SetCapture(n(instruction.a)),
            PackedOpcode::GetGlobal => Op::GetGlobalPooled(GlobalNameId(instruction.a)),
            PackedOpcode::NotImplemented => Op::NotImplemented,
            PackedOpcode::DefineGlobal => Op::DefineGlobalPooled(GlobalNameId(instruction.a)),
            PackedOpcode::CombineOverloads => Op::CombineOverloads,
            PackedOpcode::DefineMapGlobals => Op::DefineMapGlobals,
            PackedOpcode::RecordModuleTag => Op::RecordModuleTag {
                declaration: n(instruction.a),
                tag: n(instruction.b),
                arguments: n(instruction.c),
            },
            PackedOpcode::SetGlobal => Op::SetGlobalPooled(GlobalNameId(instruction.a)),
            PackedOpcode::MakeClosure => Op::MakeClosurePooled {
                chunk: n(instruction.a),
                captures: CaptureListId(instruction.b),
            },
            PackedOpcode::List => Op::List(n(instruction.a)),
            PackedOpcode::ListSpread => Op::ListSpreadPooled(ListSpreadId(instruction.a)),
            PackedOpcode::Map => Op::Map(n(instruction.a)),
            PackedOpcode::StructSchema => Op::StructSchemaPooled(SchemaFieldsId(instruction.a)),
            PackedOpcode::Struct => Op::StructPooled(StructFieldsId(instruction.a)),
            PackedOpcode::StructCopy => Op::StructCopyPooled(StructFieldsId(instruction.a)),
            PackedOpcode::GetIndex => Op::GetIndex,
            PackedOpcode::GetSlice => Op::GetSlice {
                has_start: instruction.a != 0,
                has_end: instruction.b != 0,
                has_step: instruction.c != 0,
            },
            PackedOpcode::Add => Op::Add,
            PackedOpcode::Subtract => Op::Subtract,
            PackedOpcode::Multiply => Op::Multiply,
            PackedOpcode::Divide => Op::Divide,
            PackedOpcode::Modulo => Op::Modulo,
            PackedOpcode::BitAnd => Op::BitAnd,
            PackedOpcode::BitOr => Op::BitOr,
            PackedOpcode::BitXor => Op::BitXor,
            PackedOpcode::ShiftLeft => Op::ShiftLeft,
            PackedOpcode::ShiftRight => Op::ShiftRight,
            PackedOpcode::ListAppend => Op::ListAppend,
            PackedOpcode::ListPrepend => Op::ListPrepend,
            PackedOpcode::Negate => Op::Negate,
            PackedOpcode::Not => Op::Not,
            PackedOpcode::BitNot => Op::BitNot,
            PackedOpcode::Equal => Op::Equal,
            PackedOpcode::Greater => Op::Greater,
            PackedOpcode::Less => Op::Less,
            PackedOpcode::GuardGreater => Op::GuardGreater,
            PackedOpcode::GuardLess => Op::GuardLess,
            PackedOpcode::Jump => Op::Jump(n(instruction.a)),
            PackedOpcode::JumpIfFalse => Op::JumpIfFalse(n(instruction.a)),
            PackedOpcode::JumpIfProvided => Op::JumpIfProvided {
                slot: n(instruction.a),
                target: n(instruction.b),
            },
            PackedOpcode::Call => Op::Call(n(instruction.a)),
            PackedOpcode::CallPositional => Op::CallPositional(n(instruction.a)),
            PackedOpcode::CallSpread => Op::CallSpreadPooled(CallArgumentsId(instruction.a)),
            PackedOpcode::CallSelected => Op::CallSelectedPooled(SelectedCallId(instruction.a)),
            PackedOpcode::PipelineCall => Op::PipelineCallPooled(CallArgumentsId(instruction.a)),
            PackedOpcode::PipelineCallSelected => {
                Op::PipelineCallSelectedPooled(SelectedCallId(instruction.a))
            }
            PackedOpcode::Import => Op::ImportPooled(CallArgumentsId(instruction.a)),
            PackedOpcode::Spawn => Op::Spawn,
            PackedOpcode::Nursery => Op::Nursery {
                has_limit: instruction.a != 0,
            },
            PackedOpcode::Select => Op::SelectPooled(SelectCasesId(instruction.a)),
            PackedOpcode::SelectApply => Op::SelectApply,
            PackedOpcode::TryMatch => Op::TryMatchPooled {
                pattern: MatchPatternId(instruction.a),
                bindings: n(instruction.b),
                operands: n(instruction.c),
            },
            PackedOpcode::MatchFailure => Op::MatchFailure,
            PackedOpcode::Throw => Op::Throw,
            PackedOpcode::EnterScope => Op::EnterScope,
            PackedOpcode::LeaveScope => Op::LeaveScope,
            PackedOpcode::Defer => Op::Defer {
                mode: match instruction.a {
                    0 => DeferMode::Always,
                    1 => DeferMode::Success,
                    2 => DeferMode::Error,
                    _ => return missing("defer mode"),
                },
            },
            PackedOpcode::Recur => Op::RecurPooled(CallArgumentsId(instruction.a)),
            PackedOpcode::RecurPositional => Op::RecurPositional(n(instruction.a)),
            PackedOpcode::Return => Op::Return,
        };
        Ok(Instruction {
            op,
            span: instruction.span,
        })
    }

    fn unpack_chunk(chunk: &CompiledChunk) -> Result<Chunk, String> {
        Ok(Chunk {
            name: chunk.name.clone(),
            arity: chunk.arity,
            parameters: chunk.parameters.clone(),
            callable_identity: chunk.callable_identity,
            locals: chunk.locals,
            constants: chunk.constants.clone(),
            code: chunk
                .code
                .iter()
                .map(Self::unpack_instruction)
                .collect::<Result<_, _>>()?,
            spans: Vec::new(),
            span_ids: HashMap::new(),
        })
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

    /// Whether this program declares a validated local top-level `main`.
    #[must_use]
    pub fn has_entrypoint(&self) -> bool {
        self.entrypoint.is_some()
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

    pub(crate) fn entrypoint(&self) -> Option<Entrypoint> {
        self.entrypoint
    }

    pub(crate) fn set_entrypoint(&mut self, entrypoint: Option<Entrypoint>) {
        self.entrypoint = entrypoint;
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
        if let Some(entrypoint) = self.entrypoint
            && self
                .callable_identity(entrypoint.callable_identity)
                .is_none()
        {
            return Err("program entrypoint references missing callable identity".into());
        }
        for (chunk_index, compiled) in self.chunks.iter().enumerate() {
            if let Some(error) = compiled.invalid_instructions.values().next() {
                return Err(error.clone());
            }
            let chunk = Self::unpack_chunk(compiled)?;
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
                self.validate_op(chunk_index, instruction_index, &chunk, &instruction.op)?;
            }
            let initial_stack = if chunk_index == entry {
                0
            } else {
                chunk
                    .arity
                    .checked_add(1)
                    .ok_or_else(|| format!("function `{}` has too many parameters", chunk.name))?
            };
            self.validate_stack(&chunk, initial_stack)?;
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
            Op::Call(count) if count.checked_add(1).is_none() => {
                Err("call argument count is too large".into())
            }
            Op::Map(count) if count.checked_mul(2).is_none() => {
                Err(format!("{} has too many map operands", location()))
            }
            Op::Select(cases)
                if cases
                    .iter()
                    .try_fold(0usize, |count, case| {
                        count.checked_add(match case {
                            SelectCase::Receive { has_handler }
                            | SelectCase::After { has_handler }
                            | SelectCase::Await { has_handler } => 1 + usize::from(*has_handler),
                            SelectCase::Send { has_handler } => 2 + usize::from(*has_handler),
                            SelectCase::Default { has_handler } => usize::from(*has_handler),
                        })
                    })
                    .is_none() =>
            {
                Err(format!("{} has too many select operands", location()))
            }
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
            Op::InterpolatePooled(id) if self.interpolation(*id).is_none() => Err(format!(
                "{} references missing interpolation metadata",
                location()
            )),
            Op::ListSpreadPooled(id) if self.list_spread(*id).is_none() => Err(format!(
                "{} references missing list spread metadata",
                location()
            )),
            Op::CallSpreadPooled(id)
            | Op::PipelineCallPooled(id)
            | Op::ImportPooled(id)
            | Op::RecurPooled(id)
                if self.call_arguments(*id).is_none() =>
            {
                Err(format!("{} references missing call metadata", location()))
            }
            Op::CallSelectedPooled(id) | Op::PipelineCallSelectedPooled(id) => {
                let (kinds, identity) = self.selected_call(*id).ok_or_else(|| {
                    format!("{} references missing selected call metadata", location())
                })?;
                if self.call_arguments(kinds).is_none() {
                    return Err(format!("{} references missing call metadata", location()));
                }
                if self.callable_identity(identity).is_none() {
                    return Err("selected callable identity does not exist".into());
                }
                Ok(())
            }
            Op::SelectPooled(id) if self.select_cases(*id).is_none() => {
                Err(format!("{} references missing select metadata", location()))
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
            Op::SelectPooled(id)
                if self.select_cases(*id).is_some_and(<[SelectCase]>::is_empty) =>
            {
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
            MatchPattern::Enum(index) | MatchPattern::Pinned(index) => operand(*index),
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
            MatchPattern::Literal(_)
            | MatchPattern::Enum(_)
            | MatchPattern::Wildcard
            | MatchPattern::Pinned(_) => Some(0),
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
        if chunk.code.is_empty() {
            return Err(format!(
                "function `{}` falls through without Return",
                chunk.name
            ));
        }
        let mut stacks = vec![None; chunk.code.len()];
        let mut pending = VecDeque::new();
        stacks[0] = Some(VerificationState {
            stack: vec![StackValue::Unknown; initial_stack],
            scope_depth: 0,
        });
        pending.push_back(0usize);
        while let Some(index) = pending.pop_front() {
            let state = stacks[index].as_ref().expect("queued stack state exists");
            let instruction = &chunk.code[index];
            let Some(next_stack) = self.apply_stack_effect(&state.stack, &instruction.op) else {
                let (pops, _) = self.stack_effect(&instruction.op);
                return Err(format!(
                    "function `{}` instruction {index} requires {pops} stack values, has {}",
                    chunk.name,
                    state.stack.len()
                ));
            };
            let scope_depth = match instruction.op {
                Op::EnterScope => state.scope_depth.checked_add(1).ok_or_else(|| {
                    format!(
                        "function `{}` instruction {index} has too many scopes",
                        chunk.name
                    )
                })?,
                Op::LeaveScope => state.scope_depth.checked_sub(1).ok_or_else(|| {
                    format!(
                        "function `{}` instruction {index} leaves no active scope",
                        chunk.name
                    )
                })?,
                _ => state.scope_depth,
            };
            let successors: Vec<usize> = match &instruction.op {
                Op::Return | Op::Throw | Op::MatchFailure | Op::NotImplemented => Vec::new(),
                Op::Recur(_) | Op::RecurPooled(_) | Op::RecurPositional(_) => vec![0],
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
            if successors.is_empty()
                && !matches!(
                    instruction.op,
                    Op::Return | Op::Throw | Op::MatchFailure | Op::NotImplemented
                )
            {
                return Err(format!(
                    "function `{}` instruction {index} falls through without Return",
                    chunk.name
                ));
            }
            for successor in successors {
                let successor_state = if matches!(
                    instruction.op,
                    Op::Recur(_) | Op::RecurPooled(_) | Op::RecurPositional(_)
                ) {
                    VerificationState {
                        stack: vec![StackValue::Unknown; initial_stack],
                        scope_depth: 0,
                    }
                } else {
                    VerificationState {
                        stack: next_stack.clone(),
                        scope_depth,
                    }
                };
                if let Some(existing) = &mut stacks[successor] {
                    if existing.scope_depth != successor_state.scope_depth {
                        return Err(format!(
                            "function `{}` instruction {successor} has inconsistent scope depth",
                            chunk.name
                        ));
                    }
                    if Self::merge_stack(&mut existing.stack, successor_state.stack) {
                        pending.push_back(successor);
                    }
                } else {
                    stacks[successor] = Some(successor_state);
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
            Op::InterpolatePooled(id) => (
                self.interpolation(*id)
                    .expect("validated interpolation metadata")
                    .len()
                    .saturating_sub(1),
                1,
            ),
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
            Op::ListSpreadPooled(id) => (
                self.list_spread(*id)
                    .expect("validated list spread metadata")
                    .len(),
                1,
            ),
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
            | Op::Less
            | Op::GuardGreater
            | Op::GuardLess => (2, 1),
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
            Op::Call(count) | Op::CallPositional(count) => (count.checked_add(1).unwrap_or(0), 1),
            Op::CallSpread(kinds) | Op::CallSelected { kinds, .. } => (kinds.len() + 1, 1),
            Op::CallSpreadPooled(id) => (
                self.call_arguments(*id)
                    .expect("validated call metadata")
                    .len()
                    + 1,
                1,
            ),
            Op::CallSelectedPooled(id) => (
                self.call_arguments(
                    self.selected_call(*id)
                        .expect("validated selected call metadata")
                        .0,
                )
                .expect("validated call metadata")
                .len()
                    + 1,
                1,
            ),
            Op::PipelineCall(kinds) | Op::PipelineCallSelected { kinds, .. } => {
                (kinds.len() + 2, 1)
            }
            Op::PipelineCallPooled(id) => (
                self.call_arguments(*id)
                    .expect("validated call metadata")
                    .len()
                    + 2,
                1,
            ),
            Op::PipelineCallSelectedPooled(id) => (
                self.call_arguments(
                    self.selected_call(*id)
                        .expect("validated selected call metadata")
                        .0,
                )
                .expect("validated call metadata")
                .len()
                    + 2,
                1,
            ),
            Op::Import(kinds) => (kinds.len(), 1),
            Op::ImportPooled(id) => (
                self.call_arguments(*id)
                    .expect("validated call metadata")
                    .len(),
                1,
            ),
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
            Op::SelectPooled(id) => (
                self.select_cases(*id)
                    .expect("validated select metadata")
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
            Op::RecurPooled(id) => (
                self.call_arguments(*id)
                    .expect("validated call metadata")
                    .len(),
                0,
            ),
            Op::RecurPositional(count) => (*count, 0),
        }
    }
}
