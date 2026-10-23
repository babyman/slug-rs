//! Finds assignments that discard a change made by a call inside them.
//!
//! In `output = output :+ render(evaluate(x))`, `output` is read before
//! `evaluate` runs. When `evaluate`, or a function it calls, assigns
//! `output`, the assignment then overwrites that change with a value built
//! from the earlier read. Lists, maps, and strings are immutable, so code that
//! accumulates into a `var` this way loses the inner updates silently.
//!
//! Names are resolved lexically, with a block's declarations visible
//! throughout it. Only calls of a function declared with `val name = fn ...`
//! are followed; a call through any other value, or a function passed on and
//! called elsewhere, is not, so the check reports only updates it can show are
//! lost. A call changes the same `var` only through functions nested in the
//! one that declares it: a path that leaves that function reaches the `var`
//! again only by calling it, which makes a new one.

use std::collections::{HashMap, HashSet};

use slug_vm::{SourcePosition, SourceSpan};

use super::{
    SourceError, SourceErrorCode,
    ast::{
        CallArgument, Expr, ExprKind, ListElement, MapPatternKey, Parameter, Pattern, RestPattern,
        SelectCaseKind, StringPart,
    },
};

/// Returns an error for the first assignment that discards a change a call
/// inside it makes to the assigned `var`.
pub(super) fn check_lost_updates(expressions: &[Expr]) -> Result<(), SourceError> {
    let mut analysis = Analysis::default();
    analysis.scopes.push(HashMap::new());
    analysis.predeclare(expressions);
    for expression in expressions {
        analysis.walk(expression);
    }
    analysis.report()
}

#[derive(Clone, Copy)]
enum Symbol {
    Var(usize),
    Function(usize),
    Other,
}

/// What one function's body does directly.
#[derive(Default)]
struct FunctionFacts {
    assigns: HashSet<usize>,
    calls: HashSet<usize>,
    /// The functions it is nested in, outermost first.
    enclosing: Vec<usize>,
}

/// An assignment to a `var`, with the reads of that `var` and the calls of
/// named functions its value makes.
struct Assignment<'a> {
    name: &'a str,
    var: usize,
    reads: Vec<&'a SourceSpan>,
    calls: Vec<(usize, &'a str, &'a Expr)>,
}

#[derive(Default)]
struct Analysis<'a> {
    scopes: Vec<HashMap<String, Symbol>>,
    /// Per `var`, the function that declares it, or `None` at module level.
    vars: Vec<Option<usize>>,
    functions: Vec<FunctionFacts>,
    /// The functions whose bodies are being walked, outermost first.
    enclosing: Vec<usize>,
    /// Functions declared by name but not yet walked, by the declaration's
    /// value, so walking it uses the predeclared function.
    pending: HashMap<*const Expr, usize>,
    assignments: Vec<Assignment<'a>>,
    /// The assignments whose values are being walked, innermost last. A
    /// nested function literal does not run with them, so it hides them.
    active: Vec<Vec<usize>>,
}

impl<'a> Analysis<'a> {
    fn resolve(&self, name: &str) -> Symbol {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(Symbol::Other)
    }

    fn declare(&mut self, name: &str, symbol: Symbol) {
        self.scopes
            .last_mut()
            .expect("an analysis always has a scope")
            .insert(name.into(), symbol);
    }

    /// Declares every name the statements of a block or module declare, so a
    /// use anywhere in it resolves to them.
    fn predeclare(&mut self, values: &'a [Expr]) {
        for value in values {
            match &value.kind {
                ExprKind::Declare {
                    mutable,
                    pattern,
                    value: initial,
                    ..
                } => {
                    if let (false, Pattern::Binding(name), ExprKind::Function { .. }) =
                        (mutable, pattern, &initial.kind)
                    {
                        let function = self.new_function();
                        self.pending
                            .insert(std::ptr::from_ref(initial.as_ref()), function);
                        self.declare(name, Symbol::Function(function));
                        continue;
                    }
                    let mut names = Vec::new();
                    pattern_names(pattern, &mut names);
                    for name in names {
                        let symbol = if *mutable {
                            self.vars.push(self.enclosing.last().copied());
                            Symbol::Var(self.vars.len() - 1)
                        } else {
                            Symbol::Other
                        };
                        self.declare(name, symbol);
                    }
                }
                ExprKind::Foreign { name, .. }
                | ExprKind::Resource { name, .. }
                | ExprKind::Enum { name, .. } => self.declare(name, Symbol::Other),
                _ => {}
            }
        }
    }

    fn new_function(&mut self) -> usize {
        self.functions.push(FunctionFacts {
            enclosing: self.enclosing.clone(),
            ..FunctionFacts::default()
        });
        self.functions.len() - 1
    }

    fn current(&self) -> Option<usize> {
        self.enclosing.last().copied()
    }

