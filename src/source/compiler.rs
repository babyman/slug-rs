use std::collections::{HashMap, HashSet};

use crate::bytecode::{Entrypoint, EntrypointArguments};
use crate::{
    CallArgumentKind, Capture, Chunk, MatchMapKey, MatchPattern, MatchRest, MatchType,
    ModuleDeclaration, ModuleTag, Op, ParameterSignature, Program, SchemaField, SelectCase,
    SourceSpan,
};

use super::{
    SourceError,
    ast::{
        Binary, CallArgument, CasePattern, Expr, ExprKind, ForeignSignature, ListElement,
        MapPatternKey, MatchCase, Parameter, Pattern, Prefix, RestPattern, SelectCaseKind,
        StringPart, Tag, TypeAnnotation,
    },
    environment::{CallableIdentity, SemanticAnalysis},
    state::{Binding, State},
};

pub(super) struct Compiler {
    path: String,
    expressions: Vec<Expr>,
    chunks: Vec<Chunk>,
    globals: HashMap<String, bool>,
    callable_globals: HashSet<String>,
    declarations: Vec<ModuleDeclaration>,
    selected_calls: HashMap<SourceSpan, CallableIdentity>,
    function_identities: HashMap<SourceSpan, CallableIdentity>,
    foreign_identities: HashMap<SourceSpan, CallableIdentity>,
    callable_identities: Vec<CallableIdentity>,
    guard_comparisons: bool,
}
impl Compiler {
    pub(super) fn new(path: &str, expressions: Vec<Expr>, analysis: &SemanticAnalysis) -> Self {
        Self {
            path: path.into(),
            expressions,
            chunks: Vec::new(),
            globals: HashMap::new(),
            callable_globals: HashSet::new(),
            declarations: Vec::new(),
            selected_calls: analysis.selected_calls.clone(),
            function_identities: analysis.function_identities.clone(),
            foreign_identities: analysis.foreign_identities.clone(),
            callable_identities: Vec::new(),
            guard_comparisons: false,
        }
    }
    #[allow(clippy::too_many_lines)]
    pub(super) fn compile(mut self) -> Result<Program, SourceError> {
        let mut exports = Vec::new();
        let mut bindings = Vec::new();
        let entrypoint = self.entrypoint()?;
        for expression in &self.expressions {
            if let ExprKind::Declare {
                mutable,
                exported,
                pattern,
                ..
            } = &expression.kind
            {
                let names = pattern_bindings(pattern, &expression.span)?;
                self.declarations.push(ModuleDeclaration {
                    bindings: names.clone(),
                    mutable: *mutable,
                    exported: *exported,
                    foreign: false,
                    foreign_arity: None,
                    foreign_callable_identity: None,
                    documentation: match &expression.kind {
                        ExprKind::Declare { documentation, .. } => documentation.clone(),
                        _ => unreachable!("declaration pattern was matched"),
                    },
                    tags: match &expression.kind {
                        ExprKind::Declare { tags, .. } => tags
                            .iter()
                            .map(|tag| ModuleTag {
                                name: tag.name.clone(),
                                arguments: Vec::new(),
                            })
                            .collect(),
                        _ => unreachable!("declaration pattern was matched"),
                    },
                });
                if *exported {
                    exports.extend(names.iter().cloned());
                }
                for name in names {
                    bindings.push(name.clone());
                    self.globals.insert(name, *mutable);
                }
            }
            if let ExprKind::Foreign {
                exported,
                name,
                documentation,
                tags,
                signature,
            } = &expression.kind
            {
                let foreign_callable_identity =
                    self.foreign_identities.get(&expression.span).cloned();
                self.declarations.push(ModuleDeclaration {
                    bindings: vec![name.clone()],
                    mutable: false,
                    exported: *exported,
                    foreign: true,
                    foreign_arity: Some(foreign_arity(signature)),
                    foreign_callable_identity,
                    documentation: documentation.clone(),
                    tags: tags
                        .iter()
                        .map(|tag| ModuleTag {
                            name: tag.name.clone(),
                            arguments: Vec::new(),
                        })
                        .collect(),
                });
                if *exported {
                    exports.push(name.clone());
                }
                bindings.push(name.clone());
                self.globals.insert(name.clone(), false);
                self.callable_globals.insert(name.clone());
            }
        }
        bindings.sort();
        bindings.dedup();
        let expressions = self.expressions.clone();
        let mut state = State::root();
        for (index, expression) in expressions.iter().enumerate() {
            self.expression(&mut state, expression)?;
            if index + 1 < expressions.len() {
                state.emit(Op::Pop, &expression.span);
            }
        }
        if expressions.is_empty() {
            state.emit(Op::Nil, &SourceSpan::new(self.path.clone(), 1, 1));
        }
        state.emit(Op::Return, &SourceSpan::new(self.path.clone(), 1, 1));
        self.chunks.push(state.finish("main", 0));
        let mut program = Program::new();
        for chunk in self.chunks {
            program.add_chunk(chunk);
        }
        program.set_bindings(bindings);
        program.set_declarations(self.declarations);
        let mut seen_exports = HashSet::new();
        exports.retain(|name| seen_exports.insert(name.clone()));
        program.set_exports(exports);
        program.set_entrypoint(entrypoint);
        program.set_callable_identities(self.callable_identities);
        Ok(program)
    }

    fn entrypoint(&mut self) -> Result<Option<Entrypoint>, SourceError> {
        let candidates = self
            .expressions
            .iter()
            .filter_map(|expression| {
                entrypoint_candidate(expression).map(|(arguments, span)| (arguments, span.clone()))
            })
            .collect::<Vec<_>>();
        let mut entrypoint = None;
        for (arguments, span) in candidates {
            if entrypoint.is_some() {
                return Err(SourceError::semantic(
                    "program has more than one eligible `main` entrypoint",
                    span,
                ));
            }
            let identity = self
                .function_identities
                .get(&span)
                .cloned()
                .ok_or_else(|| {
                    SourceError::semantic("cannot identify program entrypoint", span.clone())
                })?;
            entrypoint = Some(Entrypoint {
                arguments,
                callable_identity: self.intern_callable_identity(identity),
            });
        }
        Ok(entrypoint)
    }

