use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use slug_vm::{
    CallArgumentKind, CallableIdentity, Capture, Chunk, Entrypoint, EntrypointArguments, EnumValue,
    ForeignResourceSignature, MatchMapKey, MatchPattern, MatchRest, MatchType, ModuleDeclaration,
    ModuleTag, Op, ParameterSignature, Program, ProgramBuilder, SchemaField, SelectCase,
    SourceSpan, Value,
};

use super::{
    SourceEdit, SourceError, SourceErrorCode, SourcePosition, SourceRange,
    ast::{
        Binary, CallArgument, CasePattern, Expr, ExprKind, ForeignSignature, ListElement,
        MapPatternKey, MatchCase, Parameter, Pattern, Prefix, RestPattern, SelectCaseKind,
        StringPart, Tag, TypeAnnotation,
    },
    environment::SemanticAnalysis,
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
    foreign_resource_signatures: HashMap<SourceSpan, ForeignResourceSignature>,
    match_constraints: HashMap<SourceSpan, Vec<Option<super::semantic::Type>>>,
    expression_types: HashMap<SourceSpan, super::semantic::Type>,
    callable_identities: Vec<CallableIdentity>,
    guard_comparisons: bool,
    /// The names the runtime provides to every module, when they are known
    /// at compile time. A name that is neither these nor declared is then
    /// an error; otherwise it is looked up when it runs.
    implicit_globals: Option<HashSet<String>>,
    /// The name the module is imported by, which its enum values carry.
    module_name: Option<String>,
    /// The names `{*}` declarations define, when they are known.
    selected_globals: Option<HashSet<String>>,
}

/// The VM's own builtins, which it installs in every module's globals.
const VM_BUILTINS: [&str; 4] = ["cfg", "int", "stacktrace", "describe"];

pub(super) struct CompiledProgram {
    pub(super) program: Program,
    pub(super) globals: HashMap<String, bool>,
    pub(super) callable_globals: HashSet<String>,
}

impl Compiler {
    pub(super) fn new(path: &str, expressions: Vec<Expr>, analysis: &SemanticAnalysis) -> Self {
        Self::with_globals(path, expressions, analysis, HashMap::new(), HashSet::new())
    }

    pub(super) fn with_globals(
        path: &str,
        expressions: Vec<Expr>,
        analysis: &SemanticAnalysis,
        globals: HashMap<String, bool>,
        callable_globals: HashSet<String>,
    ) -> Self {
        Self {
            path: path.into(),
            expressions,
            chunks: Vec::new(),
            globals,
            callable_globals,
            declarations: Vec::new(),
            selected_calls: analysis.selected_calls.clone(),
            function_identities: analysis.function_identities.clone(),
            foreign_identities: analysis.foreign_identities.clone(),
            foreign_resource_signatures: analysis.foreign_resource_signatures.clone(),
            match_constraints: analysis.match_constraints.clone(),
            expression_types: analysis.expression_types.clone(),
            callable_identities: Vec::new(),
            guard_comparisons: false,
            implicit_globals: None,
            module_name: None,
            selected_globals: analysis.selected_globals.clone(),
        }
    }

    /// Makes a name that is neither declared nor one of `names`, the
    /// exports of the implicit `slug.builtin` module, nor a VM builtin an
    /// error at compile time.
    pub(super) fn with_module_name(mut self, name: &str) -> Self {
        self.module_name = Some(name.into());
        self
    }

    pub(super) fn with_implicit_globals(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.implicit_globals = Some(
            names
                .into_iter()
                .chain(VM_BUILTINS.map(String::from))
                .collect(),
        );
        self
    }

    /// Whether `name` is certainly not a global the runtime will have.
    fn is_unknown_global(&self, name: &str) -> bool {
        let (Some(implicit), Some(selected)) = (&self.implicit_globals, &self.selected_globals)
        else {
            return false;
        };
        !implicit.contains(name) && !selected.contains(name)
    }