    fn read(&mut self, name: &str, span: &'a SourceSpan) {
        let Symbol::Var(var) = self.resolve(name) else {
            return;
        };
        for &index in self.active.last().into_iter().flatten() {
            if self.assignments[index].var == var {
                self.assignments[index].reads.push(span);
            }
        }
    }

    fn call(&mut self, function: usize, name: &'a str, call: &'a Expr) {
        if let Some(current) = self.current() {
            self.functions[current].calls.insert(function);
        }
        for &index in self.active.last().into_iter().flatten() {
            self.assignments[index].calls.push((function, name, call));
        }
    }

    fn walk_scoped(&mut self, bindings: &[&'a str], expressions: &[&'a Expr]) {
        self.scopes.push(HashMap::new());
        for name in bindings {
            self.declare(name, Symbol::Other);
        }
        for expression in expressions {
            self.walk(expression);
        }
        self.scopes.pop();
    }

    fn walk_function(&mut self, function: usize, parameters: &'a [Parameter], body: &'a Expr) {
        self.enclosing.push(function);
        self.active.push(Vec::new());
        self.scopes.push(HashMap::new());
        for parameter in parameters {
            self.declare(&parameter.name, Symbol::Other);
        }
        for parameter in parameters {
            if let Some(default) = &parameter.default {
                self.walk(default);
            }
        }
        self.walk(body);
        self.scopes.pop();
        self.active.pop();
        self.enclosing.pop();
    }

    fn walk_arguments(&mut self, arguments: &'a [CallArgument]) {
        for argument in arguments {
            match argument {
                CallArgument::Positional(value)
                | CallArgument::Named { value, .. }
                | CallArgument::Spread(value) => self.walk(value),
            }
        }
    }

