use std::{fs, process::Command};

fn runner() -> Command {
    Command::new(env!("CARGO_BIN_EXE_slug-fixtures"))
}

fn slug() -> &'static str {
    env!("CARGO_BIN_EXE_slug")
}

fn root(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "slug-conformance-runner-{kind}-{}",
        std::process::id()
    ))
}

#[test]
fn runs_classified_fixtures_and_compares_their_streams() {
    let root = root("success");
    fs::create_dir_all(&root).expect("create fixture directory");
    fs::write(root.join("success.slug"), "println(42)\n").expect("write fixture source");
    fs::write(
        root.join("success.fixture.toml"),
        "schema = 1\noutcome = \"success\"\nstdout = \"42\\n\"\nstderr = \"\"\n",
    )
    .expect("write fixture metadata");

    let output = runner()
        .arg(&root)
        .arg("--slug")
        .arg(slug())
        .output()
        .expect("run fixture runner");
    fs::remove_dir_all(root).expect("remove fixture directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn compares_exact_runtime_diagnostics_with_source_locations() {
    let root = root("runtime-error");
    fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("failure.slug");
    fs::write(&source, "???\n").expect("write fixture source");
    let diagnostic = format!(
        "slug: runtime error: not implemented at {}:1:1\n  in main\n",
        source.display()
    );
    fs::write(
        root.join("failure.fixture.toml"),
        format!("schema = 1\noutcome = \"runtime-error\"\ndiagnostic = {diagnostic:?}\n"),
    )
    .expect("write fixture metadata");

    let output = runner()
        .arg(&root)
        .arg("--slug")
        .arg(slug())
        .output()
        .expect("run fixture runner");
    fs::remove_dir_all(root).expect("remove fixture directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_unclassified_fixture_sources() {
    let root = root("unclassified");
    fs::create_dir_all(&root).expect("create fixture directory");
    fs::write(root.join("missing.slug"), "println(42)\n").expect("write fixture source");

    let output = runner()
        .arg(&root)
        .arg("--slug")
        .arg(slug())
        .output()
        .expect("run fixture runner");
    fs::remove_dir_all(root).expect("remove fixture directory");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot read")
    );
}
