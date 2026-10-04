fn main() {
    freeze_entry_program();
    validate_blink_program();
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

include!("capabilities.rs");

fn validate_blink_program() {
    let source_path = "programs/blink.slug";
    println!("cargo:rerun-if-changed={source_path}");
    println!("cargo:rerun-if-changed=capabilities.rs");
    let source = std::fs::read_to_string(source_path)
        .unwrap_or_else(|error| panic!("cannot read {source_path}: {error}"));
    let image = slug_frontend::compile_frozen_gpio_entry(source_path, &source, &BLINK_CAPABILITIES)
        .unwrap_or_else(|error| panic!("cannot freeze {source_path}: {error}"));
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"))
        .join("slug_blink_program.rs");
    std::fs::write(output, image).expect("writes generated blink program");
}

fn freeze_entry_program() {
    let source_path = "programs/hello.slug";
    println!("cargo:rerun-if-changed={source_path}");
    let source = std::fs::read_to_string(source_path)
        .unwrap_or_else(|error| panic!("cannot read {source_path}: {error}"));
    let image = slug_frontend::compile_frozen_uart_entry(source_path, &source)
        .unwrap_or_else(|error| panic!("cannot freeze {source_path}: {error}"));
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"))
        .join("slug_frozen_program.rs");
    std::fs::write(output, image).expect("writes generated frozen program");
}

fn linker_is_available() -> bool {
    std::process::Command::new("xtensa-esp32s3-elf-gcc")
        .arg("--version")
        .output()
        .is_ok()
}