    #[allow(clippy::too_many_lines)]
    fn expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        match &expression.kind {
            ExprKind::Value(value) => {
                let constant = state.constant(value.clone());
                state.emit(Op::Constant(constant), &expression.span);
            }
            ExprKind::Interpolate(parts) => {
                let mut text = vec![String::new()];
                for part in parts {
                    match part {
                        StringPart::Text(value) => text
                            .last_mut()
                            .expect("interpolation has text")
                            .push_str(value),
                        StringPart::Name(name) => {
                            self.expression(
                                state,
                                &Expr {
                                    kind: ExprKind::Name(name.clone()),
                                    span: expression.span.clone(),
                                },
                            )?;
                            text.push(String::new());
                        }
                    }
                }
                state.emit(Op::Interpolate(text), &expression.span);
            }
            ExprKind::Documentation(content) => {
                debug_assert!(
                    content
                        .lines()
                        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('*'))
                );
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::NotImplemented => state.emit(Op::NotImplemented, &expression.span),
            ExprKind::Name(name) => match state.lookup(name).or_else(|| {
                self.globals
                    .get(name)
                    .map(|mutable| Binding::Global { mutable: *mutable })
            }) {
                Some(Binding::Global { .. }) | None => {
                    state.emit(Op::GetGlobal(name.clone()), &expression.span);
                }
                Some(Binding::Local { slot, .. }) => {
                    state.emit(Op::GetLocal(slot), &expression.span);
                }
                Some(Binding::Capture { slot, .. }) => {
                    state.emit(Op::GetCapture(slot), &expression.span);
                }
                Some(Binding::Outer { .. }) => unreachable!("outer bindings are captured lazily"),
            },
            ExprKind::Declare {
                mutable,
                exported,
                pattern,
                documentation,
                tags,
                value,
                ..
            } => {
                if *exported {
                    debug_assert!(state.is_root());
                }
                if let Some(documentation) = documentation {
                    debug_assert!(
                        documentation.lines().all(
                            |line| line.trim().is_empty() || line.trim_start().starts_with('*')
                        )
                    );
                }
                let declaration = if state.is_module_scope() {
                    Some(
                        self.expressions
                            .iter()
                            .filter(|candidate| matches!(candidate.kind, ExprKind::Declare { .. }))
                            .position(|candidate| candidate.span == expression.span)
                            .expect("compiled declaration was recorded"),
                    )
                } else {
                    None
                };
                let callable_name = matches!(&value.kind, ExprKind::Function { .. })
                    .then(|| match pattern {
                        Pattern::Binding(name) => Some(name),
                        _ => None,
                    })
                    .flatten();
                let existing_callable = callable_name.and_then(|name| {
                    if state.is_root() {
                        self.callable_globals
                            .contains(name)
                            .then_some(Binding::Global { mutable: false })
                    } else {
                        state.current_callable(name)
                    }
                });
                self.tags(state, tags, declaration, &expression.span)?;
                self.expression(state, value)?;
                if let Some(binding) = existing_callable {
                    match binding {
                        Binding::Global { .. } => state.emit(
                            Op::GetGlobal(callable_name.expect("callable name").clone()),
                            &expression.span,
                        ),
                        Binding::Local { slot, .. } => {
                            state.emit(Op::GetLocal(slot), &expression.span);
                        }
                        Binding::Capture { .. } => {
                            unreachable!("current scope cannot contain a capture")
                        }
                        Binding::Outer { .. } => {
                            unreachable!("current scope cannot contain an outer binding")
                        }
                    }
                    state.emit(Op::CombineOverloads, &expression.span);
                    match binding {
                        Binding::Global { .. } => state.emit(
                            Op::SetGlobal(callable_name.expect("callable name").clone()),
                            &expression.span,
                        ),
                        Binding::Local { slot, .. } => {
                            state.emit(Op::SetLocal(slot), &expression.span);
                        }
                        Binding::Capture { .. } => {
                            unreachable!("current scope cannot contain a capture")
                        }
                        Binding::Outer { .. } => {
                            unreachable!("current scope cannot contain an outer binding")
                        }
                    }
                } else {
                    self.bind_pattern(
                        state,
                        pattern,
                        *mutable,
                        callable_name.is_some(),
                        &expression.span,
                    )?;
                    if state.is_root()
                        && let Some(name) = callable_name
                    {
                        self.callable_globals.insert(name.clone());
                    } else if state.is_root()
                        && let Pattern::Binding(name) = pattern
                    {
                        self.callable_globals.remove(name);
                    }
                }
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Foreign { tags, .. } => {
                let declaration = self
                    .expressions
                    .iter()
                    .filter(|candidate| {
                        matches!(
                            candidate.kind,
                            ExprKind::Declare { .. } | ExprKind::Foreign { .. }
                        )
                    })
                    .position(|candidate| candidate.span == expression.span)
                    .expect("compiled declaration was recorded");
                self.tags(state, tags, Some(declaration), &expression.span)?;
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Assign { name, value } => {
                let binding = state
                    .lookup(name)
                    .or_else(|| {
                        self.globals
                            .get(name)
                            .map(|mutable| Binding::Global { mutable: *mutable })
                    })
                    .ok_or_else(|| {
                        SourceError::semantic(
                            format!("unknown name `{name}`"),
                            expression.span.clone(),
                        )
                    })?;
                let mutable = match binding {
                    Binding::Global { mutable }
                    | Binding::Local { mutable, .. }
                    | Binding::Capture { mutable, .. }
                    | Binding::Outer { mutable, .. } => mutable,
                };
                if !mutable {
                    return Err(SourceError::semantic(
                        format!("cannot assign to immutable binding `{name}`"),
                        expression.span.clone(),
                    ));
                }
                self.expression(state, value)?;
                // Stores consume their operand, while assignment expressions evaluate to it.
                state.emit(Op::Duplicate, &expression.span);
                match binding {
                    Binding::Global { .. } => {
                        state.emit(Op::SetGlobal(name.clone()), &expression.span);
                    }
                    Binding::Local { slot, .. } => {
                        state.emit(Op::SetLocal(slot), &expression.span);
                    }
                    Binding::Capture { slot, .. } => {
                        state.emit(Op::SetCapture(slot), &expression.span);
                    }
                    Binding::Outer { .. } => unreachable!("outer bindings are captured lazily"),
                }
            }
            ExprKind::Return { value } => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "return is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                self.tail_expression(state, value)?;
                state.emit(Op::Return, &expression.span);
            }
            ExprKind::Throw { value } => {
                self.expression(state, value)?;
                state.emit(Op::Throw, &expression.span);
            }
            ExprKind::Defer {
                value,
                mode,
                error_name,
            } => {
                let deferred = Expr {
                    kind: ExprKind::Function {
                        type_parameters: Vec::new(),
                        parameters: error_name
                            .iter()
                            .cloned()
                            .map(|name| Parameter {
                                name,
                                discard: false,
                                tags: Vec::new(),
                                annotation: None,
                                default: None,
                                variadic: false,
                            })
                            .collect(),
                        return_annotation: None,
                        body: value.clone(),
                    },
                    span: expression.span.clone(),
                };
                self.expression(state, &deferred)?;
                state.emit(Op::Defer { mode: *mode }, &expression.span);
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Recur(_) => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "recur is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                return Err(SourceError::semantic(
                    "recur is only valid in tail position",
                    expression.span.clone(),
                ));
            }
            ExprKind::Spawn(body) => {
                let function = Expr {
                    kind: ExprKind::Function {
                        type_parameters: Vec::new(),
                        parameters: Vec::new(),
                        return_annotation: None,
                        body: body.clone(),
                    },
                    span: expression.span.clone(),
                };
                self.expression(state, &function)?;
                state.emit(Op::Spawn, &expression.span);
            }
            ExprKind::Select(cases) => {
                for case in cases {
                    match &case.kind {
                        SelectCaseKind::Receive(value) | SelectCaseKind::Await(value) => {
                            self.expression(state, value)?;
                        }
                        SelectCaseKind::Send { channel, value } => {
                            self.expression(state, channel)?;
                            self.expression(state, value)?;
                        }
                        SelectCaseKind::After(value) => self.expression(state, value)?,
                        SelectCaseKind::Default => {}
                    }
                    if let Some(handler) = &case.handler {
                        self.expression(state, handler)?;
                    }
                }
                state.emit(
                    Op::Select(
                        cases
                            .iter()
                            .map(|case| match &case.kind {
                                SelectCaseKind::Receive(_) => SelectCase::Receive {
                                    has_handler: case.handler.is_some(),
                                },
                                SelectCaseKind::Send { .. } => SelectCase::Send {
                                    has_handler: case.handler.is_some(),
                                },
                                SelectCaseKind::After(_) => SelectCase::After {
                                    has_handler: case.handler.is_some(),
                                },
                                SelectCaseKind::Await(_) => SelectCase::Await {
                                    has_handler: case.handler.is_some(),
                                },
                                SelectCaseKind::Default => SelectCase::Default {
                                    has_handler: case.handler.is_some(),
                                },
                            })
                            .collect(),
                    ),
                    &expression.span,
                );
                state.emit(Op::SelectApply, &expression.span);
            }
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    self.expression(state, limit)?;
                }
                let function = if matches!(body.kind, ExprKind::Function { .. }) {
                    body.as_ref().clone()
                } else {
                    Expr {
                        kind: ExprKind::Function {
                            type_parameters: Vec::new(),
                            parameters: Vec::new(),
                            return_annotation: None,
                            body: body.clone(),
                        },
                        span: expression.span.clone(),
                    }
                };
                self.expression(state, &function)?;
                state.emit(
                    Op::Nursery {
                        has_limit: limit.is_some(),
                    },
                    &expression.span,
                );
            }
            ExprKind::Match { subject, cases } => {
                let subject = subject.as_deref().ok_or_else(|| {
                    SourceError::semantic(
                        "match requires a subject or pipeline input",
                        expression.span.clone(),
                    )
                })?;
                self.compile_match(state, subject, cases, false)?;
            }
            ExprKind::Binary {
                left,
                operator,
                right,
            } => {
                if !matches!(operator, Binary::Pipeline) {
                    self.expression(state, left)?;
                }
                match operator {
                    Binary::Pipeline => {
                        if let ExprKind::Match {
                            subject: None,
                            cases,
                        } = &right.kind
                        {
                            self.compile_match(state, left, cases, false)?;
                        } else if matches!(&right.kind, ExprKind::Match { .. }) {
                            return Err(SourceError::semantic(
                                "pipeline match must omit its subject",
                                right.span.clone(),
                            ));
                        } else {
                            self.expression(state, left)?;
                            self.compile_pipeline_call(state, right)?;
                        }
                    }
                    Binary::And => {
                        let end = state.jump_if_false(&expression.span);
                        state.emit(Op::Pop, &expression.span);
                        self.expression(state, right)?;
                        state.patch(end);
                    }
                    Binary::Or => {
                        let right_hand_side = state.jump_if_false(&expression.span);
                        let end = state.jump(&expression.span);
                        state.patch(right_hand_side);
                        state.emit(Op::Pop, &expression.span);
                        self.expression(state, right)?;
                        state.patch(end);
                    }
                    Binary::NotEqual => {
                        self.expression(state, right)?;
                        state.emit(Op::Equal, &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::GreaterEqual => {
                        self.expression(state, right)?;
                        state.emit(self.less_op(), &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::LessEqual => {
                        self.expression(state, right)?;
                        state.emit(self.greater_op(), &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::Add => {
                        self.expression(state, right)?;
                        state.emit(Op::Add, &expression.span);
                    }
                    Binary::Subtract => {
                        self.expression(state, right)?;
                        state.emit(Op::Subtract, &expression.span);
                    }
                    Binary::Multiply => {
                        self.expression(state, right)?;
                        state.emit(Op::Multiply, &expression.span);
                    }
                    Binary::Divide => {
                        self.expression(state, right)?;
                        state.emit(Op::Divide, &expression.span);
                    }
                    Binary::Modulo => {
                        self.expression(state, right)?;
                        state.emit(Op::Modulo, &expression.span);
                    }
                    Binary::BitAnd => {
                        self.expression(state, right)?;
                        state.emit(Op::BitAnd, &expression.span);
                    }
                    Binary::BitOr => {
                        self.expression(state, right)?;
                        state.emit(Op::BitOr, &expression.span);
                    }
                    Binary::BitXor => {
                        self.expression(state, right)?;
                        state.emit(Op::BitXor, &expression.span);
                    }
                    Binary::ShiftLeft => {
                        self.expression(state, right)?;
                        state.emit(Op::ShiftLeft, &expression.span);
                    }
                    Binary::ShiftRight => {
                        self.expression(state, right)?;
                        state.emit(Op::ShiftRight, &expression.span);
                    }
                    Binary::Append => {
                        self.expression(state, right)?;
                        state.emit(Op::ListAppend, &expression.span);
                    }
                    Binary::Prepend => {
                        self.expression(state, right)?;
                        state.emit(Op::ListPrepend, &expression.span);
                    }
                    Binary::Equal => {
                        self.expression(state, right)?;
                        state.emit(Op::Equal, &expression.span);
                    }
                    Binary::Greater => {
                        self.expression(state, right)?;
                        state.emit(self.greater_op(), &expression.span);
                    }
                    Binary::Less => {
                        self.expression(state, right)?;
                        state.emit(self.less_op(), &expression.span);
                    }
                }
            }
            ExprKind::Prefix { operators, value } => {
                self.expression(state, value)?;
                for (operator, span) in operators.iter().rev() {
                    state.emit(
                        match operator {
                            Prefix::Negate => Op::Negate,
                            Prefix::Not => Op::Not,
                            Prefix::BitNot => Op::BitNot,
                        },
                        span,
                    );
                }
            }
            ExprKind::Call { callee, arguments } => {
                let import = matches!(&callee.kind, ExprKind::Name(name) if name == "import");
                if !import {
                    self.expression(state, callee)?;
                }
                let mut kinds = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    let argument = match argument {
                        CallArgument::Positional(argument) => {
                            kinds.push(CallArgumentKind::Positional);
                            argument
                        }
                        CallArgument::Named { name, value } => {
                            kinds.push(CallArgumentKind::Named(name.clone()));
                            value
                        }
                        CallArgument::Spread(argument) => {
                            kinds.push(CallArgumentKind::Spread);
                            argument
                        }
                    };
                    self.expression(state, argument)?;
                }
                let op = if import {
                    Op::Import(kinds)
                } else if let Some(identity) = self.selected_identity(&expression.span) {
                    Op::CallSelected { kinds, identity }
                } else {
                    Op::CallSpread(kinds)
                };
                state.emit(op, &expression.span);
            }
            ExprKind::TypeApply { callee, .. } => self.expression(state, callee)?,
            ExprKind::List(values) => {
                let mut spreads = Vec::with_capacity(values.len());
                for value in values {
                    let value = match value {
                        ListElement::Value(value) => {
                            spreads.push(false);
                            value
                        }
                        ListElement::Spread(value) => {
                            spreads.push(true);
                            value
                        }
                    };
                    self.expression(state, value)?;
                }
                if spreads.iter().any(|spread| *spread) {
                    state.emit(Op::ListSpread(spreads), &expression.span);
                } else {
                    state.emit(Op::List(values.len()), &expression.span);
                }
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.expression(state, key)?;
                    self.expression(state, value)?;
                }
                state.emit(Op::Map(entries.len()), &expression.span);
            }
            ExprKind::StructSchema(fields) => {
                let mut seen = HashSet::new();
                for field in fields {
                    if !seen.insert(field.name.as_str()) {
                        return Err(SourceError::semantic(
                            format!("duplicate struct field '{}'", field.name),
                            expression.span.clone(),
                        ));
                    }
                    if let Some(default) = &field.default {
                        self.expression(state, default)?;
                    }
                }
                state.emit(
                    Op::StructSchema(
                        fields
                            .iter()
                            .map(|field| SchemaField {
                                name: field.name.clone(),
                                has_default: field.default.is_some(),
                            })
                            .collect(),
                    ),
                    &expression.span,
                );
            }
            ExprKind::StructInit { schema, fields } => {
                self.expression(state, schema)?;
                for (_, value) in fields {
                    self.expression(state, value)?;
                }
                state.emit(
                    Op::Struct(fields.iter().map(|(name, _)| name.clone()).collect()),
                    &expression.span,
                );
            }
            ExprKind::StructCopy { value, fields } => {
                self.expression(state, value)?;
                for (_, replacement) in fields {
                    self.expression(state, replacement)?;
                }
                state.emit(
                    Op::StructCopy(fields.iter().map(|(name, _)| name.clone()).collect()),
                    &expression.span,
                );
            }
            ExprKind::Index { collection, index } => {
                self.expression(state, collection)?;
                self.expression(state, index)?;
                state.emit(Op::GetIndex, &expression.span);
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                self.expression(state, collection)?;
                if let Some(start) = start {
                    self.expression(state, start)?;
                }
                if let Some(end) = end {
                    self.expression(state, end)?;
                }
                if let Some(step) = step {
                    self.expression(state, step)?;
                }
                state.emit(
                    Op::GetSlice {
                        has_start: start.is_some(),
                        has_end: end.is_some(),
                        has_step: step.is_some(),
                    },
                    &expression.span,
                );
            }
            ExprKind::Block(values) => {
                state.enter_scope();
                state.emit(Op::EnterScope, &expression.span);
                for (index, value) in values.iter().enumerate() {
                    self.expression(state, value)?;
                    if index + 1 < values.len() {
                        state.emit(Op::Pop, &value.span);
                    }
                }
                if values.is_empty() {
                    state.emit(Op::Nil, &expression.span);
                }
                state.emit(Op::LeaveScope, &expression.span);
                state.leave_scope();
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(state, condition)?;
                let otherwise = state.jump_if_false(&expression.span);
                state.emit(Op::Pop, &expression.span);
                self.expression(state, then_branch)?;
                let end = state.jump(&expression.span);
                state.patch(otherwise);
                state.emit(Op::Pop, &expression.span);
                if let Some(otherwise) = else_branch {
                    self.expression(state, otherwise)?;
                } else {
                    state.emit(Op::Nil, &expression.span);
                }
                state.patch(end);
            }
            ExprKind::Function {
                parameters, body, ..
            } => {
                for parameter in parameters {
                    self.tags(state, &parameter.tags, None, &expression.span)?;
                }
                let names = plain_parameters(parameters, &expression.span)?;
                let (mut chunk, captures) = self.function(&names, parameters, body, state)?;
                if let Some(identity) = self.function_identity(&expression.span) {
                    chunk.callable_identity = Some(identity);
                }
                chunk.parameters = parameters
                    .iter()
                    .map(|parameter| ParameterSignature {
                        name: parameter.name.clone(),
                        has_default: parameter.default.is_some(),
                        variadic: parameter.variadic,
                    })
                    .collect();
                let index = self.chunks.len();
                chunk.name = format!("<fn #{index}>");
                self.chunks.push(chunk);
                state.emit(
                    Op::MakeClosure {
                        chunk: index,
                        captures,
                    },
                    &expression.span,
                );
            }
        }
        Ok(())
    }
    fn tags(
        &mut self,
        state: &mut State,
        tags: &[Tag],
        declaration: Option<usize>,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        for (tag_index, tag) in tags.iter().enumerate() {
            debug_assert!(
                !tag.name.is_empty(),
                "the parser only constructs named tags"
            );
            for argument in &tag.arguments {
                self.expression(state, argument)?;
            }
            if let Some(declaration) = declaration {
                state.emit(
                    Op::RecordModuleTag {
                        declaration,
                        tag: tag_index,
                        arguments: tag.arguments.len(),
                    },
                    span,
                );
            } else {
                for argument in &tag.arguments {
                    state.emit(Op::Pop, &argument.span);
                }
            }
        }
        Ok(())
    }
    fn compile_match(
        &mut self,
        state: &mut State,
        subject: &Expr,
        cases: &[MatchCase],
        tail: bool,
    ) -> Result<(), SourceError> {
        self.expression(state, subject)?;
        let mut ends = Vec::new();
        for case in cases {
            let (pattern, names, operands) = lower_case_patterns(&case.patterns, &case.span)?;
            state.emit(Op::Duplicate, &case.span);
            for operand in &operands {
                self.emit_pattern_operand(state, operand, &case.span)?;
            }
            state.emit(
                Op::TryMatch {
                    pattern,
                    bindings: names.len(),
                    operands: operands.len(),
                },
                &case.span,
            );
            state.enter_scope();
            let slots = names
                .into_iter()
                .map(|name| state.declare(name, false, false))
                .collect::<Vec<_>>();
            let next = state.jump_if_false(&case.span);
            state.emit(Op::Pop, &case.span);
            for slot in slots.iter().rev() {
                state.emit(Op::SetLocal(*slot), &case.span);
            }
            let guard_next = if let Some(guard) = &case.guard {
                self.guard_expression(state, guard)?;
                Some(state.jump_if_false(&case.span))
            } else {
                None
            };
            if guard_next.is_some() {
                state.emit(Op::Pop, &case.span);
            }
            state.emit(Op::Pop, &case.span);
            if tail {
                self.tail_expression(state, &case.value)?;
            } else {
                self.expression(state, &case.value)?;
            }
            state.leave_scope();
            ends.push(state.jump(&case.span));
            state.patch(next);
            state.emit(Op::Pop, &case.span);
            for _ in slots {
                state.emit(Op::Pop, &case.span);
            }
            if let Some(guard_next) = guard_next {
                let skip_guard_cleanup = state.jump(&case.span);
                state.patch(guard_next);
                state.emit(Op::Pop, &case.span);
                state.patch(skip_guard_cleanup);
            }
        }
        state.emit(Op::Pop, &subject.span);
        state.emit(Op::Nil, &subject.span);
        for end in ends {
            state.patch(end);
        }
        Ok(())
    }

    fn guard_expression(
        &mut self,
        state: &mut State,
        expression: &Expr,
    ) -> Result<(), SourceError> {
        let previous = std::mem::replace(&mut self.guard_comparisons, true);
        let result = self.expression(state, expression);
        self.guard_comparisons = previous;
        result
    }

    fn greater_op(&self) -> Op {
        if self.guard_comparisons {
            Op::GuardGreater
        } else {
            Op::Greater
        }
    }

    fn less_op(&self) -> Op {
        if self.guard_comparisons {
            Op::GuardLess
        } else {
            Op::Less
        }
    }

    fn compile_pipeline_call(
        &mut self,
        state: &mut State,
        expression: &Expr,
    ) -> Result<(), SourceError> {
        if let ExprKind::Call { callee, arguments } = &expression.kind {
            self.expression(state, callee)?;
            let mut kinds = Vec::with_capacity(arguments.len());
            for argument in arguments {
                let argument = match argument {
                    CallArgument::Positional(argument) => {
                        kinds.push(CallArgumentKind::Positional);
                        argument
                    }
                    CallArgument::Named { name, value } => {
                        kinds.push(CallArgumentKind::Named(name.clone()));
                        value
                    }
                    CallArgument::Spread(argument) => {
                        kinds.push(CallArgumentKind::Spread);
                        argument
                    }
                };
                self.expression(state, argument)?;
            }
            let op = if let Some(identity) = self.selected_identity(&expression.span) {
                Op::PipelineCallSelected { kinds, identity }
            } else {
                Op::PipelineCall(kinds)
            };
            state.emit(op, &expression.span);
        } else {
            self.expression(state, expression)?;
            let op = self.selected_identity(&expression.span).map_or_else(
                || Op::PipelineCall(Vec::new()),
                |identity| Op::PipelineCallSelected {
                    kinds: Vec::new(),
                    identity,
                },
            );
            state.emit(op, &expression.span);
        }
        Ok(())
    }

    fn selected_identity(&mut self, span: &SourceSpan) -> Option<usize> {
        let identity = self.selected_calls.get(span)?.clone();
        Some(self.intern_callable_identity(identity))
    }

    fn function_identity(&mut self, span: &SourceSpan) -> Option<usize> {
        let identity = self.function_identities.get(span)?.clone();
        Some(self.intern_callable_identity(identity))
    }

    fn intern_callable_identity(&mut self, identity: CallableIdentity) -> usize {
        if let Some(index) = self
            .callable_identities
            .iter()
            .position(|existing| existing == &identity)
        {
            return index;
        }
        let index = self.callable_identities.len();
        self.callable_identities.push(identity);
        index
    }
    fn emit_pattern_operand(
        &mut self,
        state: &mut State,
        operand: &PatternOperand,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        let name = match operand {
            PatternOperand::Pinned(name) | PatternOperand::StructSchema(name) => name,
            PatternOperand::Computed(expression) => return self.expression(state, expression),
        };
        let binding = state
            .lookup(name)
            .or_else(|| {
                self.globals
                    .get(name)
                    .map(|mutable| Binding::Global { mutable: *mutable })
            })
            .ok_or_else(|| {
                SourceError::semantic(format!("unknown pinned binding `{name}`"), span.clone())
            })?;
        match binding {
            Binding::Global { .. } => state.emit(Op::GetGlobal(name.into()), span),
            Binding::Local { slot, .. } => state.emit(Op::GetLocal(slot), span),
            Binding::Capture { slot, .. } => state.emit(Op::GetCapture(slot), span),
            Binding::Outer { .. } => unreachable!("outer bindings are captured lazily"),
        }
        Ok(())
    }
    fn bind_pattern(
        &mut self,
        state: &mut State,
        pattern: &Pattern,
        mutable: bool,
        callable: bool,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        if matches!(pattern, Pattern::MapAll) {
            if !state.is_root() {
                return Err(SourceError::semantic(
                    "{*} declarations are only valid at top level",
                    span.clone(),
                ));
            }
            state.emit(Op::DefineMapGlobals, span);
            return Ok(());
        }
        let names = pattern_bindings(pattern, span)?;
        let mut operands = Vec::new();
        let pattern = lower_pattern(pattern, &mut operands);
        for operand in &operands {
            self.emit_pattern_operand(state, operand, span)?;
        }
        state.emit(
            Op::TryMatch {
                pattern,
                bindings: names.len(),
                operands: operands.len(),
            },
            span,
        );
        let failed = state.jump_if_false(span);
        state.emit(Op::Pop, span);
        let bindings = names
            .into_iter()
            .map(|name| {
                let binding = if state.is_root() {
                    Binding::Global { mutable }
                } else {
                    Binding::Local {
                        slot: state.declare(name.clone(), mutable, callable),
                        mutable,
                    }
                };
                (name, binding)
            })
            .collect::<Vec<_>>();
        for (name, binding) in bindings.iter().rev() {
            match binding {
                Binding::Global { .. } => state.emit(Op::DefineGlobal(name.clone()), span),
                Binding::Local { slot, .. } => state.emit(Op::SetLocal(*slot), span),
                Binding::Capture { .. } => unreachable!("new bindings cannot be captures"),
                Binding::Outer { .. } => unreachable!("new bindings cannot be outer bindings"),
            }
        }
        let end = state.jump(span);
        state.patch(failed);
        state.emit(Op::Pop, span);
        for _ in &bindings {
            state.emit(Op::Pop, span);
        }
        state.emit(Op::MatchFailure, span);
        state.patch(end);
        Ok(())
    }
    fn tail_expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        match &expression.kind {
            ExprKind::Recur(arguments) => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "recur is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                let mut kinds = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    let argument = match argument {
                        CallArgument::Positional(argument) => {
                            kinds.push(CallArgumentKind::Positional);
                            argument
                        }
                        CallArgument::Named { name, value } => {
                            kinds.push(CallArgumentKind::Named(name.clone()));
                            value
                        }
                        CallArgument::Spread(argument) => {
                            kinds.push(CallArgumentKind::Spread);
                            argument
                        }
                    };
                    self.expression(state, argument)?;
                }
                state.emit(Op::Recur(kinds), &expression.span);
            }
            ExprKind::Block(values) => {
                state.enter_scope();
                state.emit(Op::EnterScope, &expression.span);
                for (index, value) in values.iter().enumerate() {
                    if index + 1 == values.len() {
                        self.tail_expression(state, value)?;
                    } else {
                        self.expression(state, value)?;
                        state.emit(Op::Pop, &value.span);
                    }
                }
                if values.is_empty() {
                    state.emit(Op::Nil, &expression.span);
                }
                state.emit(Op::LeaveScope, &expression.span);
                state.leave_scope();
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(state, condition)?;
                let otherwise = state.jump_if_false(&expression.span);
                state.emit(Op::Pop, &expression.span);
                self.tail_expression(state, then_branch)?;
                let end = state.jump(&expression.span);
                state.patch(otherwise);
                state.emit(Op::Pop, &expression.span);
                if let Some(otherwise) = else_branch {
                    self.tail_expression(state, otherwise)?;
                } else {
                    state.emit(Op::Nil, &expression.span);
                }
                state.patch(end);
            }
            ExprKind::Match { subject, cases } => {
                let subject = subject.as_deref().ok_or_else(|| {
                    SourceError::semantic(
                        "match requires a subject or pipeline input",
                        expression.span.clone(),
                    )
                })?;
                self.compile_match(state, subject, cases, true)?;
            }
            _ => self.expression(state, expression)?,
        }
        Ok(())
    }
    fn function(
        &mut self,
        parameters: &[String],
        parameter_metadata: &[Parameter],
        body: &Expr,
        parent: &mut State,
    ) -> Result<(Chunk, Vec<Capture>), SourceError> {
        let mut state = State::function(parameters, parent.visible());
        for (slot, parameter) in parameter_metadata.iter().enumerate() {
            if let Some(default) = &parameter.default {
                let end = state.jump_if_provided(slot, &default.span);
                self.expression(&mut state, default)?;
                state.emit(Op::SetLocal(slot), &default.span);
                state.patch(end);
            }
        }
        self.tail_function_body(&mut state, body)?;
        state.emit(Op::Return, &body.span);
        let captures = parent.resolve_child_captures(state.captures());
        Ok((state.finish("<fn>", parameters.len()), captures))
    }

    fn tail_function_body(&mut self, state: &mut State, body: &Expr) -> Result<(), SourceError> {
        let ExprKind::Block(values) = &body.kind else {
            return self.tail_expression(state, body);
        };
        for (index, value) in values.iter().enumerate() {
            if index + 1 == values.len() {
                self.tail_expression(state, value)?;
            } else {
                self.expression(state, value)?;
                state.emit(Op::Pop, &value.span);
            }
        }
        if values.is_empty() {
            state.emit(Op::Nil, &body.span);
        }
        Ok(())
    }
}

