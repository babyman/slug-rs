//! The one diagnostic document shared by `--diagnostic-format=json` and the
//! interactive server. See `docs/reference/diagnostics.md`.

use serde::{Deserialize, Serialize};

use slug_frontend::{SourceError, SourceErrorKind};
use slug_vm::{RuntimeError, RuntimeErrorKind, SourceSpan, Value};

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
        let category = match error.kind {
            SourceErrorKind::Parse => DiagnosticCategory::Parse,
            SourceErrorKind::Semantic => DiagnosticCategory::Semantic,
        };
        Self {
            location: error.span.as_ref().map(location),
            range: error.range.map(|range| DiagnosticRange {
                start: DiagnosticPosition {
                    line: range.start.line,
                    column: range.start.column,
                },
                end: DiagnosticPosition {
                    line: range.end.line,
                    column: range.end.column,
                },
            }),
            ..Self::new(category, error.code.as_str(), error.message.clone())
        }
    }

    #[must_use]
    pub fn from_runtime(error: &RuntimeError) -> Self {
        let category = if error.kind == RuntimeErrorKind::Module {
            DiagnosticCategory::Module
        } else {
            DiagnosticCategory::Runtime
        };
        Self {
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
            cause: error
                .cause
                .as_deref()
                .map(|cause| Box::new(Self::from_runtime(cause))),
            ..Self::new(category, runtime_code(&error.kind), error.message.clone())
        }
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
