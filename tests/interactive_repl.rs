use std::{
    io::Write,
    process::{Command, Stdio},
};

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
