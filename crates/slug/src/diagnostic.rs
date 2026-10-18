//! The one diagnostic document shared by `--diagnostic-format=json`, the
//! interactive server, and text output, which renders the same document. See
//! `docs/reference/diagnostics.md`.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use slug_frontend::{SourceError, SourceFix, SourcePosition, SourceRange, import_name_range};
use slug_vm::{
    RuntimeError, RuntimeErrorKind, SourceDiagnostic, SourceErrorKind, SourceSpan, Value,
};

/// The diagnostic document version.
pub const DIAGNOSTIC_VERSION: u8 = 2;

/// A versioned, wire-safe projection of a Slug, host, or protocol failure.
///
/// Every member is always present, `null` when it does not apply, except
/// `thrown`, which is present only for `thrown` runtime errors so that
/// `throw nil` remains distinguishable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub version: u8,
    pub category: DiagnosticCategory,
    /// The stable failure class (`docs/reference/diagnostic-codes.md`).
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub location: Option<DiagnosticLocation>,
    #[serde(default)]
    pub range: Option<DiagnosticRange>,
    /// What was required at the location, such as a type or token, when known.
    #[serde(default)]
    pub expected: Option<String>,
    /// What was found instead, when known.
    #[serde(default)]
    pub actual: Option<String>,
    /// Machine-applicable fixes, each a set of edits in `location`'s file.
    #[serde(default)]
    pub fixes: Vec<DiagnosticFix>,
    /// Likely, but not certain, corrections in the same form as `fixes`. They
    /// may change what the program means, so review one before applying it.
    #[serde(default)]
    pub suggestions: Vec<DiagnosticFix>,
    #[serde(default)]
    pub frames: Vec<DiagnosticFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thrown: Option<serde_json::Value>,
    #[serde(default)]
    pub native: Option<NativeDiagnostic>,
    #[serde(default)]
    pub cause: Option<Box<Self>>,
}

/// The stage or service that produced a diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Parse,
    Semantic,
    Module,
    Runtime,
    Usage,
    Io,
    Host,
    Artifact,
    Protocol,
}

impl DiagnosticCategory {
    /// The category's wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Semantic => "semantic",
            Self::Module => "module",
            Self::Runtime => "runtime",
            Self::Usage => "usage",
            Self::Io => "io",
            Self::Host => "host",
            Self::Artifact => "artifact",
            Self::Protocol => "protocol",
        }
    }
}

/// A source point. For an operator error it is the operator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticLocation {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

/// The extent of the offending source in the location's file; `end` is
/// exclusive.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticRange {
    pub start: DiagnosticPosition,
    pub end: DiagnosticPosition,
}

/// A 1-based line and column counting Unicode scalar values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticPosition {
    pub line: u32,
    pub column: u32,
}

/// One machine-applicable fix. Apply all of its edits together.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticFix {
    pub message: String,
    pub edits: Vec<DiagnosticEdit>,
}

/// Replace `range` with `text`; an empty range inserts and empty text deletes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticEdit {
    pub range: DiagnosticRange,
    pub text: String,
}

/// One Slug call frame in a runtime diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticFrame {
    pub function: String,
    #[serde(default)]
    pub location: Option<DiagnosticLocation>,
}

/// Structured native error details retained by runtime diagnostics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NativeDiagnostic {
    pub code: String,
    /// The native error's data in the value JSON form, or `null`.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

impl Diagnostic {
    /// A failure with no source location, such as a startup or protocol error.
    #[must_use]
    pub fn new(category: DiagnosticCategory, code: &str, message: impl Into<String>) -> Self {
        Self {
            version: DIAGNOSTIC_VERSION,
            category,
            code: code.into(),
            message: message.into(),
            location: None,
            range: None,
            expected: None,
            actual: None,
            fixes: Vec::new(),
            suggestions: Vec::new(),
            frames: Vec::new(),
            thrown: None,
            native: None,
            cause: None,
        }
    }

    pub(crate) fn protocol(code: &str, message: impl Into<String>) -> Self {
        Self::new(DiagnosticCategory::Protocol, code, message)
    }

