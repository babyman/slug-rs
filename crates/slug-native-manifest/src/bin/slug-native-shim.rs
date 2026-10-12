//! Generates or checks the typed native shim for a clutch manifest.
//!
//! ```text
//! slug-native-shim generate <clutch.toml> <provider> <shim.c>
//! slug-native-shim check <clutch.toml> <provider> <shim.c>
//! slug-native-shim stage <clutch.toml> <provider> <platform>
//! ```
//!
//! `stage` builds the dynamic library the manifest names for `platform`.

use std::{env, fs, path::Path, process::ExitCode};

use slug_native_manifest::NativeManifest;

const USAGE: &str = "usage: slug-native-shim (generate|check) <clutch.toml> <provider> <shim.c>\n       slug-native-shim stage <clutch.toml> <provider> <platform>";

fn main() -> ExitCode {
    match run(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("slug-native-shim: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<(), String> {
    let [mode, manifest_path, provider, shim_path] = arguments else {
        return Err(USAGE.into());
    };
    let source = fs::read_to_string(manifest_path)
        .map_err(|error| format!("cannot read {manifest_path}: {error}"))?;
    let manifest = NativeManifest::from_clutch_source(&source)
        .map_err(|error| format!("{manifest_path}: {error}"))?;
    match mode.as_str() {
        "generate" => {
            let shim = slug_native_manifest::generate(&manifest, provider)
                .map_err(|error| error.to_string())?;
            fs::write(shim_path, shim).map_err(|error| format!("cannot write {shim_path}: {error}"))
        }
        "check" => {
            let existing = fs::read_to_string(shim_path)
                .map_err(|error| format!("cannot read {shim_path}: {error}"))?;
            slug_native_manifest::check(&manifest, provider, &existing)
                .map_err(|error| format!("{shim_path}: {error}"))
        }
        "stage" => {
            let root = Path::new(manifest_path)
                .parent()
                .unwrap_or_else(|| Path::new("."));
            let library = slug_native_manifest::build_dynamic(root, &manifest, provider, shim_path)
                .map_err(|error| format!("{manifest_path}: {error}"))?;
            println!("staged {}", library.display());
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
