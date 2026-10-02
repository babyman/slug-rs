//! In-memory restricted-host assembly.
//!
//! This crate intentionally depends on the frontend, VM, and deny-all loader
//! only. It does not select desktop roots or discover process configuration.

use std::{collections::HashMap, rc::Rc};

use slug_frontend::{ModuleHost, SourceError};
use slug_nil_loader::NilLoader;
use slug_vm::{RuntimeError, Value, Vm, VmConfiguration};

/// Explicit in-memory configuration for the restricted host.
#[derive(Clone, Debug, Default)]
pub struct RestrictedConfiguration {
    values: HashMap<String, Value>,
}

impl RestrictedConfiguration {
    /// Creates configuration from preselected Slug values.
    #[must_use]
    pub fn from_values(values: impl IntoIterator<Item = (String, Value)>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

impl VmConfiguration for RestrictedConfiguration {
    fn resolve(&self, key: &str, fallback: &Value) -> Value {
        self.values
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback.clone())
    }

    fn arguments(&self) -> &[String] {
        &[]
    }

    fn argument_map(&self) -> Value {
        Value::Map(
            vec![
                (Value::string("options"), Value::Map(Vec::new().into())),
                (Value::string("positional"), Value::list(Vec::new())),
            ]
            .into(),
        )
    }
}

/// Evaluates in-memory source with no external-import capability.
///
/// # Errors
///
/// Returns checked source or runtime failures from the selected frontend and
/// VM; an external import is denied by [`NilLoader`].
pub fn evaluate(source: &str) -> Result<Value, RestrictedHostError> {
    evaluate_with_configuration(source, RestrictedConfiguration::default())
}

/// Evaluates in-memory source with an explicitly supplied host configuration.
///
/// # Errors
///
/// Returns checked source or runtime failures from the selected frontend and
/// VM; an external import is denied by [`NilLoader`].
pub fn evaluate_with_configuration(
    source: &str,
    configuration: RestrictedConfiguration,
) -> Result<Value, RestrictedHostError> {
    let host = ModuleHost::with_resolver(Rc::new(NilLoader), Rc::new(configuration));
    let program = host
        .compile_source("memory:entry", source)
        .map_err(RestrictedHostError::Source)?;
    Vm::with_host(Rc::new(host))
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

    use super::{RestrictedConfiguration, evaluate, evaluate_with_configuration};

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

    #[test]
    fn uses_only_explicitly_supplied_configuration() {
        let configuration =
            RestrictedConfiguration::from_values([("feature.enabled".into(), Value::Bool(true))]);

        assert_eq!(
            evaluate_with_configuration("cfg(\"feature.enabled\", false)\n", configuration)
                .expect("evaluate source with explicit configuration"),
            Value::Bool(true)
        );
    }
}