    #[must_use]
    pub fn host(code: &str, message: impl Into<String>) -> Self {
        Self::new(DiagnosticCategory::Host, code, message)
    }

    #[must_use]
    pub fn from_source(error: &SourceError) -> Self {
        Self::from_source_diagnostic(&error.into())
    }

    #[must_use]
    pub fn from_source_diagnostic(error: &SourceDiagnostic) -> Self {
        let category = match error.kind {
            SourceErrorKind::Parse => DiagnosticCategory::Parse,
            SourceErrorKind::Semantic => DiagnosticCategory::Semantic,
        };
        Self {
            location: error.span.as_ref().map(location),
            range: error.range.map(range),
            fixes: error.fixes.iter().map(diagnostic_fix).collect(),
            suggestions: error.suggestions.iter().map(diagnostic_fix).collect(),
            expected: error.expected.as_deref().map(str::to_owned),
            actual: error.actual.as_deref().map(str::to_owned),
            ..Self::new(category, error.code, error.message.clone())
        }
    }

    #[must_use]
    pub fn from_runtime(error: &RuntimeError) -> Self {
        Self::from_runtime_in(error, &|_| None)
    }

    /// Like [`Self::from_runtime`], reading source text through `sources`,
    /// keyed by location path, to locate a not-found module's name in its
    /// `import` call.
    #[must_use]
    pub fn from_runtime_in(error: &RuntimeError, sources: &dyn Fn(&str) -> Option<String>) -> Self {
        let category = if matches!(
            error.kind,
            RuntimeErrorKind::Module | RuntimeErrorKind::ModuleNotFound
        ) {
            DiagnosticCategory::Module
        } else {
            DiagnosticCategory::Runtime
        };
        let mut diagnostic = Self {
            location: error.span.as_ref().map(location),
            frames: error
                .frames
                .iter()
                .map(|frame| DiagnosticFrame {
                    function: frame.function.clone(),
                    location: frame.span.as_ref().map(location),
                })
                .collect(),
            thrown: error.thrown.as_deref().map(value_json),
            native: error.native.as_deref().map(|native| NativeDiagnostic {
                code: native.code.clone(),
                data: native.data.as_ref().map(value_json),
            }),
            // An import of a module that failed to compile is caused by that
            // module's source error.
            cause: error.compile_error().map_or_else(
                || {
                    error
                        .cause
                        .as_deref()
                        .map(|cause| Box::new(Self::from_runtime_in(cause, sources)))
                },
                |compile_error| Some(Box::new(Self::from_source_diagnostic(compile_error))),
            ),
            ..Self::new(category, runtime_code(&error.kind), error.message.clone())
        };
        if let Some(missing) = error.missing_module() {
            diagnostic.suggest_modules(missing, sources);
        }
        diagnostic
    }

    /// Points a not-found module diagnostic at the module's name in its
    /// `import` call, and suggests each close name as a replacement for it.
    fn suggest_modules(
        &mut self,
        missing: &slug_vm::MissingModule,
        sources: &dyn Fn(&str) -> Option<String>,
    ) {
        let name_range = self.location.as_ref().and_then(|location| {
            let source = sources(&location.path)?;
            import_name_range(
                &location.path,
                &source,
                location.line,
                location.column,
                &missing.name,
            )
        });
        if let (Some(location), Some(name_range)) = (self.location.as_mut(), name_range) {
            location.line = name_range.start.line;
            location.column = name_range.start.column;
            self.range = Some(range(name_range));
        }
        self.suggestions = missing
            .suggestions
            .iter()
            .map(|suggestion| DiagnosticFix {
                message: format!("did you mean `{suggestion}`?"),
                edits: self
                    .range
                    .iter()
                    .map(|range| DiagnosticEdit {
                        range: range.clone(),
                        text: format!("\"{suggestion}\""),
                    })
                    .collect(),
            })
            .collect();
        // Imports cannot reach upward, so a module above the search has no
        // edit; the importing file must move beside it.
        self.suggestions
            .extend(missing.elsewhere.iter().map(|path| DiagnosticFix {
                message: format!(
                    "`{path}` exists outside the directories searched: imports look only in the importing file's directory, the program's directory, and installed modules, so move the importing file beside it"
                ),
                edits: Vec::new(),
            }));
    }