    fn walk_pattern(&mut self, pattern: &'a Pattern) {
        match pattern {
            Pattern::At { pattern, .. } => self.walk_pattern(pattern),
            Pattern::List { items, .. } => {
                for item in items {
                    self.walk_pattern(item);
                }
            }
            Pattern::Map { entries, .. } => {
                for (key, value) in entries {
                    if let MapPatternKey::Computed(key) = key {
                        self.walk(key);
                    }
                    self.walk_pattern(value);
                }
            }
            Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Binding(_)
            | Pattern::Pinned(_)
            | Pattern::MapAll
            | Pattern::EnumCase { .. } => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn walk(&mut self, expression: &'a Expr) {
        match &expression.kind {
            ExprKind::Name(name) => self.read(name, &expression.span),
            ExprKind::Interpolate(parts) => {
                for part in parts {
                    if let StringPart::Name(name) = part {
                        self.read(name, &expression.span);
                    }
                }
            }
            ExprKind::Declare { pattern, value, .. } => {
                self.walk_pattern(pattern);
                if let (
                    ExprKind::Function {
                        parameters, body, ..
                    },
                    Some(function),
                ) = (
                    &value.kind,
                    self.pending.remove(&std::ptr::from_ref(value.as_ref())),
                ) {
                    self.walk_function(function, parameters, body);
                } else {
                    self.walk(value);
                }
            }
            ExprKind::Assign { name, value } => {
                let Symbol::Var(var) = self.resolve(name) else {
                    self.walk(value);
                    return;
                };
                if let Some(current) = self.current() {
                    self.functions[current].assigns.insert(var);
                }
                self.assignments.push(Assignment {
                    name,
                    var,
                    reads: Vec::new(),
                    calls: Vec::new(),
                });
                let index = self.assignments.len() - 1;
                if self.active.is_empty() {
                    self.active.push(Vec::new());
                }
                self.active
                    .last_mut()
                    .expect("an active list exists")
                    .push(index);
                self.walk(value);
                self.active
                    .last_mut()
                    .expect("an active list exists")
                    .retain(|active| *active != index);
            }
            ExprKind::Function {
                parameters, body, ..
            } => {
                let function = self.new_function();
                self.walk_function(function, parameters, body);
            }
            ExprKind::Call { callee, arguments } => {
                self.walk(callee);
                self.walk_arguments(arguments);
                if let ExprKind::Name(name) = &callee.kind
                    && let Symbol::Function(function) = self.resolve(name)
                {
                    self.call(function, name, expression);
                }
            }
            ExprKind::Recur(arguments) => {
                self.walk_arguments(arguments);
                if let Some(current) = self.current() {
                    self.call(current, "recur", expression);
                }
            }
            ExprKind::Block(values) => {
                self.scopes.push(HashMap::new());
                self.predeclare(values);
                for value in values {
                    self.walk(value);
                }
                self.scopes.pop();
            }
            ExprKind::Match { subject, cases } => {
                if let Some(subject) = subject {
                    self.walk(subject);
                }
                for case in cases {
                    let mut names = Vec::new();
                    for pattern in &case.patterns {
                        self.walk_pattern(&pattern.pattern);
                        pattern_names(&pattern.pattern, &mut names);
                    }
                    let mut values = case.guard.iter().collect::<Vec<_>>();
                    values.push(&case.value);
                    self.walk_scoped(&names, &values);
                }
            }
            ExprKind::Defer {
                value, error_name, ..
            } => {
                let names = error_name.iter().map(String::as_str).collect::<Vec<_>>();
                self.walk_scoped(&names, &[value]);
            }
            ExprKind::Select(cases) => {
                for case in cases {
                    match &case.kind {
                        SelectCaseKind::Receive(value)
                        | SelectCaseKind::After(value)
                        | SelectCaseKind::Await(value) => self.walk(value),
                        SelectCaseKind::Send { channel, value } => {
                            self.walk(channel);
                            self.walk(value);
                        }
                        SelectCaseKind::Default => {}
                    }
                    if let Some(handler) = &case.handler {
                        self.walk(handler);
                    }
                }
            }
            ExprKind::Return { value }
            | ExprKind::Throw { value }
            | ExprKind::Spawn(value)
            | ExprKind::Prefix { value, .. }
            | ExprKind::TypeApply { callee: value, .. } => self.walk(value),
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    self.walk(limit);
                }
                self.walk(body);
            }
            ExprKind::Binary { left, right, .. } => {
                self.walk(left);
                self.walk(right);
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk(condition);
                self.walk(then_branch);
                if let Some(else_branch) = else_branch {
                    self.walk(else_branch);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    match value {
                        ListElement::Value(value) | ListElement::Spread(value) => self.walk(value),
                    }
                }
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.walk(key);
                    self.walk(value);
                }
            }
            ExprKind::StructSchema(fields) => {
                for field in fields {
                    if let Some(default) = &field.default {
                        self.walk(default);
                    }
                }
            }
            ExprKind::StructInit { schema, fields } => {
                self.walk(schema);
                for (_, value) in fields {
                    self.walk(value);
                }
            }
            ExprKind::StructCopy { value, fields } => {
                self.walk(value);
                for (_, replacement) in fields {
                    self.walk(replacement);
                }
            }
            ExprKind::Index { collection, index } => {
                self.walk(collection);
                self.walk(index);
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                self.walk(collection);
                for bound in [start, end, step].into_iter().flatten() {
                    self.walk(bound);
                }
            }
            ExprKind::Value(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Foreign { .. }
            | ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. } => {}
        }
    }

    /// Whether calling `function` can assign the instance of `var` that the
    /// caller sees: through a path of functions nested in the one that
    /// declares it, one of which assigns it.
    fn reaches(&self, function: usize, var: usize, visited: &mut HashSet<usize>) -> bool {
        if !visited.insert(function) {
            return false;
        }
        let facts = &self.functions[function];
        if let Some(declaring) = self.vars[var]
            && !facts.enclosing.contains(&declaring)
        {
            return false;
        }
        facts.assigns.contains(&var)
            || facts
                .calls
                .iter()
                .any(|&callee| self.reaches(callee, var, visited))
    }

    fn report(&self) -> Result<(), SourceError> {
        for assignment in &self.assignments {
            for &(function, callee, call) in &assignment.calls {
                if !self.reaches(function, assignment.var, &mut HashSet::new()) {
                    continue;
                }
                let Some(read) = assignment
                    .reads
                    .iter()
                    .find(|read| (read.line, read.column) < (call.end.line, call.end.column))
                else {
                    continue;
                };
                let name = assignment.name;
                return Err(SourceError::semantic(
                    SourceErrorCode::LostUpdate,
                    format!(
                        "this assignment to `{name}` discards what the call to `{callee}` at {}:{} assigns to it: `{name}` is read here, before that call runs; bind the call's result first, as in `val value = {callee}(...)`, then read `{name}`",
                        call.span.line, call.span.column
                    ),
                    (*read).clone(),
                )
                .with_end(SourcePosition {
                    line: read.line,
                    column: read.column + u32::try_from(name.chars().count()).unwrap_or(0),
                }));
            }
        }
        Ok(())
    }
}

fn pattern_names<'a>(pattern: &'a Pattern, names: &mut Vec<&'a str>) {
    match pattern {
        Pattern::Binding(name) => names.push(name),
        Pattern::At { name, pattern } => {
            names.push(name);
            pattern_names(pattern, names);
        }
        Pattern::List { items, rest } => {
            for item in items {
                pattern_names(item, names);
            }
            if let Some(RestPattern::Binding(name)) = rest {
                names.push(name);
            }
        }
        Pattern::Map { entries, rest, .. } => {
            for (_, value) in entries {
                pattern_names(value, names);
            }
            if let Some(RestPattern::Binding(name)) = rest {
                names.push(name);
            }
        }
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => {}
    }
}
