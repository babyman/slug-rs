use std::fs;

use slug_vm::{FixtureMetadata, FixtureOutcome};

fn path(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "slug-fixture-metadata-{kind}-{}.toml",
        std::process::id()
    ))
}

#[test]
fn reads_complete_versioned_fixture_metadata() {
    let path = path("complete");
    fs::write(
        &path,
        "schema = 1\noutcome = \"runtime-error\"\nstdout = \"before\\n\"\nstderr = \"\"\nmodule_root = \"project\"\nlibrary_root = \"lib\"\ntimeout_ms = 250\ndiagnostic = \"slug: runtime error: boom\"\n",
    )
    .expect("write fixture metadata");

    let metadata = FixtureMetadata::load(&path).expect("parse fixture metadata");
    fs::remove_file(path).expect("remove fixture metadata");

    assert_eq!(metadata.outcome, FixtureOutcome::RuntimeError);
    assert_eq!(metadata.stdout.as_deref(), Some("before\n"));
    assert_eq!(metadata.stderr.as_deref(), Some(""));
    assert_eq!(
        metadata.module_root.as_deref(),
        Some(std::path::Path::new("project"))
    );
    assert_eq!(
        metadata.library_root.as_deref(),
        Some(std::path::Path::new("lib"))
    );
    assert_eq!(metadata.timeout_ms, Some(250));
    assert_eq!(
        metadata.diagnostic.as_deref(),
        Some("slug: runtime error: boom")
    );
}

#[test]
fn rejects_incomplete_or_unsafe_fixture_metadata() {
    for (kind, source, message) in [
        (
            "missing",
            "schema = 1\n",
            "missing fixture metadata field `outcome`",
        ),
        (
            "unknown",
            "schema = 1\noutcome = \"success\"\nextra = true\n",
            "unknown fixture metadata field `extra`",
        ),
        (
            "escape",
            "schema = 1\noutcome = \"success\"\nmodule_root = \"../outside\"\n",
            "module_root` must be a relative path",
        ),
        (
            "diagnostic",
            "schema = 1\noutcome = \"success\"\ndiagnostic = \"nope\"\n",
            "success fixtures cannot declare a diagnostic",
        ),
    ] {
        let path = path(kind);
        fs::write(&path, source).expect("write invalid fixture metadata");
        let error = FixtureMetadata::load(&path).expect_err("metadata must be rejected");
        fs::remove_file(path).expect("remove invalid fixture metadata");
        assert!(error.to_string().contains(message), "{error}");
    }
}
