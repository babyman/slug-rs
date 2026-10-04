fn main() {
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

fn linker_is_available() -> bool {
    std::process::Command::new("xtensa-esp32s3-elf-gcc")
        .arg("--version")
        .output()
        .is_ok()
}