fn plain_parameters(
    parameters: &[Parameter],
    span: &SourceSpan,
) -> Result<Vec<String>, SourceError> {
    let mut names = HashSet::new();
    let mut plain = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        if !parameter.discard && !names.insert(&parameter.name) {
            return Err(SourceError::semantic(
                format!("duplicate parameter '{}'", parameter.name),
                span.clone(),
            ));
        }
        plain.push(parameter.name.clone());
    }
    Ok(plain)
}

#[derive(Clone, Debug)]
enum PatternOperand {
    Pinned(String),
    Computed(Expr),
    StructSchema(String),
}

fn lower_pattern(pattern: &Pattern, operands: &mut Vec<PatternOperand>) -> MatchPattern {
    match pattern {
        Pattern::Literal(value) => MatchPattern::Literal(value.clone()),
        Pattern::Wildcard => MatchPattern::Wildcard,
        Pattern::Binding(_) => MatchPattern::Binding,
        Pattern::Pinned(name) => {
            let index = operands.len();
            operands.push(PatternOperand::Pinned(name.clone()));
            MatchPattern::Pinned(index)
        }
        Pattern::At { pattern, .. } => MatchPattern::At(Box::new(lower_pattern(pattern, operands))),
        Pattern::List { items, rest } => MatchPattern::List {
            items: items
                .iter()
                .map(|pattern| lower_pattern(pattern, operands))
                .collect(),
            rest: lower_rest_pattern(rest.as_ref()),
        },
        Pattern::Map {
            entries,
            rest,
            exact,
        } => MatchPattern::Map {
            entries: entries
                .iter()
                .map(|(key, pattern)| {
                    let key = match key {
                        MapPatternKey::String(key) => MatchMapKey::String(key.clone()),
                        MapPatternKey::Computed(expression) => {
                            let index = operands.len();
                            operands.push(PatternOperand::Computed(expression.clone()));
                            MatchMapKey::Operand(index)
                        }
                    };
                    (key, lower_pattern(pattern, operands))
                })
                .collect(),
            rest: lower_rest_pattern(rest.as_ref()),
            exact: *exact,
        },
        Pattern::MapAll => unreachable!("{{*}} declarations do not lower to match bytecode"),
    }
}

