//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashMap, fmt};

use slug_loader::{ForeignDeclaration, ForeignParameter};
use slug_vm::{Program, SourceSpan};
pub use slug_vm::{
    SourceDiagnostic, SourceEdit, SourceErrorKind, SourceFix, SourcePosition, SourceRange,
    apply_fix,
};

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
mod stack;
#[path = "lowering/state.rs"]
mod state;
#[path = "semantics/typecheck.rs"]
mod typecheck;
#[path = "semantics/updates.rs"]
mod updates;
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
    /// The extent of the offending source in `span`'s file, when known. It
    /// contains `span`, which may be a focus point inside it such as an
    /// operator.
    pub range: Option<SourceRange>,
    /// What the checker or parser required here, such as a type (`num`) or
    /// a token (`` `}` ``), when it is known.
    pub expected: Option<Box<str>>,
    /// What it found instead, when it is known.
    pub actual: Option<Box<str>>,
    /// Its fixes and suggestions, when it has any; see
    /// [`SourceError::fixes`] and [`SourceError::suggestions`].
    pub corrections: Option<Box<SourceCorrections>>,
}

/// The edits a [`SourceError`] offers, boxed because most errors have none.
#[derive(Clone, Debug, Default)]
pub struct SourceCorrections {
    /// Edits that resolve the error without changing what the rest of the
    /// program means. Empty unless a fix is certain.
    pub fixes: Vec<SourceFix>,
    /// Likely corrections in the same form as `fixes`, which may change what
    /// the program means.
    pub suggestions: Vec<SourceFix>,
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
    LostUpdate,
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
        Self::LostUpdate,
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
            Self::LostUpdate => "lost_update",
            Self::InvalidEntrypoint => "invalid_entrypoint",
            Self::Internal => "internal",
        }
    }
}

impl SourceError {
    fn at(code: SourceErrorCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Parse,
            code,
            message: message.into(),
            span: Some(span),
            range: None,
            expected: None,
            actual: None,
            corrections: None,
        }
    }

    fn at_token(code: SourceErrorCode, message: impl Into<String>, token: &ast::Token) -> Self {
        let mut error = Self::at(code, message, token.span.clone()).with_end(token.end);
        error.actual = Some(token.kind.describe().into());
        error
    }

    /// Edits that resolve this error without changing what the rest of the
    /// program means. Empty unless a fix is certain.
    #[must_use]
    pub fn fixes(&self) -> &[SourceFix] {
        self.corrections
            .as_ref()
            .map_or(&[], |corrections| &corrections.fixes)
    }

    /// Likely corrections in the same form as [`Self::fixes`], which may
    /// change what the program means.
    #[must_use]
    pub fn suggestions(&self) -> &[SourceFix] {
        self.corrections
            .as_ref()
            .map_or(&[], |corrections| &corrections.suggestions)
    }

    fn with_fix(self, message: impl Into<String>, edit: SourceEdit) -> Self {
        self.with_fix_edits(message, vec![edit])
    }

    /// A fix made of several edits, applied together.
    fn with_fix_edits(mut self, message: impl Into<String>, edits: Vec<SourceEdit>) -> Self {
        self.corrections
            .get_or_insert_default()
            .fixes
            .push(SourceFix {
                message: message.into(),
                edits,
            });
        self
    }

    fn with_suggestion(self, message: impl Into<String>, edit: SourceEdit) -> Self {
        self.with_suggestion_edits(message, vec![edit])
    }

    /// A suggestion made of several edits, applied together.
    fn with_suggestion_edits(mut self, message: impl Into<String>, edits: Vec<SourceEdit>) -> Self {
        self.corrections
            .get_or_insert_default()
            .suggestions
            .push(SourceFix {
                message: message.into(),
                edits,
            });
        self
    }

    /// Covers a whole match case and, when deleting it cannot change
    /// behavior, offers to delete it. A case never selected is never run,
    /// but its pattern operands are still evaluated when it is tested, so
    /// cases with pinned names or computed keys get no fix.
    fn unreachable_case(mut self, case: &ast::MatchCase) -> Self {
        let range = SourceRange {
            start: case.start,
            end: case.end,
        };
        self.range = Some(range);
        if case
            .patterns
            .iter()
            .all(|pattern| pattern.pattern.is_static())
        {
            self = self.with_fix(
                "remove the unreachable case",
                SourceEdit {
                    range,
                    text: String::new(),
                },
            );
        }
        self
    }

    fn with_expected_actual(mut self, expected: String, actual: String) -> Self {
        self.expected = Some(expected.into());
        self.actual = Some(actual.into());
        self
    }

    /// Extends this error from its span to `end`.
    fn with_end(mut self, end: SourcePosition) -> Self {
        if let Some(span) = &self.span {
            let start = SourcePosition {
                line: span.line,
                column: span.column,
            };
            self.range = Some(SourceRange { start, end });
        }
        self
    }

    /// Gives a range-less error raised at `expression`'s span that
    /// expression's extent. Applied as errors leave each expression, so the
    /// innermost expression at the error's span claims it.
    fn claimed_by(self, expression: &ast::Expr) -> Self {
        if self.range.is_none() && self.span.as_ref() == Some(&expression.span) {
            self.with_extent(expression)
        } else {
            self
        }
    }

    /// Covers all of `expression`, including operands left of its span.
    fn with_extent(mut self, expression: &ast::Expr) -> Self {
        self.range = Some(SourceRange {
            start: expression.extent_start(),
            end: expression.end,
        });
        self
    }

    fn semantic(code: SourceErrorCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Semantic,
            code,
            message: message.into(),
            span: Some(span),
            range: None,
            expected: None,
            actual: None,
            corrections: None,
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

impl From<&SourceError> for SourceDiagnostic {
    fn from(error: &SourceError) -> Self {
        Self {
            kind: error.kind,
            code: error.code.as_str(),
            message: error.message.clone(),
            span: error.span.clone(),
            range: error.range,
            expected: error.expected.clone(),
            actual: error.actual.clone(),
            fixes: error.fixes().into(),
            suggestions: error.suggestions().into(),
        }
    }
}

/// Compiles the currently supported core Slug source subset into a VM program.
///
/// # Errors
/// Returns a source error with a source location for invalid syntax or source
/// semantics, including directly provable type mismatches.
pub fn compile(path: &str, source: &str) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let mut compilation =
        compile_expressions(path, expressions, ImportSnapshots::new(), None, None)?;
    compilation.program.set_module_key(path);
    Ok(compilation.program)
}

