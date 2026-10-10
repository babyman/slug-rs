use std::rc::Rc;

use slug_frontend::ModuleHost;
use slug_loader::{ModuleKey, ModuleSource, StaticModuleProvider, StaticModuleResolver};
use slug_vm::{
    EmptyVmConfiguration, NativeArity, NativeCall, NativeError, NativeModule, NativeOwnedValue,
    NativeStatus, NativeValueKind, Value, VmHost, cslug,
};

fn host(modules: impl IntoIterator<Item = (&'static str, &'static str)>) -> ModuleHost {
    let providers = modules
        .into_iter()
        .map(|(name, text)| {
            StaticModuleProvider::new(
                name,
                ModuleSource {
                    key: ModuleKey::new(format!("memory:{name}")),
                    diagnostic_name: format!("{name}.slug"),
                    text: text.into(),
                    activation: None,
                },
            )
            .expect("valid static provider")
        })
        .collect::<Vec<_>>();
    let resolver = StaticModuleResolver::new(providers).expect("valid resolver");
    ModuleHost::with_resolver(Rc::new(resolver), Rc::new(EmptyVmConfiguration))
}

fn returns_seven(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::integer(7))
}

fn stops_after_first_gpio(call: &mut NativeCall<'_>) -> NativeStatus {
    call.raise(NativeError::new(
        "test.stop",
        "light walk reached its first GPIO write",
    ))
}

fn returns_nil(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::nil())
}