fn lower_case_patterns(
    patterns: &[CasePattern],
    span: &SourceSpan,
) -> Result<(MatchPattern, Vec<String>, Vec<PatternOperand>), SourceError> {
    if patterns
        .iter()
        .any(|pattern| matches!(pattern.pattern, Pattern::MapAll))
    {
        return Err(SourceError::semantic(
            "{*} is only valid in a top-level declaration",
            span.clone(),
        ));
    }
    let mut operands = Vec::new();
    if let [pattern] = patterns {
        return Ok((
            lower_case_pattern(pattern, &mut operands)?,
            pattern_bindings(&pattern.pattern, span)?,
            operands,
        ));
    }

    for pattern in patterns {
        if !pattern_bindings(&pattern.pattern, span)?.is_empty() {
            return Err(SourceError::semantic(
                "match alternatives cannot introduce bindings",
                span.clone(),
            ));
        }
    }
    Ok((
        MatchPattern::Alternatives(
            patterns
                .iter()
                .map(|pattern| lower_case_pattern(pattern, &mut operands))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Vec::new(),
        operands,
    ))
}

fn lower_case_pattern(
    case_pattern: &CasePattern,
    operands: &mut Vec<PatternOperand>,
) -> Result<MatchPattern, SourceError> {
    let pattern = lower_pattern(&case_pattern.pattern, operands);
    let Some(constraint) = &case_pattern.constraint else {
        return Ok(pattern);
    };
    Ok(MatchPattern::Constrained {
        pattern: Box::new(pattern),
        constraint: lower_match_type(constraint, operands)?,
    })
}

fn lower_match_type(
    annotation: &TypeAnnotation,
    operands: &mut Vec<PatternOperand>,
) -> Result<MatchType, SourceError> {
    match annotation {
        TypeAnnotation::Name(name) => match name.as_str() {
            "any" => Ok(MatchType::Any),
            "nil" => Ok(MatchType::Nil),
            "bool" => Ok(MatchType::Bool),
            "num" => Ok(MatchType::Num),
            "str" => Ok(MatchType::Str),
            "bytes" => Ok(MatchType::Bytes),
            "list" => Ok(MatchType::List(None)),
            "map" => Ok(MatchType::Map(None)),
            "fn" => Ok(MatchType::Function),
            "task" => Ok(MatchType::Task),
            "chan" => Ok(MatchType::Channel),
            "schema" => Ok(MatchType::Schema),
            "struct" => Ok(MatchType::Struct(None)),
            _ => Err(SourceError::semantic(
                format!("unknown type `{name}`"),
                SourceSpan::new("<internal>", 1, 1),
            )),
        },
        TypeAnnotation::Apply { name, arguments } => match (name.as_str(), arguments.as_slice()) {
            ("list", [element]) => Ok(MatchType::List(Some(Box::new(lower_match_type(
                element, operands,
            )?)))),
            ("map", [key, value]) => Ok(MatchType::Map(Some((
                Box::new(lower_match_type(key, operands)?),
                Box::new(lower_match_type(value, operands)?),
            )))),
            ("struct", [TypeAnnotation::Name(name)]) => {
                let index = operands.len();
                operands.push(PatternOperand::StructSchema(name.clone()));
                Ok(MatchType::Struct(Some(index)))
            }
            _ => Err(SourceError::semantic(
                "match type constraint is not runtime-checkable",
                SourceSpan::new("<internal>", 1, 1),
            )),
        },
        TypeAnnotation::Union(members) => members
            .iter()
            .map(|member| lower_match_type(member, operands))
            .collect::<Result<Vec<_>, _>>()
            .map(MatchType::Union),
        TypeAnnotation::Tuple(_) => Err(SourceError::semantic(
            "match type constraint is not runtime-checkable",
            SourceSpan::new("<internal>", 1, 1),
        )),
    }
}

fn pattern_bindings(pattern: &Pattern, span: &SourceSpan) -> Result<Vec<String>, SourceError> {
    fn collect(pattern: &Pattern, names: &mut Vec<String>) {
        match pattern {
            Pattern::Binding(name) => names.push(name.clone()),
            Pattern::At { name, pattern } => {
                names.push(name.clone());
                collect(pattern, names);
            }
            Pattern::List { items, rest } => {
                for item in items {
                    collect(item, names);
                }
                if let Some(RestPattern::Binding(name)) = rest {
                    names.push(name.clone());
                }
            }
            Pattern::Map { entries, rest, .. } => {
                for (_, pattern) in entries {
                    collect(pattern, names);
                }
                if let Some(RestPattern::Binding(name)) = rest {
                    names.push(name.clone());
                }
            }
            Pattern::MapAll | Pattern::Literal(_) | Pattern::Wildcard | Pattern::Pinned(_) => {}
        }
    }

    let mut names = Vec::new();
    collect(pattern, &mut names);
    let mut seen = HashSet::new();
    if let Some(name) = names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(SourceError::semantic(
            format!("duplicate match binding `{name}`"),
            span.clone(),
        ));
    }
    Ok(names)
}