    /// The error for a name that nothing declares, suggesting the names in
    /// scope that it most likely misspells.
    fn unknown_name(&self, name: &str, expression: &Expr, state: &State) -> SourceError {
        let mut candidates = state.visible().into_keys().collect::<HashSet<_>>();
        candidates.extend(self.globals.keys().cloned());
        candidates.extend(self.implicit_globals.iter().flatten().cloned());
        candidates.extend(self.selected_globals.iter().flatten().cloned());
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        candidates.sort();
        let suggestions =
            slug_loader::suggest_module_names(name, candidates.iter().map(String::as_str));
        let message = match suggestions.as_slice() {
            [] => format!("unknown name `{name}`"),
            [suggestion] => format!("unknown name `{name}`; did you mean `{suggestion}`?"),
            suggestions => format!(
                "unknown name `{name}`; did you mean one of {}?",
                suggestions
                    .iter()
                    .map(|suggestion| format!("`{suggestion}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let range = SourceRange {
            start: expression.extent_start(),
            end: expression.end,
        };
        suggestions.into_iter().fold(
            SourceError::semantic(
                SourceErrorCode::UnknownName,
                message,
                expression.span.clone(),
            )
            .with_extent(expression),
            |error, suggestion| {
                error.with_suggestion(
                    format!("replace with `{suggestion}`"),
                    SourceEdit {
                        range,
                        text: suggestion,
                    },
                )
            },
        )
    }
    #[allow(clippy::too_many_lines)]
    pub(super) fn compile(mut self) -> Result<CompiledProgram, SourceError> {
        let mut exports = Vec::new();
        let mut bindings = Vec::new();
        let mut foreign_callable_identities = Vec::new();
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
                    resource_type: None,
                    foreign_callable_identity: None,
                    foreign_resource_signature: None,
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
                if let Some(identity) = &foreign_callable_identity {
                    foreign_callable_identities.push(identity.clone());
                }
                let foreign_resource_signature = self
                    .foreign_resource_signatures
                    .get(&expression.span)
                    .cloned();
                self.declarations.push(ModuleDeclaration {
                    bindings: vec![name.clone()],
                    mutable: false,
                    exported: *exported,
                    foreign: true,
                    foreign_arity: Some(foreign_arity(signature)),
                    resource_type: None,
                    foreign_callable_identity,
                    foreign_resource_signature,
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
            if let ExprKind::Resource {
                exported,
                name,
                documentation,
                tags,
            } = &expression.kind
            {
                self.declarations.push(ModuleDeclaration {
                    bindings: vec![name.clone()],
                    mutable: false,
                    exported: *exported,
                    foreign: false,
                    foreign_arity: None,
                    resource_type: Some(name.clone()),
                    foreign_callable_identity: None,
                    foreign_resource_signature: None,
                    documentation: documentation.clone(),
                    tags: tags
                        .iter()
                        .map(|tag| ModuleTag {
                            name: tag.name.clone(),
                            arguments: Vec::new(),
                        })
                        .collect(),
                });
            }
            if let ExprKind::Enum {
                exported,
                name,
                documentation,
                tags,
                ..
            } = &expression.kind
            {
                self.declarations.push(ModuleDeclaration {
                    bindings: vec![name.clone()],
                    mutable: false,
                    exported: *exported,
                    foreign: false,
                    foreign_arity: None,
                    resource_type: None,
                    foreign_callable_identity: None,
                    foreign_resource_signature: None,
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
            }
        }
        bindings.sort();
        bindings.dedup();
        let expressions = self.expressions.clone();
        let mut state = State::root();
        for (index, expression) in expressions.iter().enumerate() {
            if index + 1 < expressions.len() {
                self.statement(&mut state, expression)?;
            } else {
                self.expression(&mut state, expression)?;
            }
        }
        if expressions.is_empty() {
            state.emit(Op::Nil, &SourceSpan::new(self.path.clone(), 1, 1));
        }
        state.emit(Op::Return, &SourceSpan::new(self.path.clone(), 1, 1));
        self.chunks.push(state.finish("main", 0));
        for identity in foreign_callable_identities {
            self.intern_callable_identity(identity);
        }
        let mut program = ProgramBuilder::new();
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
        Ok(CompiledProgram {
            program: program.finish(),
            globals: self.globals,
            callable_globals: self.callable_globals,
        })
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
                    SourceErrorCode::InvalidEntrypoint,
                    "program has more than one eligible `main` entrypoint",
                    span,
                ));
            }
            let identity = self
                .function_identities
                .get(&span)
                .cloned()
                .ok_or_else(|| {
                    SourceError::semantic(
                        SourceErrorCode::InvalidEntrypoint,
                        "cannot identify program entrypoint",
                        span.clone(),
                    )
                })?;
            entrypoint = Some(Entrypoint::new(
                arguments,
                self.intern_callable_identity(identity),
            ));
        }
        Ok(entrypoint)
    }

    /// Compiles a block statement, whose value is discarded. An `if`
    /// without `else` skips the `nil` its false branch would push only to
    /// be popped.
    fn statement(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        let ExprKind::If {
            condition,
            then_branch,
            else_branch: None,
        } = &expression.kind
        else {
            self.expression(state, expression)?;
            state.emit(Op::Pop, &expression.span);
            return Ok(());
        };
        super::stack::grow(|| {
            self.expression(state, condition)?;
            let end = state.jump_if_false_pop(&expression.span);
            self.expression(state, then_branch)?;
            state.emit(Op::Pop, &expression.span);
            state.patch(end);
            Ok(())
        })
        .map_err(|error: SourceError| error.claimed_by(expression))
    }

    fn expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        super::stack::grow(|| {
            self.expression_inner(state, expression)
                .map_err(|error| error.claimed_by(expression))
        })
    }

    #[allow(clippy::too_many_lines)]
    fn expression_inner(
        &mut self,
        state: &mut State,
        expression: &Expr,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
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
                                    &Expr::new(
                                        ExprKind::Name(name.clone()),
                                        expression.span.clone(),
                                        expression.end,
                                    ),
                                )?;
                                text.push(String::new());
                            }
                        }
                    }
                    state.emit(Op::Interpolate(text), &expression.span);
                }
                ExprKind::Documentation(content) => {
                    debug_assert!(
                        content.lines().all(
                            |line| line.trim().is_empty() || line.trim_start().starts_with('*')
                        )
                    );
                    state.emit(Op::Nil, &expression.span);
                }
                ExprKind::NotImplemented => state.emit(Op::NotImplemented, &expression.span),
                ExprKind::Name(name) if state.before_declaration(name) => {
                    return Err(SourceError::semantic(
                        SourceErrorCode::UnknownName,
                        format!("`{name}` is used before its declaration"),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                ExprKind::Name(name) => match state.lookup(name).or_else(|| {
                    self.globals
                        .get(name)
                        .map(|mutable| Binding::Global { mutable: *mutable })
                }) {
                    None if state.declared_later(name) => {
                        return Err(SourceError::semantic(
                        SourceErrorCode::UnknownName,
                        format!(
                            "`{name}` is declared later in this block; only a function declared with `fn` can be used before its declaration"
                        ),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                    }
                    None if self.is_unknown_global(name) => {
                        return Err(self.unknown_name(name, expression, state));
                    }
                    Some(Binding::Global { .. }) | None => {
                        state.emit(Op::GetGlobal(name.clone()), &expression.span);
                    }
                    Some(Binding::Local { slot, .. }) => {
                        state.emit(Op::GetLocal(slot), &expression.span);
                    }
                    Some(Binding::Capture { slot, .. }) => {
                        state.emit(Op::GetCapture(slot), &expression.span);
                    }
                    Some(Binding::Inherited { .. }) => {
                        unreachable!("inherited bindings are captured lazily")
                    }
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
                        debug_assert!(documentation.lines().all(
                            |line| line.trim().is_empty() || line.trim_start().starts_with('*')
                        ));
                    }
                    let declaration = if state.is_module_scope() {
                        Some(self.module_declaration_index(expression))
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
                    // A local function is bound before its body is compiled, so
                    // the body can call it by name; captures are live, so the
                    // slot holds the function by the time the body runs. Module
                    // functions already resolve by name when called.
                    let own_slot = callable_name
                        .filter(|_| existing_callable.is_none() && !state.is_root())
                        .map(|name| {
                            state
                                .claim_hoisted(name)
                                .unwrap_or_else(|| state.declare(name.clone(), *mutable, true))
                        });
                    self.expression(state, value)?;
                    if let Some(slot) = own_slot {
                        state.emit(Op::SetLocal(slot), &expression.span);
                    } else if let Some(binding) = existing_callable {
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
                            Binding::Inherited { .. } => {
                                unreachable!("current scope cannot contain an inherited binding")
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
                            Binding::Inherited { .. } => {
                                unreachable!("current scope cannot contain an inherited binding")
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
                ExprKind::Foreign { tags, .. } | ExprKind::Resource { tags, .. } => {
                    let declaration = self.module_declaration_index(expression);
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
                                SourceErrorCode::UnknownName,
                                format!("unknown name `{name}`"),
                                expression.span.clone(),
                            )
                            .with_extent(expression)
                        })?;
                    let mutable = match binding {
                        Binding::Global { mutable }
                        | Binding::Local { mutable, .. }
                        | Binding::Capture { mutable, .. }
                        | Binding::Inherited { mutable } => mutable,
                    };
                    if !mutable {
                        return Err(SourceError::semantic(
                            SourceErrorCode::ImmutableAssignment,
                            format!("cannot assign to immutable binding `{name}`"),
                            expression.span.clone(),
                        )
                        .with_extent(expression));
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
                        Binding::Inherited { .. } => {
                            unreachable!("inherited bindings are captured lazily")
                        }
                    }
                }
                ExprKind::Return { value } => {
                    if !state.allows_return() {
                        return Err(SourceError::semantic(
                            SourceErrorCode::InvalidControlFlow,
                            "return is only valid inside a function",
                            expression.span.clone(),
                        )
                        .with_extent(expression));
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
                    let deferred = Expr::new(
                        ExprKind::Function {
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
                        expression.span.clone(),
                        expression.end,
                    );
                    self.expression(state, &deferred)?;
                    state.emit(Op::Defer { mode: *mode }, &expression.span);
                    state.emit(Op::Nil, &expression.span);
                }
                ExprKind::Recur(_) => {
                    if !state.allows_return() {
                        return Err(SourceError::semantic(
                            SourceErrorCode::InvalidControlFlow,
                            "recur is only valid inside a function",
                            expression.span.clone(),
                        )
                        .with_extent(expression));
                    }
                    return Err(SourceError::semantic(
                        SourceErrorCode::InvalidControlFlow,
                        "recur is only valid in tail position",
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                ExprKind::Spawn(body) => {
                    let function = Expr::new(
                        ExprKind::Function {
                            type_parameters: Vec::new(),
                            parameters: Vec::new(),
                            return_annotation: None,
                            body: body.clone(),
                        },
                        expression.span.clone(),
                        expression.end,
                    );
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
                        Expr::new(
                            ExprKind::Function {
                                type_parameters: Vec::new(),
                                parameters: Vec::new(),
                                return_annotation: None,
                                body: body.clone(),
                            },
                            expression.span.clone(),
                            expression.end,
                        )
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
                            SourceErrorCode::InvalidMatch,
                            "match requires a subject or pipeline input",
                            expression.span.clone(),
                        )
                        .with_extent(expression)
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
                                    SourceErrorCode::InvalidMatch,
                                    "pipeline match must omit its subject",
                                    right.span.clone(),
                                )
                                .with_extent(right));
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
                        Binary::Coalesce => {
                            state.emit(Op::Duplicate, &expression.span);
                            state.emit(Op::Nil, &expression.span);
                            state.emit(Op::Equal, &expression.span);
                            let end = state.jump_if_false_pop(&expression.span);
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
                            state.emit(self.less_op(left, right), &expression.span);
                            state.emit(Op::Not, &expression.span);
                        }
                        Binary::LessEqual => {
                            self.expression(state, right)?;
                            state.emit(self.greater_op(left, right), &expression.span);
                            state.emit(Op::Not, &expression.span);
                        }
                        Binary::Add => {
                            self.expression(state, right)?;
                            state.emit(
                                if self.numeric_operands(left, right) {
                                    Op::AddNum
                                } else {
                                    Op::Add
                                },
                                &expression.span,
                            );
                        }
                        Binary::Subtract => {
                            self.expression(state, right)?;
                            state.emit(
                                if self.numeric_operands(left, right) {
                                    Op::SubtractNum
                                } else {
                                    Op::Subtract
                                },
                                &expression.span,
                            );
                        }
                        Binary::Multiply => {
                            self.expression(state, right)?;
                            state.emit(
                                if self.numeric_operands(left, right) {
                                    Op::MultiplyNum
                                } else {
                                    Op::Multiply
                                },
                                &expression.span,
                            );
                        }
                        Binary::Divide => {
                            self.expression(state, right)?;
                            state.emit(
                                if self.numeric_operands(left, right) {
                                    Op::DivideNum
                                } else {
                                    Op::Divide
                                },
                                &expression.span,
                            );
                        }
                        Binary::Modulo => {
                            self.expression(state, right)?;
                            state.emit(
                                if self.numeric_operands(left, right) {
                                    Op::ModuloNum
                                } else {
                                    Op::Modulo
                                },
                                &expression.span,
                            );
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
                            state.emit(self.greater_op(left, right), &expression.span);
                        }
                        Binary::Less => {
                            self.expression(state, right)?;
                            state.emit(self.less_op(left, right), &expression.span);
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
                    let positional = arguments
                        .iter()
                        .all(|argument| matches!(argument, CallArgument::Positional(_)));
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
                    } else if positional {
                        Op::CallPositional(arguments.len())
                    } else {
                        Op::CallSpread(kinds)
                    };
                    state.emit(op, &expression.span);
                }
                ExprKind::TypeApply { callee, .. } => self.expression(state, callee)?,
                ExprKind::TypeAlias { .. } => {
                    state.emit(Op::Nil, &expression.span);
                }
                ExprKind::Enum {
                    name, cases, tags, ..
                } => {
                    let declaration = self.module_declaration_index(expression);
                    self.tags(state, tags, Some(declaration), &expression.span)?;
                    for case in cases {
                        let key = state.constant(Value::string(case.clone()));
                        state.emit(Op::Constant(key), &expression.span);
                        let value = state.constant(Value::Enum(Rc::new(EnumValue {
                            module: self
                                .module_name
                                .as_ref()
                                .unwrap_or(&self.path)
                                .clone()
                                .into(),
                            name: name.clone().into(),
                            case: case.clone().into(),
                        })));
                        state.emit(Op::Constant(value), &expression.span);
                    }
                    state.emit(Op::Map(cases.len()), &expression.span);
                    state.emit(Op::DefineGlobal(name.clone()), &expression.span);
                    state.emit(Op::GetGlobal(name.clone()), &expression.span);
                }
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
                    let mut constant_keys = HashSet::new();
                    for (key, value) in entries {
                        if let ExprKind::Value(constant) = &key.kind
                            && let Some(canonical) = ConstantMapKey::new(constant)
                            && !constant_keys.insert(canonical)
                        {
                            return Err(SourceError::semantic(
                                SourceErrorCode::RepeatedMapKey,
                                format!("map literal repeats key `{constant}`"),
                                key.span.clone(),
                            )
                            .with_extent(key));
                        }
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
                                SourceErrorCode::DuplicateName,
                                format!("duplicate struct field '{}'", field.name),
                                expression.span.clone(),
                            )
                            .with_extent(expression));
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
                    if let ExprKind::Value(key) = &index.kind {
                        let constant = state.constant(key.clone());
                        state.emit(Op::GetIndexConstant(constant), &expression.span);
                    } else {
                        self.expression(state, index)?;
                        state.emit(Op::GetIndex, &expression.span);
                    }
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
                    hoist_functions(state, values);
                    let runtime_scope = values.iter().any(contains_scoped_defer);
                    if runtime_scope {
                        state.emit(Op::EnterScope, &expression.span);
                    }
                    for (index, value) in values.iter().enumerate() {
                        if index + 1 < values.len() {
                            self.statement(state, value)?;
                        } else {
                            self.expression(state, value)?;
                        }
                    }
                    if values.is_empty() {
                        state.emit(Op::Nil, &expression.span);
                    }
                    if runtime_scope {
                        state.emit(Op::LeaveScope, &expression.span);
                    }
                    state.leave_scope();
                }
                ExprKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.expression(state, condition)?;
                    let otherwise = state.jump_if_false_pop(&expression.span);
                    self.expression(state, then_branch)?;
                    let end = state.jump(&expression.span);
                    state.patch(otherwise);
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
        })
    }
    fn tags(
        &mut self,
        state: &mut State,
        tags: &[Tag],
        declaration: Option<usize>,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
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
        })
    }

    fn module_declaration_index(&self, expression: &Expr) -> usize {
        self.expressions
            .iter()
            .filter(|candidate| {
                matches!(
                    candidate.kind,
                    ExprKind::Declare { .. }
                        | ExprKind::Foreign { .. }
                        | ExprKind::Resource { .. }
                        | ExprKind::Enum { .. }
                )
            })
            .position(|candidate| candidate.span == expression.span)
            .expect("compiled declaration was recorded")
    }
    fn compile_match(
        &mut self,
        state: &mut State,
        subject: &Expr,
        cases: &[MatchCase],
        tail: bool,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
            self.expression(state, subject)?;
            let mut ends = Vec::new();
            for case in cases {
                let constraints = self.match_constraints.get(&case.span).ok_or_else(|| {
                    SourceError::semantic(
                        SourceErrorCode::Internal,
                        "missing resolved match constraints",
                        case.span.clone(),
                    )
                    .with_end(case.end)
                })?;
                let (pattern, names, operands) =
                    lower_case_patterns(&case.patterns, constraints, &case.span)?;
                for operand in &operands {
                    self.emit_pattern_operand(state, operand, &case.span)?;
                }
                let next = state.match_case(pattern, operands.len(), &case.span);
                state.enter_scope();
                let slots = names
                    .into_iter()
                    .map(|name| state.declare(name, false, false))
                    .collect::<Vec<_>>();
                for slot in slots.iter().rev() {
                    state.emit(Op::SetLocal(*slot), &case.span);
                }
                let guard_next = if let Some(guard) = &case.guard {
                    self.guard_expression(state, guard)?;
                    Some(state.jump_if_false_pop(&case.span))
                } else {
                    None
                };
                state.emit(Op::Pop, &case.span);
                if tail {
                    self.tail_expression(state, &case.value)?;
                } else {
                    self.expression(state, &case.value)?;
                }
                state.leave_scope();
                ends.push(state.jump(&case.span));
                state.patch(next);
                if let Some(guard_next) = guard_next {
                    state.patch(guard_next);
                }
            }
            state.emit(Op::Pop, &subject.span);
            state.emit(Op::Nil, &subject.span);
            for end in ends {
                state.patch(end);
            }
            Ok(())
        })
    }

    fn guard_expression(
        &mut self,
        state: &mut State,
        expression: &Expr,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
            let previous = std::mem::replace(&mut self.guard_comparisons, true);
            let result = self.expression(state, expression);
            self.guard_comparisons = previous;
            result
        })
    }

    fn greater_op(&self, left: &Expr, right: &Expr) -> Op {
        if self.guard_comparisons {
            Op::GuardGreater
        } else if self.numeric_operands(left, right) {
            Op::GreaterNum
        } else {
            Op::Greater
        }
    }

    fn less_op(&self, left: &Expr, right: &Expr) -> Op {
        if self.guard_comparisons {
            Op::GuardLess
        } else if self.numeric_operands(left, right) {
            Op::LessNum
        } else {
            Op::Less
        }
    }

    fn compile_pipeline_call(
        &mut self,
        state: &mut State,
        expression: &Expr,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
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
        })
    }

    fn numeric_operands(&self, left: &Expr, right: &Expr) -> bool {
        matches!(
            (
                self.expression_types.get(&left.span),
                self.expression_types.get(&right.span),
            ),
            (
                Some(super::semantic::Type::Num),
                Some(super::semantic::Type::Num),
            )
        )
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
                SourceError::semantic(
                    SourceErrorCode::UnknownName,
                    format!("unknown pinned binding `{name}`"),
                    span.clone(),
                )
            })?;
        match binding {
            Binding::Global { .. } => state.emit(Op::GetGlobal(name.into()), span),
            Binding::Local { slot, .. } => state.emit(Op::GetLocal(slot), span),
            Binding::Capture { slot, .. } => state.emit(Op::GetCapture(slot), span),
            Binding::Inherited { .. } => unreachable!("inherited bindings are captured lazily"),
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
        super::stack::grow(|| {
            if matches!(pattern, Pattern::MapAll) {
                if !state.is_root() {
                    return Err(SourceError::semantic(
                        SourceErrorCode::TopLevelOnly,
                        "{*} declarations are only valid at top level",
                        span.clone(),
                    ));
                }
                state.emit(Op::DefineMapGlobals, span);
                return Ok(());
            }
            let names = pattern_bindings(pattern, span)?;
            // A plain local name always matches, so it binds the value
            // directly; `SetLocal` resolves a module binding as the match
            // would. A top-level name keeps the match, because
            // `DefineGlobal` stores a binding unresolved.
            if let Pattern::Binding(name) = pattern
                && !state.is_root()
            {
                let slot = state.declare(name.clone(), mutable, callable);
                state.emit(Op::SetLocal(slot), span);
                return Ok(());
            }
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
            let failed = state.jump_if_false_pop(span);
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
                    Binding::Inherited { .. } => unreachable!("new bindings cannot be inherited"),
                }
            }
            let end = state.jump(span);
            state.patch(failed);
            for _ in &bindings {
                state.emit(Op::Pop, span);
            }
            state.emit(Op::MatchFailure, span);
            state.patch(end);
            Ok(())
        })
    }
    #[allow(clippy::too_many_lines)]
    fn tail_expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        super::stack::grow(|| {
            match &expression.kind {
                ExprKind::Recur(arguments) => {
                    if !state.allows_return() {
                        return Err(SourceError::semantic(
                            SourceErrorCode::InvalidControlFlow,
                            "recur is only valid inside a function",
                            expression.span.clone(),
                        )
                        .with_extent(expression));
                    }
                    let positional = arguments
                        .iter()
                        .all(|argument| matches!(argument, CallArgument::Positional(_)));
                    let mut kinds = (!positional).then(|| Vec::with_capacity(arguments.len()));
                    for argument in arguments {
                        let argument = match argument {
                            CallArgument::Positional(argument) => {
                                if let Some(kinds) = &mut kinds {
                                    kinds.push(CallArgumentKind::Positional);
                                }
                                argument
                            }
                            CallArgument::Named { name, value } => {
                                kinds
                                    .as_mut()
                                    .expect("non-positional recur needs argument metadata")
                                    .push(CallArgumentKind::Named(name.clone()));
                                value
                            }
                            CallArgument::Spread(argument) => {
                                kinds
                                    .as_mut()
                                    .expect("non-positional recur needs argument metadata")
                                    .push(CallArgumentKind::Spread);
                                argument
                            }
                        };
                        self.expression(state, argument)?;
                    }
                    state.emit(
                        if positional {
                            Op::RecurPositional(arguments.len())
                        } else {
                            Op::Recur(kinds.expect("non-positional recur collected metadata"))
                        },
                        &expression.span,
                    );
                }
                ExprKind::Block(values) => {
                    state.enter_scope();
                    hoist_functions(state, values);
                    let runtime_scope = values.iter().any(contains_scoped_defer);
                    if runtime_scope {
                        state.emit(Op::EnterScope, &expression.span);
                    }
                    for (index, value) in values.iter().enumerate() {
                        if index + 1 == values.len() {
                            self.tail_expression(state, value)?;
                        } else {
                            self.statement(state, value)?;
                        }
                    }
                    if values.is_empty() {
                        state.emit(Op::Nil, &expression.span);
                    }
                    if runtime_scope {
                        state.emit(Op::LeaveScope, &expression.span);
                    }
                    state.leave_scope();
                }
                ExprKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.expression(state, condition)?;
                    let otherwise = state.jump_if_false_pop(&expression.span);
                    self.tail_expression(state, then_branch)?;
                    let end = state.jump(&expression.span);
                    state.patch(otherwise);
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
                            SourceErrorCode::InvalidMatch,
                            "match requires a subject or pipeline input",
                            expression.span.clone(),
                        )
                        .with_extent(expression)
                    })?;
                    self.compile_match(state, subject, cases, true)?;
                }
                _ => self.expression(state, expression)?,
            }
            Ok(())
        })
    }
    fn function(
        &mut self,
        parameters: &[String],
        parameter_metadata: &[Parameter],
        body: &Expr,
        parent: &mut State,
    ) -> Result<(Chunk, Vec<Capture>), SourceError> {
        super::stack::grow(|| {
            let mut state = State::function(parameters, parent.visible());
            state.inherit_later(parent);
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
        })
    }

    fn tail_function_body(&mut self, state: &mut State, body: &Expr) -> Result<(), SourceError> {
        super::stack::grow(|| {
            let ExprKind::Block(values) = &body.kind else {
                return self.tail_expression(state, body);
            };
            hoist_functions(state, values);
            for (index, value) in values.iter().enumerate() {
                if index + 1 == values.len() {
                    self.tail_expression(state, value)?;
                } else {
                    self.statement(state, value)?;
                }
            }
            if values.is_empty() {
                state.emit(Op::Nil, &body.span);
            }
            Ok(())
        })
    }
}

