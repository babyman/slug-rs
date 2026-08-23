use std::{path::PathBuf, process::Command};

#[test]
fn legacy_syntax_fixtures_remain_conformant() {
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/legacy-syntax");
    let output = Command::new(env!("CARGO_BIN_EXE_slug-fixtures"))
        .arg(fixtures)
        .arg("--slug")
        .arg(env!("CARGO_BIN_EXE_slug"))
        .output()
        .expect("run legacy syntax fixtures");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