fn embedded_len(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call.argument(0) {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    let length = match value.kind() {
        NativeValueKind::String => match value.as_str() {
            Ok(value) => value.chars().count(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::Bytes => match value.as_bytes() {
            Ok(value) => value.len(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::List | NativeValueKind::Map => {
            value.len().expect("collection kind has a length")
        }
        kind => {
            return call.raise(NativeError::new(
                "native.type",
                format!("`len` expects str, bytes, list, or map, got {kind:?}"),
            ));
        }
    };
    let length = i64::try_from(length).expect("test values fit in an integer");
    call.return_value(NativeOwnedValue::integer(length))
}

fn firmware_host() -> ModuleHost {
    let host = host([
        (
            "slug.builtin",
            include_str!("../../../clutch/slug.embedded.clutch/modules/builtin.slug"),
        ),
        (
            "slug.gpio",
            include_str!("../../../clutch/slug.embedded.clutch/modules/gpio.slug"),
        ),
        (
            "slug.time",
            include_str!("../../../clutch/slug.embedded.clutch/modules/time.slug"),
        ),
    ]);
    let builtins = NativeModule::new("slug.builtin", ()).expect("valid native module");
    host.define_foreign_batch(vec![
        builtins
            .function("print", NativeArity::Variadic { minimum: 0 }, returns_nil)
            .expect("valid print function"),
        builtins
            .function("println", NativeArity::Variadic { minimum: 0 }, returns_nil)
            .expect("valid println function"),
        builtins
            .function("len", NativeArity::Exact(1), embedded_len)
            .expect("valid len function"),
    ])
    .expect("register embedded builtin descriptors");
    host
}

fn register_embedded_builtins(host: &cslug::ArtifactHost) {
    let builtins = NativeModule::new("slug.builtin", ()).expect("valid native module");
    for function in [
        builtins
            .function("print", NativeArity::Variadic { minimum: 0 }, returns_nil)
            .expect("valid print function"),
        builtins
            .function("println", NativeArity::Variadic { minimum: 0 }, returns_nil)
            .expect("valid println function"),
        builtins
            .function("len", NativeArity::Exact(1), embedded_len)
            .expect("valid len function"),
    ] {
        host.register_builtin(function)
            .expect("register embedded builtin");
    }
}

#[test]
fn decoded_artifact_host_executes_closed_imports() {
    let artifact = host([("child", "export val value = 42\n")])
        .compile_artifact(
            "entry.slug",
            "val child = import(\"child\")\nval main = fn() { child.value }\n",
        )
        .expect("compile closed artifact");
    let bytes = cslug::encode(&artifact).expect("encode artifact");
    let artifact = cslug::decode(&bytes, cslug::Limits::default()).expect("decode artifact");

    assert_eq!(
        cslug::ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
            .expect("valid artifact")
            .run()
            .expect("execute closed import"),
        Value::Int(42)
    );
}

#[test]
fn artifact_host_rejects_imports_absent_from_its_graph() {
    let artifact = host([])
        .compile_artifact(
            "entry.slug",
            "val name = \"missing\"\nval main = fn() { import(name) }\n",
        )
        .expect("compile artifact with dynamic import");

    let error = cslug::ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
        .expect("valid artifact")
        .run()
        .expect_err("missing artifact edge is checked");
    assert!(error.to_string().contains("compiled artifact"));
}

#[test]
fn artifact_host_registers_static_module_qualified_foreign_functions() {
    let artifact = host([("slug.gpio", "export foreign write = fn():num\n")])
        .compile_artifact(
            "entry.slug",
            "val gpio = import(\"slug.gpio\")\nval main = fn() { gpio.write() }\n",
        )
        .expect("compile artifact");
    let artifact =
        cslug::ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration)).expect("valid artifact");
    let module = NativeModule::new("slug.gpio", ()).expect("valid native module");
    artifact
        .register_foreign(
            module
                .function("write", NativeArity::Exact(0), returns_seven)
                .expect("valid native function"),
        )
        .expect("register static provider");

    assert_eq!(
        artifact.run().expect("execute static provider"),
        Value::Int(7)
    );
}

#[test]
fn firmware_blink_graph_encodes_within_device_limits() {
    let artifact = firmware_host()
        .compile_artifact(
            "firmware/slug-esp32-s3/programs/blink.slug",
            include_str!("../../../firmware/slug-esp32-s3/programs/blink.slug"),
        )
        .expect("compile firmware blink graph");
    let bytes = cslug::encode(&artifact).expect("encode firmware blink graph");

    assert_eq!(
        cslug::decode(
            &bytes,
            cslug::Limits {
                artifact_bytes: 32 * 1024,
                section_bytes: 24 * 1024,
                modules: 4,
                imports: 3,
                string_bytes: 512,
                chunks_per_module: 32,
                entries_per_table: 512,
            },
        )
        .expect("firmware limits accept blink graph")
        .modules
        .len(),
        3
    );
}

#[test]
fn firmware_light_walk_reaches_its_first_gpio_write() {
    let artifact = firmware_host()
        .compile_artifact(
            "firmware/slug-esp32-s3/programs/blink.slug",
            include_str!("../../../firmware/slug-esp32-s3/programs/blink.slug"),
        )
        .expect("compile firmware light-walk graph");
    let artifact =
        cslug::ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration)).expect("valid artifact");
    register_embedded_builtins(&artifact);
    let module = NativeModule::new("slug.gpio", ()).expect("valid native module");
    artifact
        .register_foreign(
            module
                .function("write", NativeArity::Exact(2), stops_after_first_gpio)
                .expect("valid native function"),
        )
        .expect("register GPIO provider");
    let module = NativeModule::new("slug.time", ()).expect("valid native module");
    artifact
        .register_foreign(
            module
                .function("sleepMs", NativeArity::Exact(1), returns_nil)
                .expect("valid native function"),
        )
        .expect("register time provider");

    let error = artifact
        .run()
        .expect_err("light walk reaches the first GPIO callback");
    assert!(
        error
            .to_string()
            .contains("light walk reached its first GPIO write"),
        "unexpected light-walk failure: {error}"
    );
}

#[test]
fn firmware_artifact_executes_implicit_and_explicit_builtin_calls() {
    let artifact = firmware_host()
        .compile_artifact(
            "firmware-builtin.slug",
            "val core = import(\"slug.builtin\")\n\
             val main = fn() {\n\
                 print(\"implicit\")\n\
                 core.println(\"explicit\")\n\
                 len([1, 2]) + core.len(\"abc\")\n\
             }\n",
        )
        .expect("compile embedded builtin users");
    let artifact =
        cslug::ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration)).expect("valid artifact");
    register_embedded_builtins(&artifact);

    assert_eq!(
        artifact.run().expect("embedded builtin users execute"),
        Value::Int(5)
    );
}
