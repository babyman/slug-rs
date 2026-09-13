use std::{
    fs,
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
fn repl_exposes_the_implicit_len_builtin() {
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
        .write_all(b"println(\"hello Slug!\", 'len:', len('hello Slug!'))\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("hello Slug! len: 11\n"));
}

#[test]
fn repl_seeds_the_session_from_an_optional_source_file() {
    let path = std::env::temp_dir().join(format!("slug-repl-session-{}.slug", std::process::id()));
    fs::write(
        &path,
        "var msg = chan(8)\nprintln('initialized msg', msg)\n",
    )
    .expect("write startup source");
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-repl");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"msg\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");
    fs::remove_file(path).expect("remove startup source");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("initialized msg"));
    assert!(!stdout.contains("unknown name `msg`"));
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
    assert!(!stdout.contains("[incomplete]"));
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

#[test]
fn repl_returns_to_the_primary_prompt_after_a_structured_error() {
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
        .write_all(b"val =\n1 + 2\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stdout.contains("> > 3\n> "));
    assert!(stderr.contains("source error (parse): expected binding name"));
}

#[test]
fn repl_renders_runtime_details_from_the_protocol_diagnostic() {
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
        .write_all(b"val inner = fn() { 1 / 0 }\nval outer = fn() { inner() }\nouter()\n:quit\n")
        .expect("write input");
    let output = child.wait_with_output().expect("wait for repl");

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("runtime error (divide_by_zero): division by zero"));
    assert!(stderr.contains("--> <interactive:s1>:1:22"));
    assert!(stderr.contains("at <fn #0> (<interactive:s1>:1:20)"));
}

#[test]
fn repl_reports_a_missing_configured_server() {
    let missing_server =
        std::env::temp_dir().join(format!("slug-repl-missing-server-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_slug-repl"))
        .env("SLUG_SERVER", missing_server)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("start slug-repl");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("cannot start slug-server"));
}
