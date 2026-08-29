//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashMap, fmt};

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

use self::environment::{ImportSnapshots, ModuleSnapshot};

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
/// Returns a source error with a source location for invalid syntax or checked
/// source semantics.
pub fn compile(path: &str, source: &str) -> Result<Program, SourceError> {
    compile_with_imports(path, source, false, ImportSnapshots::new())
}

/// Compiles source with the optional static annotation checker enabled.
///
/// # Errors
///
/// Returns a source error with a source location for invalid annotations or
/// directly provable type mismatches.
pub fn compile_type_checked(path: &str, source: &str) -> Result<Program, SourceError> {
    compile_with_imports(path, source, true, ImportSnapshots::new())
}

pub(crate) fn compile_with_resolver(
    path: &str,
    source: &str,
    type_check: bool,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let imports = typecheck::static_import_names(&expressions)
        .into_iter()
        .filter_map(|name| resolve(&name).map(|snapshot| (name, snapshot)))
        .collect::<HashMap<_, _>>();
    compile_expressions(path, expressions, type_check, imports)
}

pub(crate) fn semantic_snapshot(path: &str, source: &str) -> Result<ModuleSnapshot, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    typecheck::validate(&expressions).map(|analysis| analysis.snapshot)
}

fn compile_with_imports(
    path: &str,
    source: &str,
    type_check: bool,
    imports: ImportSnapshots,
) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    compile_expressions(path, expressions, type_check, imports)
}

fn compile_expressions(
    path: &str,
    expressions: Vec<ast::Expr>,
    type_check: bool,
    imports: ImportSnapshots,
) -> Result<Program, SourceError> {
    let analysis = if type_check {
        typecheck::check_with_imports(&expressions, imports)?
    } else {
        typecheck::validate_with_imports(&expressions, imports)?
    };
    let mut program = Compiler::new(path, expressions, &analysis).compile()?;
    program.set_semantic_snapshot(analysis.snapshot);
    Ok(program)
}
