use serde::{Deserialize, Serialize};

use slug_frontend::{SourceError, SourceErrorKind};
use slug_vm::{RuntimeError, RuntimeErrorKind, SourceSpan, Value};

/// A versioned, wire-safe projection of a Slug or server failure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub version: u8,
    pub category: DiagnosticCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<DiagnosticRange>,
    pub frames: Vec<DiagnosticFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thrown: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<Self>>,
}

/// The origin of a protocol diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Source,
    Runtime,
    Protocol,
    Host,
}

/// A source coordinate preserved in a diagnostic projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticLocation {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

/// The extent of the offending source; `end` is exclusive.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
}

/// Structured native error details retained by runtime diagnostics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NativeDiagnostic {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Diagnostic {
    pub(crate) fn protocol(code: &str, message: impl Into<String>) -> Self {
        Self {
            version: 1,
            category: DiagnosticCategory::Protocol,
            kind: None,
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

    #[must_use]
    pub fn host(code: &str, message: impl Into<String>) -> Self {
        Self {
            version: 1,
            category: DiagnosticCategory::Host,
            kind: None,
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

    #[must_use]
    pub fn from_source(error: &SourceError) -> Self {
        Self {
            version: 1,
            category: DiagnosticCategory::Source,
            kind: Some(
                match error.kind {
                    SourceErrorKind::Parse => "parse",
                    SourceErrorKind::Semantic => "semantic",
                }
                .into(),
            ),
            code: error.code.as_str().into(),
            message: error.message.clone(),
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
            frames: Vec::new(),
            thrown: None,
            native: None,
            cause: None,
        }
    }

    #[must_use]
    pub fn from_runtime(error: &RuntimeError) -> Self {
        Self {
            version: 1,
            category: DiagnosticCategory::Runtime,
            kind: Some(runtime_kind(&error.kind).into()),
            code: runtime_kind(&error.kind).into(),
            message: error.message.clone(),
            location: error.span.as_ref().map(location),
            range: None,
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

fn runtime_kind(kind: &RuntimeErrorKind) -> &'static str {
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
