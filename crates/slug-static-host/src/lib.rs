//! Source contracts for modules selected by a static Slug host.
//!
//! This crate does not implement native capabilities. It gives firmware build
//! tooling and desktop simulation the same source-visible module declarations;
//! the selected host owns callback registration and physical effects.

use slug_loader::{ModuleKey, ModuleLoadError, ModuleSource, StaticModuleProvider};

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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use slug_frontend::ModuleHost;
    use slug_loader::StaticModuleResolver;
    use slug_vm::{EmptyVmConfiguration, Vm};

    use super::{gpio_provider, time_provider};

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
}
