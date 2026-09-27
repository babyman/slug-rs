//! In-memory restricted-host assembly.
//!
//! This crate intentionally depends on the frontend, VM, and deny-all loader
//! only. It does not select desktop roots or discover process configuration.

use std::rc::Rc;

use slug_frontend::{ModuleLoader, SourceError};
use slug_nil_loader::NilLoader;
use slug_vm::{Configuration, RuntimeError, Value, Vm};

/// Evaluates in-memory source with no external-import capability.
///
/// # Errors
///
/// Returns checked source or runtime failures from the selected frontend and
/// VM; an external import is denied by [`NilLoader`].
pub fn evaluate(source: &str) -> Result<Value, RestrictedHostError> {
    let loader = ModuleLoader::with_resolver(Rc::new(NilLoader), Configuration::default());
    let program = loader
        .compile_source("memory:entry", source)
        .map_err(RestrictedHostError::Source)?;
    Vm::with_host(Rc::new(loader))
        .run_named(&program, "main")
        .map_err(RestrictedHostError::Runtime)
}

/// Checked failure from restricted in-memory evaluation.
#[derive(Debug)]
pub enum RestrictedHostError {
    Source(SourceError),
    Runtime(RuntimeError),
}

impl std::fmt::Display for RestrictedHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RestrictedHostError {}

#[cfg(test)]
mod tests {
    use slug_vm::Value;

    use super::evaluate;

    #[test]
    fn evaluates_in_memory_source_without_external_capabilities() {
        assert_eq!(
            evaluate("40 + 2\n").expect("evaluate source"),
            Value::Int(42)
        );
    }

    #[test]
    fn rejects_external_imports_through_a_checked_runtime_error() {
        let error = evaluate("import(\"outside.module\")\n")
            .expect_err("restricted host must reject external imports");
        assert!(
            error
                .to_string()
                .contains("module `outside.module` was not found")
        );
    }
}
