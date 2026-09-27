//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashMap, fmt};

use crate::{ModuleKey, Program, SourceSpan};

#[path = "syntax/ast.rs"]
mod ast;
#[path = "lowering/compiler.rs"]
mod compiler;
#[path = "semantics/environment.rs"]
pub(crate) mod environment;
mod interactive;
#[path = "syntax/lexer.rs"]
mod lexer;
#[path = "syntax/parser.rs"]
mod parser;
#[path = "semantics/semantic.rs"]
mod semantic;
#[path = "lowering/state.rs"]
mod state;
#[path = "semantics/typecheck.rs"]
mod typecheck;
use compiler::Compiler;
use lexer::Lexer;
use parser::Parser;

use self::environment::{ImportSnapshots, ModuleSnapshot};
pub use interactive::compile_interactive_forms;
pub(crate) use interactive::compile_interactive_forms_with_resolver;
pub use interactive::{
    InteractiveCompilation, InteractiveCompilerState, SourceReadiness, source_readiness,
};

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
    let mut compilation = compile_expressions(path, expressions, ImportSnapshots::new())?;
    compilation.program.set_module_key(ModuleKey::new(path));
    Ok(compilation.program)
}

pub(crate) fn compile_with_resolver(
    path: &str,
    source: &str,
    include_implicit_builtins: bool,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<CompiledSource, SourceError> {
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
    let mut compilation = compile_expressions(path, expressions, imports)?;
    compilation.program.set_module_key(ModuleKey::new(path));
    Ok(compilation)
}

pub(crate) struct CompiledSource {
    pub(crate) program: Program,
    pub(crate) snapshot: ModuleSnapshot,
}

fn compile_expressions(
    path: &str,
    expressions: Vec<ast::Expr>,
    imports: ImportSnapshots,
) -> Result<CompiledSource, SourceError> {
    let analysis = typecheck::analyze_with_imports(&expressions, imports)?;
    let program = Compiler::new(path, expressions, &analysis)
        .compile()?
        .program;
    Ok(CompiledSource {
        program,
        snapshot: analysis.snapshot,
    })
}
