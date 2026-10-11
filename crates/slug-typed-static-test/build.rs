//! Builds the fixture's typed providers and program the way a board package
//! does, with the host C toolchain.

use std::{env, fs, path::PathBuf};

use slug_embedded_build::{CToolchain, TargetDescriptor};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let descriptor_path = root.join("fixtures/target.toml");
    println!("cargo:rerun-if-changed={}", descriptor_path.display());
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let descriptor =
        TargetDescriptor::load(&descriptor_path).unwrap_or_else(|error| panic!("{error}"));
    let typed = descriptor
        .build_typed_providers(&CToolchain::host(), &output)
        .unwrap_or_else(|error| panic!("{error}"));
    for directive in typed.cargo_directives() {
        println!("{directive}");
    }
    fs::write(output.join("typed_providers.rs"), &typed.registration)
        .expect("write typed provider registration");
    let artifact = descriptor
        .compile_artifact()
        .unwrap_or_else(|error| panic!("{error}"));
    for input in artifact.inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    fs::write(output.join("program.cslug"), &artifact.bytes).expect("write program artifact");
}
