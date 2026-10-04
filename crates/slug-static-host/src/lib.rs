//! Source contracts for modules selected by a static Slug host.
//!
//! This crate gives firmware build tooling and desktop simulation the same
//! source-visible module declarations. The desktop simulator below records
//! effects; a firmware host supplies the physical implementation separately.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use slug_frontend::{ModuleGraph, ModuleGraphHost, SourceError};
use slug_loader::{
    ModuleActivationTransaction, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver,
    ModuleSource, StaticModuleProvider, StaticModuleResolver,
};
use slug_vm::{
    EmptyVmConfiguration, NativeArity, NativeCall, NativeDescriptorError, NativeError,
    NativeFunction, NativeModule, NativeOwnedValue, NativeResourceRegistry, NativeStatus,
    NativeValueKind, Program, RuntimeError, Value, Vm, VmConfiguration, VmHost, VmHostError,
    VmModuleExports, native_resource_registry,
};

/// Source contract for a board-alias GPIO output capability.
pub const GPIO_MODULE_SOURCE: &str = r#"
/**
 * Writes a boolean level to a host-defined logical output.
 *
 * `name` identifies an alias selected by the host, such as `"led"`; it is not
 * a raw GPIO number. An unavailable alias is a checked host failure.
 */
export foreign write = fn(name:str, level:bool):nil
"#;

/// Source contract for a deliberately blocking delay capability.
pub const TIME_MODULE_SOURCE: &str = r"
/**
 * Blocks the current host execution for a whole number of milliseconds.
 *
 * The host rejects negative, fractional, non-finite, and out-of-range values
 * as checked failures. This function does not schedule a Slug timer.
 */
export foreign sleepMs = fn(milliseconds:num):nil
";

/// Returns the selected provider for `slug.gpio`.
///
/// # Errors
///
/// Returns a checked error only if the crate's fixed module identity is
/// invalid.
pub fn gpio_provider() -> Result<StaticModuleProvider, ModuleLoadError> {
    provider("slug.gpio", GPIO_MODULE_SOURCE)
}

/// Returns the selected provider for `slug.time`.
///
/// # Errors
///
/// Returns a checked error only if the crate's fixed module identity is
/// invalid.
pub fn time_provider() -> Result<StaticModuleProvider, ModuleLoadError> {
    provider("slug.time", TIME_MODULE_SOURCE)
}

fn provider(name: &str, source: &str) -> Result<StaticModuleProvider, ModuleLoadError> {
    StaticModuleProvider::new(
        name,
        ModuleSource {
            key: ModuleKey::new(format!("static:{name}")),
            diagnostic_name: format!("static:{name}"),
            text: source.into(),
            activation: None,
        },
    )
}

/// One GPIO output change observed by [`StaticHost`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpioWrite {
    /// Logical host-selected output alias, never a raw GPIO number.
    pub name: String,
    /// Level requested by the Slug program.
    pub level: bool,
}

/// Deterministic desktop host for the selected static GPIO and time modules.
///
/// It resolves no files and loads no dynamic libraries. Native callbacks record
/// output changes and delay requests, making it suitable for testing the same
/// source contracts a board-specific host will later implement.
#[derive(Clone)]
pub struct StaticHost {
    state: Rc<StaticHostState>,
}

struct StaticHostState {
    resolver: StaticModuleResolver,
    graph: ModuleGraph,
    configuration: EmptyVmConfiguration,
    foreign_functions: RefCell<HashMap<(String, String), NativeFunction>>,
    native_globals: RefCell<HashMap<String, Value>>,
    native_resources: NativeResourceRegistry,
    warnings: RefCell<Vec<String>>,
    simulation: SimulationState,
    gpio_functions: Vec<NativeFunction>,
    time_functions: Vec<NativeFunction>,
}

#[derive(Clone)]
struct SimulationState {
    output_aliases: Rc<HashSet<String>>,
    maximum_sleep_ms: u32,
    writes: Rc<RefCell<Vec<GpioWrite>>>,
    sleeps: Rc<RefCell<Vec<u32>>>,
}

