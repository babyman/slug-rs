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
    println!("cargo:rerun-if-changed={}", descriptor_path.display());

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
    let output_directory = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(output_directory.join("slug_program.cslug"), &artifact.bytes)
        .expect("write target artifact");
    let metadata = format!(
        "{{\"artifact\":\"slug_program.cslug\",\"artifact_bytes\":{},\"limits\":{{\"artifact_bytes\":{},\"section_bytes\":{},\"modules\":{},\"imports\":{},\"string_bytes\":{},\"chunks_per_module\":{},\"entries_per_table\":{}}}}}\n",
        artifact.bytes.len(), descriptor.limits.artifact_bytes, descriptor.limits.section_bytes,
        descriptor.limits.modules, descriptor.limits.imports, descriptor.limits.string_bytes,
        descriptor.limits.chunks_per_module, descriptor.limits.entries_per_table,
    );
    std::fs::write(output_directory.join("slug_target_metadata.json"), metadata)
        .expect("write target metadata");
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
