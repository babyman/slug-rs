use std::collections::{HashMap, HashSet};

use crate::{Capture, Chunk, MatchPattern, MatchRest, Op, Program, SourceSpan};

use super::{
    SourceError,
    ast::{Binary, Expr, ExprKind, MatchCase, Pattern, Prefix, RestPattern},
    state::{Binding, State},
};

pub(super) struct Compiler {
    path: String,
    expressions: Vec<Expr>,
    chunks: Vec<Chunk>,
    globals: HashMap<String, bool>,
}
impl Compiler {
    pub(super) fn new(path: &str, expressions: Vec<Expr>) -> Self {
        Self {
            path: path.into(),
            expressions,
            chunks: Vec::new(),
            globals: HashMap::new(),
        }
    }
    pub(super) fn compile(mut self) -> Result<Program, SourceError> {
        for expression in &self.expressions {
            if let ExprKind::Declare {
                mutable, pattern, ..
            } = &expression.kind
            {
                for name in pattern_bindings(pattern, &expression.span)? {
                    self.globals.insert(name, *mutable);
                }
            }
        }
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
        Ok(program)
    }
    #[allow(clippy::too_many_lines)]
    fn expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        match &expression.kind {
            ExprKind::Value(value) => {
                let constant = state.constant(value.clone());
                state.emit(Op::Constant(constant), &expression.span);
            }
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
            },
            ExprKind::Declare {
                mutable,
                pattern,
                value,
            } => {
                self.expression(state, value)?;
                Self::bind_pattern(state, pattern, *mutable, &expression.span)?;
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
                    | Binding::Capture { mutable, .. } => mutable,
                };
                if !mutable {
                    return Err(SourceError::semantic(
                        format!("cannot assign to immutable binding `{name}`"),
                        expression.span.clone(),
                    ));
                }
                self.expression(state, value)?;
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
                }
                state.emit(Op::Nil, &expression.span);
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
                        parameters: error_name.iter().cloned().collect(),
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
            ExprKind::Match { subject, cases } => {
                self.compile_match(state, subject, cases, false)?;
            }
            ExprKind::Binary {
                left,
                operator,
                right,
            } => {
                self.expression(state, left)?;
                match operator {
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
                        state.emit(Op::Less, &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::LessEqual => {
                        self.expression(state, right)?;
                        state.emit(Op::Greater, &expression.span);
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
                    Binary::Equal => {
                        self.expression(state, right)?;
                        state.emit(Op::Equal, &expression.span);
                    }
                    Binary::Greater => {
                        self.expression(state, right)?;
                        state.emit(Op::Greater, &expression.span);
                    }
                    Binary::Less => {
                        self.expression(state, right)?;
                        state.emit(Op::Less, &expression.span);
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
                        },
                        span,
                    );
                }
            }
            ExprKind::Call { callee, arguments } => {
                self.expression(state, callee)?;
                for argument in arguments {
                    self.expression(state, argument)?;
                }
                state.emit(Op::Call(arguments.len()), &expression.span);
            }
            ExprKind::List(values) => {
                for value in values {
                    self.expression(state, value)?;
                }
                state.emit(Op::List(values.len()), &expression.span);
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.expression(state, key)?;
                    self.expression(state, value)?;
                }
                state.emit(Op::Map(entries.len()), &expression.span);
            }
            ExprKind::Index { collection, index } => {
                self.expression(state, collection)?;
                self.expression(state, index)?;
                state.emit(Op::GetIndex, &expression.span);
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
            ExprKind::Function { parameters, body } => {
                let (mut chunk, captures) = self.function(parameters, body, state.visible())?;
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
            let (pattern, names) = lower_case_patterns(&case.patterns, &case.span)?;
            state.emit(Op::Duplicate, &case.span);
            state.emit(
                Op::TryMatch {
                    pattern,
                    bindings: names.len(),
                },
                &case.span,
            );
            state.enter_scope();
            let slots = names
                .into_iter()
                .map(|name| state.declare(name, false))
                .collect::<Vec<_>>();
            let next = state.jump_if_false(&case.span);
            state.emit(Op::Pop, &case.span);
            for slot in slots.iter().rev() {
                state.emit(Op::SetLocal(*slot), &case.span);
            }
            state.emit(Op::Pop, &case.span);
            let guard_next = if let Some(guard) = &case.guard {
                self.expression(state, guard)?;
                Some(state.jump_if_false(&case.span))
            } else {
                None
            };
            if guard_next.is_some() {
                state.emit(Op::Pop, &case.span);
            }
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
    fn bind_pattern(
        state: &mut State,
        pattern: &Pattern,
        mutable: bool,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        let names = pattern_bindings(pattern, span)?;
        state.emit(
            Op::TryMatch {
                pattern: lower_pattern(pattern),
                bindings: names.len(),
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
                        slot: state.declare(name.clone(), mutable),
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
                if arguments.len() != state.arity() {
                    return Err(SourceError::semantic(
                        format!(
                            "recur expects {} arguments, got {}",
                            state.arity(),
                            arguments.len()
                        ),
                        expression.span.clone(),
                    ));
                }
                for argument in arguments {
                    self.expression(state, argument)?;
                }
                state.emit(Op::Recur(arguments.len()), &expression.span);
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
                self.compile_match(state, subject, cases, true)?;
            }
            _ => self.expression(state, expression)?,
        }
        Ok(())
    }
    fn function(
        &mut self,
        parameters: &[String],
        body: &Expr,
        visible: HashMap<String, Binding>,
    ) -> Result<(Chunk, Vec<Capture>), SourceError> {
        let mut state = State::function(parameters, visible);
        self.tail_expression(&mut state, body)?;
        state.emit(Op::Return, &body.span);
        let captures = state.captures();
        Ok((state.finish("<fn>", parameters.len()), captures))
    }
}

fn lower_pattern(pattern: &Pattern) -> MatchPattern {
    match pattern {
        Pattern::Literal(value) => MatchPattern::Literal(value.clone()),
        Pattern::Wildcard => MatchPattern::Wildcard,
        Pattern::Binding(_) => MatchPattern::Binding,
        Pattern::At { pattern, .. } => MatchPattern::At(Box::new(lower_pattern(pattern))),
        Pattern::List { items, rest } => MatchPattern::List {
            items: items.iter().map(lower_pattern).collect(),
            rest: lower_rest_pattern(rest.as_ref()),
        },
        Pattern::Map {
            entries,
            rest,
            exact,
        } => MatchPattern::Map {
            entries: entries
                .iter()
                .map(|(key, pattern)| (key.clone(), lower_pattern(pattern)))
                .collect(),
            rest: lower_rest_pattern(rest.as_ref()),
            exact: *exact,
        },
    }
}

fn lower_case_patterns(
    patterns: &[Pattern],
    span: &SourceSpan,
) -> Result<(MatchPattern, Vec<String>), SourceError> {
    if let [pattern] = patterns {
        return Ok((lower_pattern(pattern), pattern_bindings(pattern, span)?));
    }

    for pattern in patterns {
        if !pattern_bindings(pattern, span)?.is_empty() {
            return Err(SourceError::semantic(
                "match alternatives cannot introduce bindings",
                span.clone(),
            ));
        }
    }
    Ok((
        MatchPattern::Alternatives(patterns.iter().map(lower_pattern).collect()),
        Vec::new(),
    ))
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
            Pattern::Literal(_) | Pattern::Wildcard => {}
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
