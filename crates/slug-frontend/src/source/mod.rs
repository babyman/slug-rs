//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashMap, fmt};

use slug_loader::{ForeignDeclaration, ForeignParameter};
use slug_vm::{Program, SourceSpan};

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
pub(crate) use typecheck::ImportScan;

#[derive(Clone, Debug)]
pub struct SourceError {
    pub kind: SourceErrorKind,
    pub code: SourceErrorCode,
    pub message: String,
    pub span: Option<SourceSpan>,
}

/// The stable failure class of a [`SourceError`], listed in
/// `docs/reference/diagnostic-codes.md`. Messages may change; codes do not.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SourceErrorCode {
    UnexpectedCharacter,
    UnterminatedLiteral,
    InvalidNumber,
    InvalidByteLiteral,
    ExpectedSyntax,
    InvalidDocumentation,
    TopLevelOnly,
    InvalidParameters,
    InvalidArgumentOrder,
    InvalidPattern,
    EmptyEnum,
    NestingLimitExceeded,
    DuplicateName,
    DuplicateSignature,
    UnknownName,
    UnknownType,
    UnknownMember,
    MissingField,
    TypeMismatch,
    InvalidTypeArguments,
    RecursiveTypeAlias,
    ImmutableAssignment,
    InvalidControlFlow,
    InvalidMatch,
    NonExhaustiveMatch,
    UnreachableMatchCase,
    NoMatchingOverload,
    AmbiguousOverload,
    RepeatedMapKey,
    InvalidEntrypoint,
    Internal,
}

impl SourceErrorCode {
    /// Every code, in registry order.
    pub const ALL: &[Self] = &[
        Self::UnexpectedCharacter,
        Self::UnterminatedLiteral,
        Self::InvalidNumber,
        Self::InvalidByteLiteral,
        Self::ExpectedSyntax,
        Self::InvalidDocumentation,
        Self::TopLevelOnly,
        Self::InvalidParameters,
        Self::InvalidArgumentOrder,
        Self::InvalidPattern,
        Self::EmptyEnum,
        Self::NestingLimitExceeded,
        Self::DuplicateName,
        Self::DuplicateSignature,
        Self::UnknownName,
        Self::UnknownType,
        Self::UnknownMember,
        Self::MissingField,
        Self::TypeMismatch,
        Self::InvalidTypeArguments,
        Self::RecursiveTypeAlias,
        Self::ImmutableAssignment,
        Self::InvalidControlFlow,
        Self::InvalidMatch,
        Self::NonExhaustiveMatch,
        Self::UnreachableMatchCase,
        Self::NoMatchingOverload,
        Self::AmbiguousOverload,
        Self::RepeatedMapKey,
        Self::InvalidEntrypoint,
        Self::Internal,
    ];

    /// The code's stable `snake_case` name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnexpectedCharacter => "unexpected_character",
            Self::UnterminatedLiteral => "unterminated_literal",
            Self::InvalidNumber => "invalid_number",
            Self::InvalidByteLiteral => "invalid_byte_literal",
            Self::ExpectedSyntax => "expected_syntax",
            Self::InvalidDocumentation => "invalid_documentation",
            Self::TopLevelOnly => "top_level_only",
            Self::InvalidParameters => "invalid_parameters",
            Self::InvalidArgumentOrder => "invalid_argument_order",
            Self::InvalidPattern => "invalid_pattern",
            Self::EmptyEnum => "empty_enum",
            Self::NestingLimitExceeded => "nesting_limit_exceeded",
            Self::DuplicateName => "duplicate_name",
            Self::DuplicateSignature => "duplicate_signature",
            Self::UnknownName => "unknown_name",
            Self::UnknownType => "unknown_type",
            Self::UnknownMember => "unknown_member",
            Self::MissingField => "missing_field",
            Self::TypeMismatch => "type_mismatch",
            Self::InvalidTypeArguments => "invalid_type_arguments",
            Self::RecursiveTypeAlias => "recursive_type_alias",
            Self::ImmutableAssignment => "immutable_assignment",
            Self::InvalidControlFlow => "invalid_control_flow",
            Self::InvalidMatch => "invalid_match",
            Self::NonExhaustiveMatch => "non_exhaustive_match",
            Self::UnreachableMatchCase => "unreachable_match_case",
            Self::NoMatchingOverload => "no_matching_overload",
            Self::AmbiguousOverload => "ambiguous_overload",
            Self::RepeatedMapKey => "repeated_map_key",
            Self::InvalidEntrypoint => "invalid_entrypoint",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceErrorKind {
    Parse,
    Semantic,
}

impl SourceError {
    fn at(code: SourceErrorCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Parse,
            code,
            message: message.into(),
            span: Some(span),
        }
    }

    fn semantic(code: SourceErrorCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Semantic,
            code,
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
    compilation.program.set_module_key(path);
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
    compilation.program.set_module_key(path);
    Ok(compilation)
}

pub(crate) fn static_import_names(path: &str, source: &str) -> Result<Vec<String>, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    Ok(typecheck::static_import_names(&expressions))
}

pub(crate) fn import_scan(path: &str, source: &str) -> Result<ImportScan, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    Ok(typecheck::import_scan(&expressions))
}

/// Returns the top-level `foreign` declarations of `source` with their type
/// annotations rendered as canonical text.
///
/// # Errors
///
/// Returns a source error when `source` does not parse.
pub fn foreign_declarations(
    path: &str,
    source: &str,
) -> Result<Vec<ForeignDeclaration>, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    Ok(expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            ast::ExprKind::Foreign {
                name, signature, ..
            } => Some(ForeignDeclaration {
                name: name.clone(),
                parameters: signature
                    .parameters
                    .iter()
                    .map(|parameter| ForeignParameter {
                        name: parameter.name.clone(),
                        annotation: parameter.annotation.as_ref().map(annotation_text),
                        variadic: parameter.variadic,
                        has_default: parameter.default.is_some(),
                    })
                    .collect(),
                result: signature.return_annotation.as_ref().map(annotation_text),
            }),
            _ => None,
        })
        .collect())
}

fn annotation_text(annotation: &ast::TypeAnnotation) -> String {
    let join = |elements: &[ast::TypeAnnotation]| {
        elements
            .iter()
            .map(annotation_text)
            .collect::<Vec<_>>()
            .join(",")
    };
    match annotation {
        ast::TypeAnnotation::Name(name) => name.clone(),
        ast::TypeAnnotation::Apply { name, arguments } => format!("{name}<{}>", join(arguments)),
        ast::TypeAnnotation::Tuple(elements) => format!("({})", join(elements)),
        ast::TypeAnnotation::Union(members) => {
            let mut members = members.iter().map(annotation_text).collect::<Vec<_>>();
            members.sort();
            members.dedup();
            members.join("|")
        }
    }
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
