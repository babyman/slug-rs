//! Builds a typed clutch's dynamic library: generates its shim and compiles
//! the shim with the clutch's C sources using the platform C compiler.

use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use crate::{
    DYNAMIC_DEFINE, ManifestError, NativeManifest, PRODUCER_HEADER, PRODUCER_HEADER_NAME,
    SHIM_C_FLAGS, generate,
};

/// The C compiler used for shims: `$CC` when set, otherwise `clang` on
/// Windows and `cc` elsewhere.
#[must_use]
pub fn c_compiler() -> String {
    env::var("CC").unwrap_or_else(|_| if cfg!(windows) { "clang" } else { "cc" }.into())
}

/// Generates the shim for `manifest` and compiles it, with the manifest's
/// sources and link libraries, into the dynamic library the manifest names
/// for `platform`. The generated shim is written beside the library with the
/// extension `shim.c`, with the producer header next to it on the include
/// path. Returns the library path.
///
/// # Errors
///
/// Returns a checked error when the platform has no library, a path escapes
/// `clutch_root`, a file cannot be written, or the compiler fails; a
/// compiler failure includes its diagnostics.
pub fn build_dynamic(
    clutch_root: &Path,
    manifest: &NativeManifest,
    provider: &str,
    platform: &str,
) -> Result<PathBuf, ManifestError> {
    let library = manifest.libraries.get(platform).ok_or_else(|| {
        ManifestError::new(
            "native.libraries",
            format!("no library for platform `{platform}`"),
        )
    })?;
    let output = clutch_root.join(contained(library, "native.libraries")?);
    let shim = output.with_extension("shim.c");
    let directory = output.parent().unwrap_or(clutch_root);
    fs::create_dir_all(directory).map_err(|error| {
        ManifestError::new(
            "native.libraries",
            format!("cannot create {}: {error}", directory.display()),
        )
    })?;
    fs::write(&shim, generate(manifest, provider)?).map_err(|error| {
        ManifestError::new("shim", format!("cannot write {}: {error}", shim.display()))
    })?;
    // The clutch's sources include the producer header from beside the shim.
    let header = directory.join(PRODUCER_HEADER_NAME);
    fs::write(&header, PRODUCER_HEADER).map_err(|error| {
        ManifestError::new(
            "shim",
            format!("cannot write {}: {error}", header.display()),
        )
    })?;

    let mut command = Command::new(c_compiler());
    command
        .args(SHIM_C_FLAGS)
        .arg(format!("-D{DYNAMIC_DEFINE}"))
        .arg("-I")
        .arg(clutch_root)
        .arg("-I")
        .arg(directory);
    if platform.starts_with("macos-") {
        command.arg("-dynamiclib");
    } else if platform.starts_with("windows-") {
        command.arg("-shared");
    } else {
        command.args(["-shared", "-fPIC"]);
    }
    for source in &manifest.sources {
        command.arg(clutch_root.join(contained(source, "native.sources")?));
    }
    command.arg(&shim).arg("-o").arg(&output);
    for library in &manifest.link {
        command.arg(format!("-l{library}"));
    }
    let result = command.output().map_err(|error| {
        ManifestError::new("shim", format!("cannot start the C compiler: {error}"))
    })?;
    if !result.status.success() {
        return Err(ManifestError::new(
            "shim",
            format!(
                "C compiler failed for {}:\n{}",
                shim.display(),
                String::from_utf8_lossy(&result.stderr)
            ),
        ));
    }
    Ok(output)
}

fn contained<'a>(path: &'a str, location: &str) -> Result<&'a Path, ManifestError> {
    let relative = Path::new(path);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(ManifestError::new(
            location,
            format!("`{path}` must be a relative path inside the clutch"),
        ));
    }
    Ok(relative)
}