impl StaticHost {
    /// Creates a host with the selected GPIO aliases and maximum delay.
    ///
    /// # Errors
    ///
    /// Returns a checked error when the fixed module or function descriptors
    /// cannot be constructed.
    pub fn with_gpio_and_time(
        output_aliases: impl IntoIterator<Item = impl Into<String>>,
        maximum_sleep_ms: u32,
    ) -> Result<Self, StaticHostBuildError> {
        let simulation = SimulationState {
            output_aliases: Rc::new(output_aliases.into_iter().map(Into::into).collect()),
            maximum_sleep_ms,
            writes: Rc::new(RefCell::new(Vec::new())),
            sleeps: Rc::new(RefCell::new(Vec::new())),
        };
        let gpio_module = NativeModule::new("slug.gpio", simulation.clone())?;
        let time_module = NativeModule::new("slug.time", simulation.clone())?;
        let gpio_functions =
            vec![gpio_module.function("write", NativeArity::Exact(2), gpio_write)?];
        let time_functions =
            vec![time_module.function("sleepMs", NativeArity::Exact(1), sleep_ms)?];
        let resolver = StaticModuleResolver::new([gpio_provider()?, time_provider()?])?;

        Ok(Self {
            state: Rc::new(StaticHostState {
                resolver,
                graph: ModuleGraph::new(),
                configuration: EmptyVmConfiguration,
                foreign_functions: RefCell::new(HashMap::new()),
                native_globals: RefCell::new(HashMap::new()),
                native_resources: native_resource_registry(),
                warnings: RefCell::new(Vec::new()),
                simulation,
                gpio_functions,
                time_functions,
            }),
        })
    }

    /// Compiles entry source against the selected static module contracts.
    ///
    /// # Errors
    ///
    /// Returns a checked source error when the program is invalid.
    pub fn compile_source(&self, path: &str, source: &str) -> Result<Program, SourceError> {
        self.state.graph.compile_source(self, path, source)
    }

    /// Compiles and executes one entry program through this host.
    ///
    /// # Errors
    ///
    /// Returns a checked source or runtime error from the selected program.
    pub fn run_source(&self, path: &str, source: &str) -> Result<Value, StaticHostError> {
        let program = self.compile_source(path, source)?;
        Vm::with_host(Rc::new(self.clone()))
            .run_named(&program, "main")
            .map_err(StaticHostError::Runtime)
    }

    /// Returns GPIO writes observed since the last [`Self::take_gpio_writes`].
    #[must_use]
    pub fn take_gpio_writes(&self) -> Vec<GpioWrite> {
        std::mem::take(&mut *self.state.simulation.writes.borrow_mut())
    }

    /// Returns delay requests observed since the last [`Self::take_sleep_requests`].
    #[must_use]
    pub fn take_sleep_requests(&self) -> Vec<u32> {
        std::mem::take(&mut *self.state.simulation.sleeps.borrow_mut())
    }
}

/// Checked failure while constructing [`StaticHost`].
#[derive(Debug)]
pub enum StaticHostBuildError {
    /// Static module provider setup failed.
    Module(ModuleLoadError),
    /// Native module descriptor setup failed.
    Descriptor(NativeDescriptorError),
}

impl From<ModuleLoadError> for StaticHostBuildError {
    fn from(error: ModuleLoadError) -> Self {
        Self::Module(error)
    }
}

impl From<NativeDescriptorError> for StaticHostBuildError {
    fn from(error: NativeDescriptorError) -> Self {
        Self::Descriptor(error)
    }
}

impl std::fmt::Display for StaticHostBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(error) => error.fmt(formatter),
            Self::Descriptor(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StaticHostBuildError {}

/// Checked failure while compiling or executing through [`StaticHost`].
#[derive(Debug)]
pub enum StaticHostError {
    /// Source validation failure.
    Source(SourceError),
    /// VM execution failure.
    Runtime(RuntimeError),
}

impl From<SourceError> for StaticHostError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

impl std::fmt::Display for StaticHostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(error) => error.fmt(formatter),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StaticHostError {}

impl ModuleResolver for StaticHost {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        self.state.resolver.resolve(request)
    }
}