    /// Renders this document as command-line text: the same members as the
    /// JSON form, with the offending source line when `sources` can read it.
    /// `sources` returns a file's text by location path.
    #[must_use]
    pub fn render_text(&self, sources: &dyn Fn(&str) -> Option<String>) -> String {
        let mut rendered = String::new();
        self.render_text_into(&mut rendered, "slug:", sources);
        rendered
    }

    fn render_text_into(
        &self,
        rendered: &mut String,
        prefix: &str,
        sources: &dyn Fn(&str) -> Option<String>,
    ) {
        let label = match self.category {
            DiagnosticCategory::Parse => Some("parse error"),
            DiagnosticCategory::Semantic => Some("semantic error"),
            DiagnosticCategory::Runtime | DiagnosticCategory::Module => Some("runtime error"),
            _ => None,
        };
        match label {
            Some(label) => write!(rendered, "{prefix} {label}: {}", self.message),
            None => write!(rendered, "{prefix} {}", self.message),
        }
        .expect("writing to a string cannot fail");
        if let Some(location) = &self.location {
            write!(
                rendered,
                "\n    --> {}:{}:{}",
                location.path, location.line, location.column
            )
            .expect("writing to a string cannot fail");
            if let Some(source) = sources(&location.path) {
                render_source_lines(rendered, &source, location, self.range.as_ref());
            }
        }
        for frame in &self.frames {
            write!(rendered, "\n  in {}", frame.function).expect("writing to a string cannot fail");
            if let Some(location) = &frame.location {
                write!(
                    rendered,
                    " at {}:{}:{}",
                    location.path, location.line, location.column
                )
                .expect("writing to a string cannot fail");
            }
        }
        write!(rendered, "\n  = code: {}", self.code).expect("writing to a string cannot fail");
        match (&self.expected, &self.actual) {
            (Some(expected), Some(actual)) => {
                write!(rendered, "\n  = expected {expected}, found {actual}")
            }
            (Some(expected), None) => write!(rendered, "\n  = expected {expected}"),
            (None, Some(actual)) => write!(rendered, "\n  = found {actual}"),
            (None, None) => Ok(()),
        }
        .expect("writing to a string cannot fail");
        if let Some(native) = &self.native {
            write!(rendered, "\n  = native: {}", native.code)
                .expect("writing to a string cannot fail");
            if let Some(data) = &native.data {
                write!(rendered, " {data}").expect("writing to a string cannot fail");
            }
        }
        for (label, fixes) in [("fix", &self.fixes), ("suggestion", &self.suggestions)] {
            for fix in fixes {
                write!(rendered, "\n  = {label}: {}", fix.message)
                    .expect("writing to a string cannot fail");
                for edit in &fix.edits {
                    write!(rendered, "\n      {}", render_edit(edit))
                        .expect("writing to a string cannot fail");
                }
            }
        }
        if let Some(cause) = &self.cause {
            rendered.push_str("\n\n");
            cause.render_text_into(rendered, "caused by:", sources);
        }
    }
}

fn render_edit(edit: &DiagnosticEdit) -> String {
    let DiagnosticRange { start, end } = &edit.range;
    let at = if start == end {
        format!("at {}:{}", start.line, start.column)
    } else {
        format!(
            "at {}:{}-{}:{}",
            start.line, start.column, end.line, end.column
        )
    };
    let text = if edit.text.contains(['`', '\n', '\r']) {
        format!("{:?}", edit.text)
    } else {
        format!("`{}`", edit.text)
    };
    match (start == end, edit.text.is_empty()) {
        (true, _) => format!("{at}: insert {text}"),
        (false, true) => format!("{at}: delete"),
        (false, false) => format!("{at}: replace with {text}"),
    }
}

