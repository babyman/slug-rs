//! In-memory restricted-host assembly.
//!
//! This crate intentionally depends on the frontend, VM, and deny-all loader
//! only. It does not select desktop roots or discover process configuration.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use slug_frontend::{ModuleGraph, ModuleGraphHost, SourceError};
use slug_loader::{ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource};
use slug_nil_loader::NilLoader;
use slug_vm::{
    Configuration, NativeDescriptorError, NativeFunction, NativeResourceRegistry, RuntimeError,
    Value, Vm, VmHost, VmHostError, VmModuleExports, native_resource_registry,
};

#[derive(Clone)]
struct RestrictedHost {
    state: Rc<RestrictedHostState>,
}

struct RestrictedHostState {
    graph: ModuleGraph,
    configuration: Configuration,
    native_globals: RefCell<HashMap<String, Value>>,
    native_resources: NativeResourceRegistry,
}

impl RestrictedHost {
    fn new() -> Self {
        Self {
            state: Rc::new(RestrictedHostState {
                graph: ModuleGraph::new(),
                configuration: Configuration::default(),
                native_globals: RefCell::new(HashMap::new()),
                native_resources: native_resource_registry(),
            }),
        }
    }
}

impl ModuleResolver for RestrictedHost {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        NilLoader.resolve(request)
    }
}

impl ModuleGraphHost for RestrictedHost {
    type Activation = ();

    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn stage_module_activation(&self, _: &ModuleSource) -> Result<Option<()>, ModuleLoadError> {
        Ok(None)
    }

    fn register_module_activation(&self, _: &ModuleSource, (): &()) -> Result<(), ModuleLoadError> {
        Ok(())
    }

    fn remove_module_activation(&self, (): &()) {}

    fn cleanup_module_activation(&self, _: &mut Option<()>) {}

    fn retain_module_activation(&self, _: &ModuleSource, (): ()) {}

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: Rc<dyn VmHost> = Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for RestrictedHost {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let importer = importer.map(slug_loader::ModuleKey::new);
        let instance = self
            .state
            .graph
            .initialize(self, ModuleRequest::new(importer.as_ref(), name))
            .map_err(|error| match error {
                ModuleLoadError::NotFound { .. } => VmHostError::not_found(error.to_string()),
                error => VmHostError::new(error.to_string()),
            })?;
        Ok(VmModuleExports {
            exports: instance.live_exports(),
        })
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }
    fn native_globals(&self) -> HashMap<String, Value> {
        self.state.native_globals.borrow().clone()
    }
    fn foreign_function(&self, _: &str, _: &str) -> Option<NativeFunction> {
        None
    }
    fn define_foreign_batch(&self, _: Vec<NativeFunction>) -> Result<(), NativeDescriptorError> {
        Ok(())
    }
    fn define_native_global(&self, name: String, value: Value) {
        self.state.native_globals.borrow_mut().insert(name, value);
    }
    fn native_resources(&self) -> NativeResourceRegistry {
        self.state.native_resources.clone()
    }
    fn configuration(&self) -> &Configuration {
        &self.state.configuration
    }
    fn warn(&self, _: String) {}
    fn shutdown(&self) {
        let _ = self.state.native_resources.finalize_all_for_shutdown();
    }
}

/// Evaluates in-memory source with no external-import capability.
///
/// # Errors
///
/// Returns checked source or runtime failures from the selected frontend and
/// VM; an external import is denied by [`NilLoader`].
pub fn evaluate(source: &str) -> Result<Value, RestrictedHostError> {
    let host = RestrictedHost::new();
    let program = host
        .state
        .graph
        .compile_source(&host, "memory:entry", source)
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