fn lower_rest_pattern(rest: Option<&RestPattern>) -> MatchRest {
    match rest {
        None => MatchRest::None,
        Some(RestPattern::Discard) => MatchRest::Discard,
        Some(RestPattern::Binding(_)) => MatchRest::Binding,
    }
}

fn foreign_arity(signature: &ForeignSignature) -> (usize, Option<usize>) {
    let required = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.variadic && parameter.default.is_none())
        .count();
    let maximum = (!signature
        .parameters
        .iter()
        .any(|parameter| parameter.variadic))
    .then_some(signature.parameters.len());
    (required, maximum)
}

fn entrypoint_candidate(expression: &Expr) -> Option<(EntrypointArguments, &SourceSpan)> {
    let ExprKind::Declare { pattern, value, .. } = &expression.kind else {
        return None;
    };
    let Pattern::Binding(name) = pattern else {
        return None;
    };
    let ExprKind::Function {
        type_parameters,
        parameters,
        ..
    } = &value.kind
    else {
        return None;
    };
    if name != "main" || !type_parameters.is_empty() {
        return None;
    }
    let arguments = match parameters.as_slice() {
        [] => EntrypointArguments::None,
        [parameter]
            if parameter.default.is_none()
                && !parameter.variadic
                && matches!(&parameter.annotation, Some(TypeAnnotation::Name(name)) if name == "list") =>
        {
            EntrypointArguments::List
        }
        [parameter]
            if parameter.default.is_none()
                && !parameter.variadic
                && matches!(&parameter.annotation, Some(TypeAnnotation::Name(name)) if name == "map") =>
        {
            EntrypointArguments::Map
        }
        _ => return None,
    };
    Some((arguments, &value.span))
}
