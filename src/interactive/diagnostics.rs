use serde::Serialize;

use crate::{RuntimeError, RuntimeErrorKind, SourceError, SourceErrorKind, SourceSpan, Value};

/// A versioned, wire-safe projection of a Slug or server failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub version: u8,
    pub category: DiagnosticCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
    pub frames: Vec<DiagnosticFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thrown: Option<ValueSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<Self>>,
}

/// The origin of a protocol diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Source,
    Runtime,
    Protocol,
    Host,
}

/// A source coordinate preserved in a diagnostic projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiagnosticLocation {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

/// One Slug call frame in a runtime diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiagnosticFrame {
    pub function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
}

/// A lossless-for-display summary for values that cannot yet be protocol values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ValueSummary {
    pub kind: String,
    pub display: String,
}

/// Structured native error details retained by runtime diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NativeDiagnostic {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ValueSummary>,
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
            code: "source_error".into(),
            message: error.message.clone(),
            location: error.span.as_ref().map(location),
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
            code: "runtime_error".into(),
            message: error.message.clone(),
            location: error.span.as_ref().map(location),
            frames: error
                .frames
                .iter()
                .map(|frame| DiagnosticFrame {
                    function: frame.function.clone(),
                    location: frame.span.as_ref().map(location),
                })
                .collect(),
            thrown: error.thrown.as_deref().map(value_summary),
            native: error.native.as_deref().map(|native| NativeDiagnostic {
                code: native.code.clone(),
                data: native.data.as_ref().map(value_summary),
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

fn value_summary(value: &Value) -> ValueSummary {
    ValueSummary {
        kind: value_kind(value).into(),
        display: value.to_string(),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Uninitialized | Value::Binding { .. } => "binding",
        Value::Nil => "nil",
        Value::Bool(_) => "bool",
        Value::Int(_) | Value::Float(_) => "num",
        Value::Str(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::List(_) => "list",
        Value::Map(_) => "map",
        Value::StructSchema(_) => "struct_schema",
        Value::Struct(_) => "struct",
        Value::Enum(_) => "enum",
        Value::Channel(_) => "chan",
        Value::Closure(_) | Value::Native(_) | Value::DeclaredNative { .. } | Value::Builtin(_) => {
            "fn"
        }
        #[cfg(feature = "concurrency")]
        Value::Task(_) => "task",
        Value::NativeResource(_) => "native_resource",
        Value::Overloads(_) => "overloads",
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