impl ModuleGraphHost for StaticHost {
    fn stage_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<Box<dyn ModuleActivationTransaction>>, ModuleLoadError> {
        let functions = match source.key.as_str() {
            "static:slug.gpio" => self.state.gpio_functions.clone(),
            "static:slug.time" => self.state.time_functions.clone(),
            _ => return Ok(None),
        };
        Ok(Some(Box::new(StaticActivation {
            state: self.state.clone(),
            functions,
            registered: Cell::new(false),
        })))
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    fn module_vm(&self, bindings: &[String]) -> Vm {
        let host: Rc<dyn VmHost> = Rc::new(self.clone());
        Vm::with_module_bindings(&host, bindings)
    }
}

impl VmHost for StaticHost {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let importer = importer.map(ModuleKey::new);
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
        ModuleGraphHost::builtin_globals(self)
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        self.state.native_globals.borrow().clone()
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        self.state
            .foreign_functions
            .borrow()
            .get(&(module.into(), name.into()))
            .cloned()
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        define_foreign_batch(&self.state.foreign_functions, functions)
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.state.native_globals.borrow_mut().insert(name, value);
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.state.native_resources.clone()
    }

    fn configuration(&self) -> &dyn VmConfiguration {
        &self.state.configuration
    }

    fn warn(&self, message: String) {
        self.state.warnings.borrow_mut().push(message);
    }

    fn shutdown(&self) {
        let _ = self.state.native_resources.finalize_all_for_shutdown();
    }
}

struct StaticActivation {
    state: Rc<StaticHostState>,
    functions: Vec<NativeFunction>,
    registered: Cell<bool>,
}

impl ModuleActivationTransaction for StaticActivation {
    fn register(&self) -> Result<(), ModuleLoadError> {
        define_foreign_batch(&self.state.foreign_functions, self.functions.clone()).map_err(
            |error| ModuleLoadError::Clutch {
                location: "static host".into(),
                message: error.to_string(),
            },
        )?;
        self.registered.set(true);
        Ok(())
    }

    fn rollback(&self) {
        if !self.registered.replace(false) {
            return;
        }
        let mut registry = self.state.foreign_functions.borrow_mut();
        for function in &self.functions {
            registry.remove(&(function.module_name().into(), function.name().into()));
        }
    }

    fn cleanup(&mut self) {
        self.rollback();
    }

    fn retain(self: Box<Self>) {}
}

