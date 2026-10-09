use std::rc::Rc;

use slug_frontend::ModuleHost;
use slug_loader::{ModuleKey, ModuleSource, StaticModuleProvider, StaticModuleResolver};
use slug_vm::{
    EmptyVmConfiguration, NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus,
    Value, cslug,
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
    let artifact = host([
        (
            "slug.gpio",
            include_str!("../../../clutch/slug.embedded.clutch/modules/gpio.slug"),
        ),
        (
            "slug.time",
            include_str!("../../../clutch/slug.embedded.clutch/modules/time.slug"),
        ),
    ])
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
                modules: 3,
                imports: 2,
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
