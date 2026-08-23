use std::{fmt, rc::Rc};

use crate::{SourceSpan, Value};

use super::Vm;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeErrorKind {
    InvalidBytecode,
    Type,
    Name,
    Arity,
    DivideByZero,
    InvalidCall,
    Native,
    NativeContract,
    Module,
    NotImplemented,
    Match,
    Thrown,
}

/// A Slug-level runtime error, never a host panic.
#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub frames: Vec<CallFrame>,
    pub thrown: Option<Box<Value>>,
    pub native: Option<Box<NativeErrorDetails>>,
    pub cause: Option<Box<RuntimeError>>,
}

#[derive(Clone, Debug)]
pub struct NativeErrorDetails {
    pub code: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallFrame {
    pub function: String,
    pub span: Option<SourceSpan>,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "runtime error: {}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at {}:{}:{}", span.path, span.line, span.column)?;
        }
        for frame in &self.frames {
            write!(f, "\n  in {}", frame.function)?;
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

impl Vm {
    #[allow(clippy::needless_pass_by_value)]
    pub(super) fn error(
        &self,
        kind: RuntimeErrorKind,
        message: String,
        span: Option<SourceSpan>,
    ) -> RuntimeError {
        RuntimeError {
            kind,
            message,
            span: span.clone(),
            thrown: None,
            native: None,
            frames: self
                .frames
                .iter()
                .rev()
                .filter(|frame| !frame.cleanup_action)
                .map(|frame| CallFrame {
                    function: frame.function.clone(),
                    span: frame.call_span.clone(),
                })
                .collect(),
            cause: None,
        }
    }

    pub(super) fn thrown(&self, value: Value, span: Option<SourceSpan>) -> RuntimeError {
        let mut error = self.error(
            RuntimeErrorKind::Thrown,
            format!("uncaught throw: {value}"),
            span,
        );
        error.thrown = Some(Box::new(value));
        error
    }

    pub(super) fn error_value(error: RuntimeError) -> Value {
        if let Some(value) = error.thrown {
            return *value;
        }
        let error_type = error
            .native
            .as_ref()
            .map_or_else(|| fault_type(&error.kind), |native| native.code.as_str());
        Value::Map(Rc::new(vec![
            (Value::string("type"), Value::string(error_type)),
            (Value::string("msg"), Value::string(error.message)),
            (
                Value::string("data"),
                error
                    .native
                    .and_then(|native| native.data)
                    .unwrap_or(Value::Nil),
            ),
        ]))
    }
}

fn fault_type(kind: &RuntimeErrorKind) -> &'static str {
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
