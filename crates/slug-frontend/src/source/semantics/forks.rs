//! Reports what a `fork` cannot do that the source shows directly.
//!
//! A fork sees a snapshot of its captures and the module variables, so an
//! assignment inside it to a binding declared outside it could never reach
//! the code that declared it: it is an error, unless the binding is a
//! module `task var`, which every task owns a copy of. A fork in a module's
//! top-level code is an error too: it would start while the module
//! initializes, and could see the module's values set after it started.
//!
//! Names are resolved lexically, with a block's declarations visible
//! throughout it. Only what is written inside the fork is checked here; a
//! write reached through a call fails when it runs.

use std::collections::HashMap;

use slug_vm::SourceSpan;

use super::{
    SourceError, SourceErrorCode,
    ast::{
        CallArgument, Expr, ExprKind, ListElement, MapPatternKey, Pattern, RestPattern,
        SelectCaseKind,
    },
};

/// Returns an error for the first write inside a fork to a binding declared
/// outside it, or the first fork in module top-level code.
///
/// An interactive session's entries are top-level code too, but each runs
/// after the earlier ones have finished, so a fork typed at the prompt
/// cannot see a value set after it starts: `interactive` allows it.
pub(super) fn check_forks(expressions: &[Expr], interactive: bool) -> Result<(), SourceError> {
    let mut analysis = Analysis {
        interactive,
        ..Analysis::default()
    };
    analysis.scopes.push(HashMap::new());
    analysis.predeclare(expressions);
    for expression in expressions {
        analysis.walk(expression)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Binding {
    /// A module-level `task var`.
    task: bool,
    /// Declared at module level.
    module: bool,
    /// How many forks enclose the declaration.
    forks: usize,
}

#[derive(Default)]
struct Analysis {
    scopes: Vec<HashMap<String, Binding>>,
    /// The forks being walked, outermost first.
    forks: Vec<SourceSpan>,
    /// How many function bodies enclose the expression being walked; zero
    /// is module top-level code.
    functions: usize,
    /// The expressions are an interactive session's entry.
    interactive: bool,
}

impl Analysis {
    fn resolve(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn declare(&mut self, name: &str, task: bool) {
        let binding = Binding {
            task,
            module: self.scopes.len() == 1,
            forks: self.forks.len(),
        };
        self.scopes
            .last_mut()
            .expect("an analysis always has a scope")
            .insert(name.into(), binding);
    }

    /// Declares every name the statements of a block or module declare, so a
    /// use anywhere in it resolves to them.
    fn predeclare(&mut self, values: &[Expr]) {
        for value in values {
            match &value.kind {
                ExprKind::Declare { task, pattern, .. } => {
                    let mut names = Vec::new();
                    pattern_names(pattern, &mut names);
                    for name in names {
                        self.declare(name, *task);
                    }
                }
                ExprKind::Foreign { name, .. }
                | ExprKind::Resource { name, .. }
                | ExprKind::Enum { name, .. } => self.declare(name, false),
                _ => {}
            }
        }
    }

    fn walk_scoped(&mut self, bindings: &[&str], expressions: &[&Expr]) -> Result<(), SourceError> {
        super::stack::grow(|| {
            self.scopes.push(HashMap::new());
            for name in bindings {
                self.declare(name, false);
            }
            let result = expressions
                .iter()
                .try_for_each(|expression| self.walk(expression));
            self.scopes.pop();
            result
        })
    }

    fn walk_arguments(&mut self, arguments: &[CallArgument]) -> Result<(), SourceError> {
        arguments.iter().try_for_each(|argument| match argument {
            CallArgument::Positional(value)
            | CallArgument::Named { value, .. }
            | CallArgument::Spread(value) => self.walk(value),
        })
    }

    fn walk_pattern(&mut self, pattern: &Pattern) -> Result<(), SourceError> {
        super::stack::grow(|| match pattern {
            Pattern::At { pattern, .. } => self.walk_pattern(pattern),
            Pattern::List { items, .. } => {
                items.iter().try_for_each(|item| self.walk_pattern(item))
            }
            Pattern::Map { entries, .. } => entries.iter().try_for_each(|(key, value)| {
                if let MapPatternKey::Computed(key) = key {
                    self.walk(key)?;
                }
                self.walk_pattern(value)
            }),
            Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Binding(_)
            | Pattern::Pinned(_)
            | Pattern::MapAll
            | Pattern::EnumCase { .. } => Ok(()),
        })
    }

    fn assign(&self, name: &str, expression: &Expr) -> Result<(), SourceError> {
        let Some(fork) = self.forks.last() else {
            return Ok(());
        };
        let Some(binding) = self.resolve(name) else {
            return Ok(());
        };
        if binding.forks >= self.forks.len() || (binding.module && binding.task) {
            return Ok(());
        }
        let message = if binding.module {
            format!(
                "a fork cannot change `{name}`, a module `var`: the fork at {}:{} sees a snapshot of it, so the change would never reach the rest of the program; return the value from the fork, or declare it `task var {name}` if each task's copy is its own",
                fork.line, fork.column
            )
        } else {
            format!(
                "a fork cannot change `{name}`, which is declared outside it: the fork at {}:{} sees a snapshot of `{name}`, so the change would never reach the code that declared it; return the value from the fork instead",
                fork.line, fork.column
            )
        };
        Err(
            SourceError::semantic(SourceErrorCode::ForkWrite, message, expression.span.clone())
                .with_extent(expression),
        )
    }

    #[allow(clippy::too_many_lines)]
    fn walk(&mut self, expression: &Expr) -> Result<(), SourceError> {
        super::stack::grow(|| match &expression.kind {
            ExprKind::Declare { pattern, value, .. } => {
                self.walk_pattern(pattern)?;
                self.walk(value)
            }
            ExprKind::Assign { name, value } => {
                self.walk(value)?;
                self.assign(name, expression)
            }
            ExprKind::Function {
                parameters, body, ..
            } => {
                self.functions += 1;
                self.scopes.push(HashMap::new());
                for parameter in parameters {
                    self.declare(&parameter.name, false);
                }
                let result = parameters
                    .iter()
                    .filter_map(|parameter| parameter.default.as_ref())
                    .try_for_each(|default| self.walk(default))
                    .and_then(|()| self.walk(body));
                self.scopes.pop();
                self.functions -= 1;
                result
            }
            ExprKind::Spawn { body, fork } => {
                if !fork {
                    return self.walk(body);
                }
                if self.functions == 0 && !self.interactive {
                    return Err(SourceError::semantic(
                        SourceErrorCode::ForkDuringInitialization,
                        "a fork cannot start while a module initializes: it could see the module's values set after it started; start it from a function, such as `main`",
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                self.forks.push(expression.span.clone());
                let result = self.walk(body);
                self.forks.pop();
                result
            }
            ExprKind::Call { callee, arguments } => {
                self.walk(callee)?;
                self.walk_arguments(arguments)
            }
            ExprKind::Recur(arguments) => self.walk_arguments(arguments),
            ExprKind::Block(values) => {
                self.scopes.push(HashMap::new());
                self.predeclare(values);
                let result = values.iter().try_for_each(|value| self.walk(value));
                self.scopes.pop();
                result
            }
            ExprKind::Match { subject, cases } => {
                if let Some(subject) = subject {
                    self.walk(subject)?;
                }
                for case in cases {
                    let mut names = Vec::new();
                    for pattern in &case.patterns {
                        self.walk_pattern(&pattern.pattern)?;
                        pattern_names(&pattern.pattern, &mut names);
                    }
                    let mut values = case.guard.iter().collect::<Vec<_>>();
                    values.push(&case.value);
                    self.walk_scoped(&names, &values)?;
                }
                Ok(())
            }
            ExprKind::Defer {
                value, error_name, ..
            } => {
                let names = error_name.iter().map(String::as_str).collect::<Vec<_>>();
                self.walk_scoped(&names, &[value])
            }
            ExprKind::Select(cases) => cases.iter().try_for_each(|case| {
                match &case.kind {
                    SelectCaseKind::Receive(value)
                    | SelectCaseKind::After(value)
                    | SelectCaseKind::Await(value) => self.walk(value)?,
                    SelectCaseKind::Send { channel, value } => {
                        self.walk(channel)?;
                        self.walk(value)?;
                    }
                    SelectCaseKind::Default => {}
                }
                case.handler
                    .as_ref()
                    .map_or(Ok(()), |handler| self.walk(handler))
            }),
            ExprKind::Return { value }
            | ExprKind::Throw { value }
            | ExprKind::Prefix { value, .. }
            | ExprKind::TypeApply { callee: value, .. } => self.walk(value),
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    self.walk(limit)?;
                }
                self.walk(body)
            }
            ExprKind::Binary { left, right, .. } => {
                self.walk(left)?;
                self.walk(right)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk(condition)?;
                self.walk(then_branch)?;
                else_branch
                    .as_ref()
                    .map_or(Ok(()), |branch| self.walk(branch))
            }
            ExprKind::List(values) => values.iter().try_for_each(|value| match value {
                ListElement::Value(value) | ListElement::Spread(value) => self.walk(value),
            }),
            ExprKind::Map(entries) => entries.iter().try_for_each(|(key, value)| {
                self.walk(key)?;
                self.walk(value)
            }),
            ExprKind::StructSchema(fields) => fields
                .iter()
                .filter_map(|field| field.default.as_ref())
                .try_for_each(|default| self.walk(default)),
            ExprKind::StructInit { schema, fields } => {
                self.walk(schema)?;
                fields.iter().try_for_each(|(_, value)| self.walk(value))
            }
            ExprKind::StructCopy { value, fields } => {
                self.walk(value)?;
                fields
                    .iter()
                    .try_for_each(|(_, replacement)| self.walk(replacement))
            }
            ExprKind::Index { collection, index } => {
                self.walk(collection)?;
                self.walk(index)
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                self.walk(collection)?;
                [start, end, step]
                    .into_iter()
                    .flatten()
                    .try_for_each(|bound| self.walk(bound))
            }
            ExprKind::Name(_)
            | ExprKind::Interpolate(_)
            | ExprKind::Value(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Foreign { .. }
            | ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. } => Ok(()),
        })
    }
}

fn pattern_names<'a>(pattern: &'a Pattern, names: &mut Vec<&'a str>) {
    super::stack::grow(|| match pattern {
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
    });
}
