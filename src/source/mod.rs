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

pub(crate) fn compile_interactive(
    path: &str,
    source: &str,
    state: &InteractiveCompilerState,
) -> Result<InteractiveCompilation, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let analysis = typecheck::analyze_with_imports_and_session(
        &expressions,
        ImportSnapshots::new(),
        &state.semantic,
    )?;
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
