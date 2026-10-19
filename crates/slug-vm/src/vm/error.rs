use alloc::{
    borrow::ToOwned,
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, fmt::Write as _};

use crate::{SourceDiagnostic, SourceSpan, Value};

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
    ModuleNotFound,
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
    /// For an error raised by an import, why the module could not be
    /// imported, when that is known.
    pub import_failure: Option<Box<ImportFailure>>,
}

/// Why an import failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportFailure {
    /// The module's source does not compile.
    Compile(SourceDiagnostic),
    /// No module has the name.
    NotFound(crate::MissingModule),
}

impl RuntimeError {
    /// The source error of an imported module that failed to compile.
    #[must_use]
    pub fn compile_error(&self) -> Option<&SourceDiagnostic> {
        match self.import_failure.as_deref() {
            Some(ImportFailure::Compile(error)) => Some(error),
            _ => None,
        }
    }

    /// The name of a module an import did not find, and the names it most
    /// likely meant.
    #[must_use]
    pub fn missing_module(&self) -> Option<&crate::MissingModule> {
        match self.import_failure.as_deref() {
            Some(ImportFailure::NotFound(missing)) => Some(missing),
            _ => None,
        }
    }
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

#[cfg(not(target_os = "none"))]
impl std::error::Error for RuntimeError {}

pub(super) fn render_stacktrace(error: &RuntimeError) -> String {
    let mut rendered = String::new();
    render_stacktrace_error(&mut rendered, error, 0);
    rendered
}

fn render_stacktrace_error(rendered: &mut String, error: &RuntimeError, indent: usize) {
    let padding = " ".repeat(indent);
    let frame_padding = " ".repeat(indent + 2);
    write!(rendered, "{padding}runtime error: {}", error.message)
        .expect("writing to a string cannot fail");
    if let Some(span) = &error.span {
        write!(
            rendered,
            "\n{frame_padding}at {}:{}:{}",
            span.path, span.line, span.column
        )
        .expect("writing to a string cannot fail");
    }
    for frame in &error.frames {
        write!(rendered, "\n{frame_padding}in {}", frame.function)
            .expect("writing to a string cannot fail");
        if let Some(span) = &frame.span {
            write!(rendered, " at {}:{}:{}", span.path, span.line, span.column)
                .expect("writing to a string cannot fail");
        }
    }
    if let Some(cause) = &error.cause {
        write!(rendered, "\n\n{padding}caused by:\n").expect("writing to a string cannot fail");
        render_stacktrace_error(rendered, cause, indent + 2);
    }
}

impl Vm {
    // Errors leave the hot path; keeping their construction out of line
    // stops it from bloating the dispatch code that calls it.
    #[allow(clippy::needless_pass_by_value)]
    #[cold]
    #[inline(never)]
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
                .enumerate()
                .rev()
                .filter(|(_, frame)| !frame.cleanup_action)
                .map(|(index, frame)| CallFrame {
                    function: frame
                        .program
                        .chunk(frame.closure.chunk)
                        .map_or_else(|| "<invalid frame>".to_owned(), |chunk| chunk.name.clone()),
                    span: match &frame.call_span {
                        Some(super::frames::CallSpan::Instruction(span)) => self
                            .frames
                            .get(index.saturating_sub(1))
                            .and_then(|caller| caller.program.span(*span))
                            .cloned(),
                        #[cfg(feature = "concurrency")]
                        Some(super::frames::CallSpan::Owned(span)) => Some((**span).clone()),
                        None => None,
                    },
                })
                .collect(),
            cause: None,
            import_failure: None,
        }
    }

    /// A module error for a failed import, keeping the source error of a
    /// module that failed to compile.
    #[cold]
    #[inline(never)]
    pub(super) fn import_error(
        &self,
        error: &crate::VmHostError,
        span: Option<SourceSpan>,
    ) -> RuntimeError {
        let (kind, failure) = if error.is_not_found() {
            (
                RuntimeErrorKind::ModuleNotFound,
                error.missing_module().cloned().map(ImportFailure::NotFound),
            )
        } else {
            (
                RuntimeErrorKind::Module,
                error.compile_error().cloned().map(ImportFailure::Compile),
            )
        };
        let mut runtime_error = self.error(kind, error.to_string(), span);
        runtime_error.import_failure = failure.map(Box::new);
        runtime_error
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
        Value::map(vec![
            (Value::string("type"), Value::string(error_type)),
            (Value::string("msg"), Value::string(error.message)),
            (
                Value::string("data"),
                error
                    .native
                    .and_then(|native| native.data)
                    .unwrap_or(Value::Nil),
            ),
        ])
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
        RuntimeErrorKind::ModuleNotFound => "module_not_found",
        RuntimeErrorKind::NotImplemented => "not_implemented",
        RuntimeErrorKind::Match => "match",
        RuntimeErrorKind::Thrown => "thrown",
    }
}
