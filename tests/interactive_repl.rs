use std::{
    io::Write,
    process::{Command, Stdio},
};

use slug_vm::source_is_incomplete;

#[test]
fn source_completeness_distinguishes_appendable_and_invalid_input() {
    assert!(source_is_incomplete("<repl>", "val add = fn(a, b) {"));
    assert!(source_is_incomplete("<repl>", "1 +"));
    assert!(!source_is_incomplete("<repl>", "val ="));
    assert!(!source_is_incomplete("<repl>", "1 + true"));
}

#[test]
fn repl_initializes_persists_values_renders_events_and_closes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"val x = 10\nx + 5\nprintln('hello')\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.starts_with("Slug REPL\n\n> "));
    assert!(stdout.contains("15\n"));
    assert!(stdout.contains("hello\n"));
}

#[test]
fn repl_renders_structured_source_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"1 + true\n:exit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("source error (semantic):"));
    assert!(stderr.contains("<interactive:s1>:"));
}

#[test]
fn repl_collects_multiline_functions_without_blank_line_termination() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"val add = fn(a, b) {\n\na + b\n}\nadd(2, 3)\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("> . . . > 5\n"));
}

#[test]
fn repl_reports_invalid_source_without_a_continuation_prompt() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"val =\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(!stdout.contains(". "));
    assert!(stderr.contains("source error (parse): expected binding name"));
}