/// Writes up to two lines before `location`'s line and that line, then
/// underlines the range when it lies on that line, or marks the location.
fn render_source_lines(
    rendered: &mut String,
    source: &str,
    location: &DiagnosticLocation,
    range: Option<&DiagnosticRange>,
) {
    // An error at the end of input may sit on the empty line after the last
    // newline.
    let lines = || source.lines().chain(std::iter::once(""));
    let Some(line) = lines().nth(location.line.saturating_sub(1) as usize) else {
        return;
    };
    let first_line = location.line.saturating_sub(2).max(1);
    let line_width = location.line.to_string().len();
    for (index, source_line) in lines()
        .enumerate()
        .skip(first_line as usize - 1)
        .take((location.line - first_line + 1) as usize)
    {
        let number = index + 1;
        let marker = if number == location.line as usize {
            ">"
        } else {
            " "
        };
        write!(
            rendered,
            "\n  {marker} {number:>line_width$} | {}",
            expand_tabs(source_line)
        )
        .expect("writing to a string cannot fail");
    }
    let (start, width) = range
        .filter(|range| range.start.line == location.line && range.end.line == location.line)
        .map_or((display_column(line, location.column), 1), |range| {
            let start = display_column(line, range.start.column);
            let end = display_column(line, range.end.column);
            (start, end.saturating_sub(start).max(1))
        });
    write!(
        rendered,
        "\n    {} | {}{} here",
        " ".repeat(line_width),
        " ".repeat(start),
        "^".repeat(width)
    )
    .expect("writing to a string cannot fail");
}

fn expand_tabs(text: &str) -> String {
    let mut expanded = String::new();
    let mut column = 0;
    for character in text.chars() {
        if character == '\t' {
            let spaces = 4 - (column % 4);
            expanded.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            expanded.push(character);
            column += 1;
        }
    }
    expanded
}

/// The display column of a 1-based scalar column, with tabs at width 4.
fn display_column(text: &str, source_column: u32) -> usize {
    let characters = source_column.saturating_sub(1) as usize;
    let mut column = 0;
    for character in text.chars().take(characters) {
        if character == '\t' {
            column += 4 - (column % 4);
        } else {
            column += 1;
        }
    }
    column
}

fn diagnostic_fix(fix: &SourceFix) -> DiagnosticFix {
    DiagnosticFix {
        message: fix.message.clone(),
        edits: fix
            .edits
            .iter()
            .map(|edit| DiagnosticEdit {
                range: range(edit.range),
                text: edit.text.clone(),
            })
            .collect(),
    }
}

fn range(range: SourceRange) -> DiagnosticRange {
    let position = |position: SourcePosition| DiagnosticPosition {
        line: position.line,
        column: position.column,
    };
    DiagnosticRange {
        start: position(range.start),
        end: position(range.end),
    }
}

fn location(span: &SourceSpan) -> DiagnosticLocation {
    DiagnosticLocation {
        path: span.path.to_string(),
        line: span.line,
        column: span.column,
    }
}

/// The stable code of a runtime error kind.
#[must_use]
pub fn runtime_code(kind: &RuntimeErrorKind) -> &'static str {
    match kind {
        RuntimeErrorKind::InvalidBytecode => "invalid_bytecode",
        RuntimeErrorKind::Type => "type",
        RuntimeErrorKind::Name => "name",
        RuntimeErrorKind::Arity => "arity",
        RuntimeErrorKind::DivideByZero => "divide_by_zero",
        RuntimeErrorKind::InvalidCall => "invalid_call",
        RuntimeErrorKind::Native => "native",
        RuntimeErrorKind::NativeContract => "native_contract",
        RuntimeErrorKind::Module => "module",
        RuntimeErrorKind::ModuleNotFound => "module_not_found",
        RuntimeErrorKind::NotImplemented => "not_implemented",
        RuntimeErrorKind::Match => "match",
        RuntimeErrorKind::Thrown => "thrown",
    }
}

/// Projects a value into the value JSON form (`docs/reference/value-json.md`).
///
/// # Panics
///
/// Never in practice: the encoder emits valid JSON within serde's nesting limit.
#[must_use]
pub fn value_json(value: &Value) -> serde_json::Value {
    serde_json::from_str(&value.to_json()).expect("the value JSON form is valid JSON")
}
