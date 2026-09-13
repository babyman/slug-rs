//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use crate::{Program, SourceSpan};

mod ast;
mod compiler;
pub(crate) mod environment;
mod lexer;
mod parser;
mod semantic;
mod state;
mod typecheck;
use compiler::Compiler;
use lexer::Lexer;
use parser::Parser;

use self::environment::{ImportSnapshots, ModuleSnapshot, SessionSnapshot};

/// Compiler state retained by an interactive source session.
#[derive(Clone, Debug, Default)]
pub(crate) struct InteractiveCompilerState {
    semantic: SessionSnapshot,
    globals: HashMap<String, bool>,
    callable_globals: HashSet<String>,
}

pub(crate) struct InteractiveCompilation {
    pub(crate) program: Program,
    pub(crate) state: InteractiveCompilerState,
}

/// Syntax readiness of source accumulated by an interactive session.
pub(crate) enum SourceReadiness {
    Complete,
    Incomplete,
    Invalid(SourceError),
}

#[derive(Clone, Debug)]
pub struct SourceError {
    pub kind: SourceErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceErrorKind {
    Parse,
    Semantic,
}

impl SourceError {
    fn at(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Parse,
            message: message.into(),
            span: Some(span),
        }
    }

    fn semantic(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Semantic,
            message: message.into(),
            span: Some(span),
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at {}:{}:{}", span.path, span.line, span.column)?;
        }
        Ok(())
    }
}
impl std::error::Error for SourceError {}

/// Compiles the currently supported core Slug source subset into a VM program.
///
/// # Errors
/// Returns a source error with a source location for invalid syntax or source
/// semantics, including directly provable type mismatches.
pub fn compile(path: &str, source: &str) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    compile_expressions(path, expressions, ImportSnapshots::new())
}

pub(crate) fn source_readiness(path: &str, source: &str) -> SourceReadiness {
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

pub(crate) fn compile_interactive(
    path: &str,
    source: &str,
    state: &InteractiveCompilerState,
) -> Result<InteractiveCompilation, SourceError> {
    compile_interactive_with_resolver(path, source, state, |_| None)
}

pub(crate) fn compile_interactive_with_resolver(
    path: &str,
    source: &str,
    state: &InteractiveCompilerState,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<InteractiveCompilation, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let mut imports = typecheck::static_import_names(&expressions)
        .into_iter()
        .filter_map(|name| resolve(&name).map(|snapshot| (name, snapshot)))
        .collect::<HashMap<_, _>>();
    if let std::collections::hash_map::Entry::Vacant(entry) = imports.entry("slug.builtin".into())
        && let Some(snapshot) = resolve("slug.builtin")
    {
        entry.insert(snapshot);
    }
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
    Ok(InteractiveCompilation {
        program,
        state: InteractiveCompilerState {
            semantic: analysis.session_snapshot,
            globals: compiled.globals,
            callable_globals: compiled.callable_globals,
        },
    })
}

pub(crate) fn compile_with_resolver(
    path: &str,
    source: &str,
    include_implicit_builtins: bool,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let mut imports = typecheck::static_import_names(&expressions)
        .into_iter()
        .filter_map(|name| resolve(&name).map(|snapshot| (name, snapshot)))
        .collect::<HashMap<_, _>>();
    if include_implicit_builtins
        && let std::collections::hash_map::Entry::Vacant(entry) =
            imports.entry("slug.builtin".into())
        && let Some(snapshot) = resolve("slug.builtin")
    {
        entry.insert(snapshot);
    }
    compile_expressions(path, expressions, imports)
}

fn compile_expressions(
    path: &str,
    expressions: Vec<ast::Expr>,
    imports: ImportSnapshots,
) -> Result<Program, SourceError> {
    let analysis = typecheck::analyze_with_imports(&expressions, imports)?;
    let mut program = Compiler::new(path, expressions, &analysis)
        .compile()?
        .program;
    program.set_semantic_snapshot(analysis.snapshot);
    Ok(program)
}
