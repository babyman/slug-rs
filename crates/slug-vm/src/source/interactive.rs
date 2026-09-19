//! Interactive source readiness and per-cell compilation.
//!
//! This module owns accumulated source classification and compiler snapshots.
//! It preserves the resolver-backed, commit-after-success behavior used by the
//! interactive server.

use std::collections::{HashMap, HashSet};

use crate::Program;

use super::{
    SourceError,
    compiler::Compiler,
    environment::{ModuleSnapshot, SessionSnapshot},
    lexer::Lexer,
    parser::Parser,
    typecheck,
};

/// Compiler state retained by an interactive source session.
#[derive(Clone, Debug, Default)]
#[doc(hidden)]
pub struct InteractiveCompilerState {
    semantic: SessionSnapshot,
    globals: HashMap<String, bool>,
    callable_globals: HashSet<String>,
}

#[doc(hidden)]
pub struct InteractiveCompilation {
    pub program: Program,
    pub state: InteractiveCompilerState,
}

/// Syntax readiness of source accumulated by an interactive session.
#[doc(hidden)]
pub enum SourceReadiness {
    Complete,
    Incomplete,
    Invalid(SourceError),
}

#[doc(hidden)]
#[must_use]
pub fn source_readiness(path: &str, source: &str) -> SourceReadiness {
    let tokens = match Lexer::new(path, source).tokens() {
        Ok(tokens) => tokens,
        Err(error) => {
            return if incomplete_lexer_error(&error, source) {
                SourceReadiness::Incomplete
            } else {
                SourceReadiness::Invalid(error)
            };
        }
    };
    let Some(end) = tokens.last().map(|token| token.span.clone()) else {
        return SourceReadiness::Complete;
    };
    match Parser::new(tokens).parse() {
        Ok(_) => SourceReadiness::Complete,
        Err(error) => {
            if error.message != "expected binding name"
                && error.span.as_ref().is_some_and(|span| span == &end)
            {
                SourceReadiness::Incomplete
            } else {
                SourceReadiness::Invalid(error)
            }
        }
    }
}

fn incomplete_lexer_error(error: &SourceError, source: &str) -> bool {
    matches!(
        error.message.as_str(),
        "unterminated block comment"
            | "unterminated byte literal"
            | "unterminated documentation block"
            | "unterminated string"
    ) || (error.message == "expected . after .." && source.ends_with(".."))
        || (error.message == "expected ???" && source.ends_with('?'))
        || (error.message == "expected exponent digit" && source.ends_with(['e', 'E', '+', '-']))
        || (error.message == "expected hexadecimal digit"
            && (source.ends_with("0x") || source.ends_with("0x_")))
}

/// Compiles each complete top-level interactive form as an independent cell.
///
/// Each returned compilation is analysed against the state produced by the
/// preceding form. This lets an interactive host commit completed forms before
/// retaining a later form that suspends.
#[doc(hidden)]
pub fn compile_interactive_forms(
    path: &str,
    source: &str,
    state: &InteractiveCompilerState,
) -> Result<Vec<InteractiveCompilation>, SourceError> {
    compile_interactive_forms_with_resolver(path, source, state, |_| None)
}

pub(crate) fn compile_interactive_forms_with_resolver(
    path: &str,
    source: &str,
    state: &InteractiveCompilerState,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<Vec<InteractiveCompilation>, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let mut state = state.clone();
    let mut compilations = Vec::with_capacity(expressions.len());
    for expression in expressions {
        let imports = typecheck::static_import_names(std::slice::from_ref(&expression))
            .into_iter()
            .filter_map(|name| resolve(&name).map(|snapshot| (name, snapshot)))
            .collect::<HashMap<_, _>>();
        let mut imports = imports;
        if let std::collections::hash_map::Entry::Vacant(entry) =
            imports.entry("slug.builtin".into())
            && let Some(snapshot) = resolve("slug.builtin")
        {
            entry.insert(snapshot);
        }
        let expressions = vec![expression];
        let analysis =
            typecheck::analyze_with_imports_and_session(&expressions, imports, &state.semantic)?;
        let compiled = Compiler::with_globals(
            path,
            expressions,
            &analysis,
            state.globals.clone(),
            state.callable_globals.clone(),
        )
        .compile()?;
        let mut program = compiled.program;
        program.set_semantic_snapshot(analysis.snapshot);
        state = InteractiveCompilerState {
            semantic: analysis.session_snapshot,
            globals: compiled.globals,
            callable_globals: compiled.callable_globals,
        };
        compilations.push(InteractiveCompilation {
            program,
            state: state.clone(),
        });
    }
    Ok(compilations)
}
