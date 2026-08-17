use std::{fs, process::Command};

fn slug() -> Command {
    Command::new(env!("CARGO_BIN_EXE_slug"))
}

#[test]
fn help_describes_the_current_public_capability() {
    let output = slug().arg("--help").output().expect("run slug --help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("bindings, assignments, literals, arithmetic, calls, and println"));
    assert!(output.stderr.is_empty());
}

#[test]
fn version_is_available_without_loading_source() {
    let output = slug()
        .arg("--version")
        .output()
        .expect("run slug --version");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version is UTF-8"),
        "slug-vm 0.1.0\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_source_through_the_public_cli() {
    let path = std::env::temp_dir().join(format!("slug-cli-{}.slug", std::process::id()));
    fs::write(&path, "val total = 6 * 7\nprintln(total)\n").expect("write Slug source");
    let output = slug().arg(&path).output().expect("run Slug source");
    fs::remove_file(path).expect("remove Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "42\n"
    );
    assert!(output.stderr.is_empty());
}
