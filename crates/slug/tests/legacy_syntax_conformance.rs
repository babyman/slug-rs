use std::{path::PathBuf, process::Command};

/// Suites that need tasks, which a build without `concurrency` lacks.
const NEEDS_CONCURRENCY: &[&str] = &["fork"];

#[test]
fn repository_conformance_fixtures_remain_conformant() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tests/conformance");
    let mut suites = std::fs::read_dir(&fixtures)
        .expect("read conformance suites")
        .map(|entry| entry.expect("conformance suite entry").path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            cfg!(feature = "concurrency")
                || !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| NEEDS_CONCURRENCY.contains(&name))
        })
        .collect::<Vec<_>>();
    suites.sort();
    for suite in suites {
        let output = Command::new(env!("CARGO_BIN_EXE_slug-fixtures"))
            .arg(&suite)
            .arg("--slug")
            .arg(env!("CARGO_BIN_EXE_slug"))
            .output()
            .expect("run conformance fixtures");

        assert!(
            output.status.success(),
            "{}: {}",
            suite.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