fn define_foreign_batch(
    registry: &RefCell<HashMap<(String, String), NativeFunction>>,
    functions: Vec<NativeFunction>,
) -> Result<(), NativeDescriptorError> {
    let keys = functions
        .iter()
        .map(|function| {
            (
                function.module_name().to_string(),
                function.name().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut registered = registry.borrow_mut();
    for (index, key) in keys.iter().enumerate() {
        if registered.contains_key(key) || keys[..index].contains(key) {
            return Err(NativeDescriptorError::new(format!(
                "foreign binding `{}.{}` is already defined",
                key.0, key.1
            )));
        }
    }
    for (key, function) in keys.into_iter().zip(functions) {
        registered.insert(key, function);
    }
    Ok(())
}

fn gpio_write(call: &mut NativeCall<'_>) -> NativeStatus {
    let name = match call.argument(0).and_then(slug_vm::NativeValueRef::as_str) {
        Ok(name) => name.to_string(),
        Err(error) => return call.raise(error),
    };
    let level = match call.argument(1).and_then(slug_vm::NativeValueRef::as_bool) {
        Ok(level) => level,
        Err(error) => return call.raise(error),
    };
    let Some(state) = call.state::<SimulationState>() else {
        return call
            .report_contract_violation("static GPIO callback has incompatible module state");
    };
    if !state.output_aliases.contains(&name) {
        return call.raise(NativeError::new(
            "gpio.alias",
            format!("GPIO output alias `{name}` is unavailable"),
        ));
    }
    state.writes.borrow_mut().push(GpioWrite { name, level });
    call.return_value(NativeOwnedValue::nil())
}

fn sleep_ms(call: &mut NativeCall<'_>) -> NativeStatus {
    let milliseconds = match call.argument(0).and_then(milliseconds_from_value) {
        Ok(milliseconds) => milliseconds,
        Err(error) => return call.raise(error),
    };
    let Some(state) = call.state::<SimulationState>() else {
        return call
            .report_contract_violation("static time callback has incompatible module state");
    };
    if milliseconds > state.maximum_sleep_ms {
        return call.raise(NativeError::new(
            "time.range",
            format!(
                "sleep duration {milliseconds}ms exceeds host maximum {}ms",
                state.maximum_sleep_ms
            ),
        ));
    }
    state.sleeps.borrow_mut().push(milliseconds);
    call.return_value(NativeOwnedValue::nil())
}

fn milliseconds_from_value(value: slug_vm::NativeValueRef<'_>) -> Result<u32, NativeError> {
    match value.kind() {
        NativeValueKind::Int => u32::try_from(value.as_i64()?).map_err(|_| {
            NativeError::new(
                "time.milliseconds",
                "sleep duration must be a whole number of milliseconds",
            )
        }),
        NativeValueKind::Float => milliseconds_from_float(value.as_f64()?),
        _ => Err(NativeError::new(
            "time.milliseconds",
            "sleep duration must be a whole number of milliseconds",
        )),
    }
}

fn milliseconds_from_float(milliseconds: f64) -> Result<u32, NativeError> {
    if !milliseconds.is_finite()
        || milliseconds < 0.0
        || milliseconds.fract() != 0.0
        || milliseconds > f64::from(u32::MAX)
    {
        return Err(NativeError::new(
            "time.milliseconds",
            "sleep duration must be a whole number of milliseconds",
        ));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(milliseconds as u32)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use slug_frontend::ModuleHost;
    use slug_loader::StaticModuleResolver;
    use slug_vm::{EmptyVmConfiguration, Vm};

    use super::{GpioWrite, StaticHost, gpio_provider, time_provider};

    fn host_with(providers: Vec<slug_loader::StaticModuleProvider>) -> ModuleHost {
        ModuleHost::with_resolver(
            Rc::new(StaticModuleResolver::new(providers).expect("valid static providers")),
            Rc::new(EmptyVmConfiguration),
        )
    }

    #[test]
    fn imported_gpio_and_time_contracts_type_check() {
        let host = host_with(vec![
            gpio_provider().expect("gpio provider"),
            time_provider().expect("time provider"),
        ]);

        host.compile_source(
            "memory:entry",
            "val gpio = import(\"slug.gpio\")\n\
             val time = import(\"slug.time\")\n\
             gpio.write(\"led\", true)\n\
             time.sleepMs(250)\n",
        )
        .expect("static module call signatures type check");
    }

    #[test]
    fn gpio_level_must_be_boolean() {
        let host = host_with(vec![gpio_provider().expect("gpio provider")]);

        let error = host
            .compile_source(
                "memory:entry",
                "val gpio = import(\"slug.gpio\")\ngpio.write(\"led\", \"on\")\n",
            )
            .expect_err("string level must fail semantic checking");
        assert!(error.to_string().contains("bool"));
    }

    #[test]
    fn unselected_static_modules_fail_as_checked_import_errors() {
        let host = host_with(vec![gpio_provider().expect("gpio provider")]);
        let program = host
            .compile_source("memory:entry", "import(\"slug.time\")\n")
            .expect("compile dynamic import");

        let error = Vm::with_host(Rc::new(host))
            .run_named(&program, "main")
            .expect_err("unselected module must fail");
        assert!(
            error
                .to_string()
                .contains("module `slug.time` was not found")
        );
    }

    #[test]
    fn simulator_records_one_gpio_blink_cycle() {
        let host = StaticHost::with_gpio_and_time(["led"], 1_000)
            .expect("fixed simulator host setup succeeds");

        host.run_source(
            "memory:blink",
            "val gpio = import(\"slug.gpio\")\n\
             val time = import(\"slug.time\")\n\
             gpio.write(\"led\", true)\n\
             time.sleepMs(250)\n\
             gpio.write(\"led\", false)\n\
             time.sleepMs(250)\n",
        )
        .expect("one blink cycle executes through static module contracts");

        assert_eq!(
            host.take_gpio_writes(),
            vec![
                GpioWrite {
                    name: "led".into(),
                    level: true,
                },
                GpioWrite {
                    name: "led".into(),
                    level: false,
                },
            ]
        );
        assert_eq!(host.take_sleep_requests(), vec![250, 250]);
    }

    #[test]
    fn simulator_rejects_an_unselected_output_alias() {
        let host = StaticHost::with_gpio_and_time(["led"], 1_000)
            .expect("fixed simulator host setup succeeds");

        let error = host
            .run_source(
                "memory:unknown-output",
                "val gpio = import(\"slug.gpio\")\ngpio.write(\"unknown\", true)\n",
            )
            .expect_err("unselected output alias must be a checked runtime failure");

        assert!(
            error
                .to_string()
                .contains("GPIO output alias `unknown` is unavailable")
        );
    }
}
