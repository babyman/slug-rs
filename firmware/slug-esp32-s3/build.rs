fn main() {
    compile_target_artifact();
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_XTENSA_ESP32S3_NONE_ELF_LINKER");
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    if cfg!(unix) && !linker_is_available() {
        panic!("Xtensa linker `xtensa-esp32s3-elf-gcc` was not found. Source $HOME/export-esp.sh after running `espup install`, then retry the build.");
    }
}

fn compile_target_artifact() {
    use slug_embedded_build::TargetDescriptor;
    use slug_vm::{NativeArity, NativeModule};

    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest"))
        .join("../..");
    println!("cargo:rerun-if-env-changed=SLUG_TARGET_DESCRIPTOR");
    let descriptor_name = std::env::var("SLUG_TARGET_DESCRIPTOR")
        .unwrap_or_else(|_| "esp32-s3-pinpulse.toml".into());
    let descriptor_path = root.join(descriptor_name);
    let descriptor = TargetDescriptor::load(&descriptor_path).expect("valid target descriptor");
    descriptor
        .validate_provider_libraries(&[
            ("slug.builtin", "slug-esp32-s3::builtin"),
            ("slug.gpio", "slug-esp32-s3::gpio"),
            ("slug.time", "slug-esp32-s3::time"),
        ])
        .expect("descriptor providers linked by this board package");
    println!("cargo:rerun-if-changed={}", descriptor_path.display());
    let output_directory = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let toolchain = slug_embedded_build::CToolchain {
        compiler: "xtensa-esp32s3-elf-gcc".into(),
        archiver: "xtensa-esp32s3-elf-ar".into(),
        flags: vec!["-mlongcalls".into(), "-Os".into(), "-ffunction-sections".into(), "-fdata-sections".into()],
    };
    let typed = descriptor
        .build_typed_providers(&toolchain, &output_directory)
        .unwrap_or_else(|error| panic!("{error}"));
    for directive in typed.cargo_directives() {
        println!("{directive}");
    }
    std::fs::write(output_directory.join("slug_typed_providers.rs"), &typed.registration)
        .expect("write typed provider registration");

    let builtins = NativeModule::new("slug.builtin", ()).expect("valid builtin module");
    let artifact = descriptor
        .compile_artifact_with_foreign([
            builtins.function("print", NativeArity::Variadic { minimum: 0 }, build_builtin).expect("print"),
            builtins.function("println", NativeArity::Variadic { minimum: 0 }, build_builtin).expect("println"),
            builtins.function("len", NativeArity::Exact(1), build_builtin).expect("len"),
        ])
        .expect("compile target artifact");
    for input in artifact.inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    std::fs::write(output_directory.join("slug_program.cslug"), &artifact.bytes)
        .expect("write target artifact");
    let limits = descriptor.cslug_limits();
    let limits_source = format!(
        "const TARGET_ARTIFACT_LIMITS: slug_vm::cslug::Limits = slug_vm::cslug::Limits {{ artifact_bytes: {}, section_bytes: {}, modules: {}, imports: {}, string_bytes: {}, chunks_per_module: {}, entries_per_table: {} }};\n",
        limits.artifact_bytes,
        limits.section_bytes,
        limits.modules,
        limits.imports,
        limits.string_bytes,
        limits.chunks_per_module,
        limits.entries_per_table,
    );
    std::fs::write(output_directory.join("slug_target_limits.rs"), limits_source)
        .expect("write target artifact limits");
}

fn build_builtin(call: &mut slug_vm::NativeCall<'_>) -> slug_vm::NativeStatus {
    call.return_value(slug_vm::NativeOwnedValue::nil())
}

fn linker_is_available() -> bool {
    std::process::Command::new("xtensa-esp32s3-elf-gcc")
        .arg("--version")
        .output()
        .is_ok()
}
