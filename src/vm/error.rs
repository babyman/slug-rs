use std::fmt;

use crate::{SourceSpan, Value};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeErrorKind {
    InvalidBytecode,
    Type,
    Name,
    Arity,
    DivideByZero,
    InvalidCall,
    Native,
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
    pub cause: Option<Box<RuntimeError>>,
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

pub(super) fn fault_type(kind: &RuntimeErrorKind) -> &'static str {
    match kind {
        RuntimeErrorKind::InvalidBytecode => "invalid_bytecode",
        RuntimeErrorKind::Type => "type",
        RuntimeErrorKind::Name => "name",
        RuntimeErrorKind::Arity => "arity",
        RuntimeErrorKind::DivideByZero => "divide_by_zero",
        RuntimeErrorKind::InvalidCall => "invalid_call",
        RuntimeErrorKind::Native => "native",
        RuntimeErrorKind::Match => "match",
        RuntimeErrorKind::Thrown => "thrown",
    }
}
