fn main() {
    compile_blink_artifact();
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_XTENSA_ESP32S3_NONE_ELF_LINKER");
    println!("cargo:rustc-link-arg=-Tlinkall.x");

    if cfg!(unix) && !linker_is_available() {
        panic!(
            "Xtensa linker `xtensa-esp32s3-elf-gcc` was not found. Source \
             $HOME/export-esp.sh after running `espup install`, then retry the build."
        );
    }
}

fn compile_blink_artifact() {
    use std::rc::Rc;

    use slug_frontend::ModuleHost;
    use slug_loader::StaticModuleResolver;
    use slug_vm::{EmptyVmConfiguration, NativeArity, NativeModule, VmHost, cslug};

    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"),
    );
    let source_path = manifest.join("programs/blink.slug");
    let contracts = manifest.join("../../clutch/slug.embedded.clutch/modules");
    let gpio_path = contracts.join("gpio.slug");
    let time_path = contracts.join("time.slug");
    let builtin_path = contracts.join("builtin.slug");
    for path in [&source_path, &gpio_path, &time_path, &builtin_path] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let source = read_source(&source_path);
    let providers = [
        provider("slug.builtin", &builtin_path),
        provider("slug.gpio", &gpio_path),
        provider("slug.time", &time_path),
    ];
    let resolver = StaticModuleResolver::new(providers).expect("embedded providers are distinct");
    let host = ModuleHost::with_resolver(Rc::new(resolver), Rc::new(EmptyVmConfiguration));
    let builtins = NativeModule::new("slug.builtin", ()).expect("static builtin module is valid");
    host.define_foreign_batch(vec![
        builtins
            .function("print", NativeArity::Variadic { minimum: 0 }, build_builtin)
            .expect("print descriptor is valid"),
        builtins
            .function(
                "println",
                NativeArity::Variadic { minimum: 0 },
                build_builtin,
            )
            .expect("println descriptor is valid"),
        builtins
            .function("len", NativeArity::Exact(1), build_builtin)
            .expect("len descriptor is valid"),
    ])
    .expect("builtin descriptors are distinct");
    let artifact = host
        .compile_artifact(&source_path.to_string_lossy(), &source)
        .unwrap_or_else(|error| panic!("cannot compile {}: {error}", source_path.display()));
    let bytes = cslug::encode(&artifact).expect("embedded artifact is encodable");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"))
        .join("slug_blink.cslug");
    std::fs::write(output, bytes).expect("writes embedded blink artifact");
}

fn build_builtin(call: &mut slug_vm::NativeCall<'_>) -> slug_vm::NativeStatus {
    call.return_value(slug_vm::NativeOwnedValue::nil())
}

fn provider(name: &str, path: &std::path::Path) -> slug_loader::StaticModuleProvider {
    slug_loader::StaticModuleProvider::new(
        name,
        slug_loader::ModuleSource {
            key: slug_loader::ModuleKey::new(format!("static:{name}")),
            diagnostic_name: path.display().to_string(),
            text: read_source(path),
            activation: None,
        },
    )
    .unwrap_or_else(|error| panic!("invalid provider {name}: {error}"))
}

fn read_source(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn linker_is_available() -> bool {
    std::process::Command::new("xtensa-esp32s3-elf-gcc")
        .arg("--version")
        .output()
        .is_ok()
}