/// Declares the slot of each function a block declares by name, so the
/// block's functions can call one another whatever their order. Only
/// functions are hoisted: a function body runs after the block has declared
/// it, while other values could be read before they exist.
fn hoist_functions(state: &mut State, values: &[Expr]) {
    if state.is_root() {
        return;
    }
    for value in values {
        if let ExprKind::Declare {
            pattern: Pattern::Binding(name),
            value: declared,
            mutable,
            ..
        } = &value.kind
        {
            if matches!(declared.kind, ExprKind::Function { .. }) {
                state.hoist(name, *mutable);
            } else {
                state.declare_later(name);
            }
        }
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
                SourceErrorCode::DuplicateName,
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
    super::stack::grow(|| match pattern {
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
        Pattern::EnumCase { path, case } => {
            let mut value = Expr::new(
                ExprKind::Name(path.split('.').next().expect("enum path has a name").into()),
                SourceSpan::new("<internal>", 1, 1),
                SourcePosition { line: 1, column: 1 },
            );
            for member in path
                .split('.')
                .skip(1)
                .chain(std::iter::once(case.as_str()))
            {
                value = Expr::new(
                    ExprKind::Index {
                        collection: Box::new(value),
                        index: Box::new(Expr::new(
                            ExprKind::Value(Value::string(member)),
                            SourceSpan::new("<internal>", 1, 1),
                            SourcePosition { line: 1, column: 1 },
                        )),
                    },
                    SourceSpan::new("<internal>", 1, 1),
                    SourcePosition { line: 1, column: 1 },
                );
            }
            let index = operands.len();
            operands.push(PatternOperand::Computed(value));
            MatchPattern::Enum(index)
        }
    })
}

fn lower_case_patterns(
    patterns: &[CasePattern],
    constraints: &[Option<super::semantic::Type>],
    span: &SourceSpan,
) -> Result<(MatchPattern, Vec<String>, Vec<PatternOperand>), SourceError> {
    if patterns.len() != constraints.len() {
        return Err(SourceError::semantic(
            SourceErrorCode::Internal,
            "missing resolved match constraint",
            span.clone(),
        ));
    }
    if patterns
        .iter()
        .any(|pattern| matches!(pattern.pattern, Pattern::MapAll))
    {
        return Err(SourceError::semantic(
            SourceErrorCode::TopLevelOnly,
            "{*} is only valid in a top-level declaration",
            span.clone(),
        ));
    }
    let mut operands = Vec::new();
    if let [pattern] = patterns {
        return Ok((
            lower_case_pattern(pattern, constraints[0].as_ref(), &mut operands)?,
            pattern_bindings(&pattern.pattern, span)?,
            operands,
        ));
    }

    for pattern in patterns {
        if !pattern_bindings(&pattern.pattern, span)?.is_empty() {
            return Err(SourceError::semantic(
                SourceErrorCode::InvalidPattern,
                "match alternatives cannot introduce bindings",
                span.clone(),
            ));
        }
    }
    Ok((
        MatchPattern::Alternatives(
            patterns
                .iter()
                .zip(constraints)
                .map(|(pattern, constraint)| {
                    lower_case_pattern(pattern, constraint.as_ref(), &mut operands)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Vec::new(),
        operands,
    ))
}

fn lower_case_pattern(
    case_pattern: &CasePattern,
    constraint: Option<&super::semantic::Type>,
    operands: &mut Vec<PatternOperand>,
) -> Result<MatchPattern, SourceError> {
    let pattern = lower_pattern(&case_pattern.pattern, operands);
    let Some(constraint) = constraint else {
        return Ok(pattern);
    };
    Ok(MatchPattern::Constrained {
        pattern: Box::new(pattern),
        constraint: lower_match_type(constraint, operands)?,
    })
}

fn lower_match_type(
    value_type: &super::semantic::Type,
    operands: &mut Vec<PatternOperand>,
) -> Result<MatchType, SourceError> {
    use super::semantic::Type;

    match value_type {
        Type::Any => Ok(MatchType::Any),
        Type::Nil => Ok(MatchType::Nil),
        Type::Bool => Ok(MatchType::Bool),
        Type::Num => Ok(MatchType::Num),
        Type::Str => Ok(MatchType::Str),
        Type::Bytes => Ok(MatchType::Bytes),
        Type::Resource(identity) => Ok(MatchType::Resource {
            module: identity.runtime_module().into(),
            name: identity.name.clone(),
        }),
        Type::Enum(identity) => Ok(MatchType::Enum {
            module: identity.runtime_module().into(),
            name: identity.name.clone(),
        }),
        Type::List(element) => element
            .as_deref()
            .map(|element| lower_match_type(element, operands).map(Box::new))
            .transpose()
            .map(MatchType::List),
        Type::Map(entries) => entries
            .as_ref()
            .map(|(key, value)| {
                Ok((
                    Box::new(lower_match_type(key, operands)?),
                    Box::new(lower_match_type(value, operands)?),
                ))
            })
            .transpose()
            .map(MatchType::Map),
        Type::Function(None) => Ok(MatchType::Function),
        Type::Task(None) => Ok(MatchType::Task),
        Type::Channel(None) => Ok(MatchType::Channel),
        Type::Schema => Ok(MatchType::Schema),
        Type::Struct(None) => Ok(MatchType::Struct(None)),
        Type::Struct(Some(identity)) => {
            let index = operands.len();
            operands.push(PatternOperand::StructSchema(identity.name.clone()));
            Ok(MatchType::Struct(Some(index)))
        }
        Type::Union(members) => members
            .iter()
            .map(|member| lower_match_type(member, operands))
            .collect::<Result<Vec<_>, _>>()
            .map(MatchType::Union),
        Type::Never
        | Type::Unknown
        | Type::Function(Some(_))
        | Type::Task(Some(_))
        | Type::Channel(Some(_))
        | Type::Tuple(_)
        | Type::Generic(_) => Err(SourceError::semantic(
            SourceErrorCode::InvalidMatch,
            "match type constraint is not runtime-checkable",
            SourceSpan::new("<internal>", 1, 1),
        )),
    }
}

fn pattern_bindings(pattern: &Pattern, span: &SourceSpan) -> Result<Vec<String>, SourceError> {
    fn collect(pattern: &Pattern, names: &mut Vec<String>) {
        super::stack::grow(|| match pattern {
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
            Pattern::MapAll
            | Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Pinned(_)
            | Pattern::EnumCase { .. } => {}
        });
    }

    let mut names = Vec::new();
    collect(pattern, &mut names);
    let mut seen = HashSet::new();
    if let Some(name) = names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(SourceError::semantic(
            SourceErrorCode::DuplicateName,
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

/// A constant map-literal key in canonical form, so that keys Slug considers
/// equal, such as `1` and `1.0` or `0` and `-0.0`, compare equal here too.
#[derive(Eq, Hash, PartialEq)]
enum ConstantMapKey<'a> {
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> ConstantMapKey<'a> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::float_cmp,
        reason = "the range and integral-value checks make this conversion exact"
    )]
    fn new(value: &'a Value) -> Option<Self> {
        match value {
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Int(value) => Some(Self::Int(*value)),
            Value::Float(value)
                if value.is_finite()
                    && *value >= i64::MIN as f64
                    && *value < -(i64::MIN as f64)
                    && (*value as i64) as f64 == *value =>
            {
                Some(Self::Int(*value as i64))
            }
            Value::Float(value) => Some(Self::Float(value.to_bits())),
            Value::Str(value) => Some(Self::Str(value)),
            Value::Bytes(value) => Some(Self::Bytes(value)),
            _ => None,
        }
    }
}

/// Whether `expression` registers a `defer` in the block that encloses it.
///
/// A block's runtime scope exists only to collect deferred work, and a
/// `defer` belongs to its innermost enclosing block, so a block without one
/// needs no `EnterScope`/`LeaveScope`. Nested blocks decide for themselves
/// and functions run in their own frames; every other expression is searched.
#[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
fn contains_scoped_defer(expression: &Expr) -> bool {
    super::stack::grow(|| {
        let arguments = |arguments: &[CallArgument]| {
            arguments.iter().any(|argument| match argument {
                CallArgument::Positional(value)
                | CallArgument::Named { value, .. }
                | CallArgument::Spread(value) => contains_scoped_defer(value),
            })
        };
        match &expression.kind {
            ExprKind::Defer { .. } => true,
            // A nested block decides for itself; a function runs in its own frame.
            ExprKind::Block(_)
            | ExprKind::Function { .. }
            | ExprKind::Value(_)
            | ExprKind::Interpolate(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Name(_)
            | ExprKind::Foreign { .. }
            | ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. } => false,
            ExprKind::Declare {
                pattern,
                tags,
                value,
                ..
            } => {
                pattern_contains_scoped_defer(pattern)
                    || tags
                        .iter()
                        .any(|tag| tag.arguments.iter().any(contains_scoped_defer))
                    || contains_scoped_defer(value)
            }
            ExprKind::Assign { value, .. }
            | ExprKind::Return { value }
            | ExprKind::Throw { value }
            | ExprKind::Spawn(value)
            | ExprKind::Prefix { value, .. } => contains_scoped_defer(value),
            ExprKind::Recur(values) => arguments(values),
            ExprKind::Nursery { limit, body } => {
                limit.as_deref().is_some_and(contains_scoped_defer) || contains_scoped_defer(body)
            }
            ExprKind::Select(cases) => cases.iter().any(|case| {
                let operand = match &case.kind {
                    SelectCaseKind::Receive(value)
                    | SelectCaseKind::After(value)
                    | SelectCaseKind::Await(value) => contains_scoped_defer(value),
                    SelectCaseKind::Send { channel, value } => {
                        contains_scoped_defer(channel) || contains_scoped_defer(value)
                    }
                    SelectCaseKind::Default => false,
                };
                operand || case.handler.as_ref().is_some_and(contains_scoped_defer)
            }),
            ExprKind::Match { subject, cases } => {
                subject.as_deref().is_some_and(contains_scoped_defer)
                    || cases.iter().any(|case| {
                        case.patterns
                            .iter()
                            .any(|pattern| pattern_contains_scoped_defer(&pattern.pattern))
                            || case.guard.as_ref().is_some_and(contains_scoped_defer)
                            || contains_scoped_defer(&case.value)
                    })
            }
            ExprKind::Binary { left, right, .. } => {
                contains_scoped_defer(left) || contains_scoped_defer(right)
            }
            ExprKind::Call {
                callee,
                arguments: values,
            } => contains_scoped_defer(callee) || arguments(values),
            ExprKind::TypeApply { callee, .. } => contains_scoped_defer(callee),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                contains_scoped_defer(condition)
                    || contains_scoped_defer(then_branch)
                    || else_branch.as_deref().is_some_and(contains_scoped_defer)
            }
            ExprKind::List(values) => values.iter().any(|value| match value {
                ListElement::Value(value) | ListElement::Spread(value) => {
                    contains_scoped_defer(value)
                }
            }),
            ExprKind::Map(entries) => entries
                .iter()
                .any(|(key, value)| contains_scoped_defer(key) || contains_scoped_defer(value)),
            ExprKind::StructSchema(fields) => fields
                .iter()
                .filter_map(|field| field.default.as_ref())
                .any(contains_scoped_defer),
            ExprKind::StructInit { schema, fields } => {
                contains_scoped_defer(schema)
                    || fields.iter().any(|(_, value)| contains_scoped_defer(value))
            }
            ExprKind::StructCopy { value, fields } => {
                contains_scoped_defer(value)
                    || fields
                        .iter()
                        .any(|(_, replacement)| contains_scoped_defer(replacement))
            }
            ExprKind::Index { collection, index } => {
                contains_scoped_defer(collection) || contains_scoped_defer(index)
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                contains_scoped_defer(collection)
                    || [start, end, step]
                        .into_iter()
                        .any(|bound| bound.as_deref().is_some_and(contains_scoped_defer))
            }
        }
    })
}

fn pattern_contains_scoped_defer(pattern: &Pattern) -> bool {
    super::stack::grow(|| match pattern {
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Binding(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => false,
        Pattern::At { pattern, .. } => pattern_contains_scoped_defer(pattern),
        Pattern::List { items, .. } => items.iter().any(pattern_contains_scoped_defer),
        Pattern::Map { entries, .. } => entries.iter().any(|(key, pattern)| {
            matches!(key, MapPatternKey::Computed(key) if contains_scoped_defer(key))
                || pattern_contains_scoped_defer(pattern)
        }),
    })
}