/// Compiles a module's source. `module_name` is the name it is imported by,
/// which its resource and enum types carry at runtime; an entry program has
/// none. Every module but `slug.builtin` sees that module's exports.
pub(crate) fn compile_with_resolver(
    path: &str,
    source: &str,
    module_name: Option<&str>,
    mut resolve: impl FnMut(&str) -> Option<ModuleSnapshot>,
) -> Result<CompiledSource, SourceError> {
    let include_implicit_builtins = module_name != Some("slug.builtin");
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
    // With the implicit module's exports known, every name the runtime
    // provides is known, so an undeclared one is an error now rather than
    // when it runs. An explicit import of `slug.builtin` does not tell
    // which names the runtime installs, so it does not count.
    let implicit_globals = include_implicit_builtins
        .then(|| imports.get("slug.builtin"))
        .flatten()
        .map(|builtins| {
            builtins
                .export_names()
                .map(String::from)
                .collect::<Vec<_>>()
        });
    let mut compilation =
        compile_expressions(path, expressions, imports, implicit_globals, module_name)?;
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

/// The range of the string literal naming `module` in the `import` call that
/// starts at `line` and `column` of `source`, when the call names it with a
/// plain string literal. Hosts use it to point a module diagnostic at the
/// name rather than the call.
#[must_use]
pub fn import_name_range(
    path: &str,
    source: &str,
    line: u32,
    column: u32,
    module: &str,
) -> Option<SourceRange> {
    let tokens = Lexer::new(path, source).tokens().ok()?;
    let call = tokens.iter().position(|token| {
        token.span.line == line
            && token.span.column == column
            && matches!(&token.kind, ast::TokenKind::Name(name) if name == "import")
    })?;
    let mut depth = 0_usize;
    for token in &tokens[call + 1..] {
        match &token.kind {
            ast::TokenKind::LParen => depth += 1,
            ast::TokenKind::RParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return None;
                }
            }
            ast::TokenKind::Interpolated(parts) if depth == 1 => {
                if let [ast::StringPart::Text(text)] = parts.as_slice()
                    && text == module
                {
                    return Some(SourceRange {
                        start: SourcePosition {
                            line: token.span.line,
                            column: token.span.column,
                        },
                        end: token.end,
                    });
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    None
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
    implicit_globals: Option<Vec<String>>,
    module_name: Option<&str>,
) -> Result<CompiledSource, SourceError> {
    stack::grow(|| {
        let analysis = typecheck::analyze_with_imports(&expressions, imports, module_name)?;
        updates::check_lost_updates(&expressions)?;
        let mut compiler = Compiler::new(path, expressions, &analysis);
        if let Some(name) = module_name {
            compiler = compiler.with_module_name(name);
        }
        if let Some(names) = implicit_globals {
            compiler = compiler.with_implicit_globals(names);
        }
        let program = compiler.compile()?.program;
        Ok(CompiledSource {
            program,
            snapshot: analysis.snapshot,
        })
    })
}
