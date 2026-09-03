use super::*;
use std::{io::Write, process::Stdio};

#[test]
fn reads_normalized_lines_and_closes_at_end_of_input() {
    let path = fixture_path("stdin-lines");
    fs::write(
        &path,
        "val { readLine } = import(\"slug.io.stdin\")\n\
         println(readLine())\n\
         println(readLine())\n\
         println(readLine())\n\
         println(readLine())\n",
    )
    .expect("write stdin line source");
    let mut child = slug()
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run stdin line source");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"first\r\n\nlast")
        .expect("write standard input");
    let output = child.wait_with_output().expect("collect stdin line output");
    fs::remove_file(path).expect("remove stdin line source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "first\n\nlast\nnil\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn stdin_prompt_and_confirm_use_the_shared_line_stream() {
    let path = fixture_path("stdin-helpers");
    fs::write(
        &path,
        "val { confirm, prompt } = import(\"slug.io.stdin\")\n\
         println(prompt(\"name: \"))\n\
         println(confirm(\"continue?\"))\n\
         println(confirm(\"default?\", true))\n",
    )
    .expect("write stdin helper source");
    let mut child = slug()
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run stdin helper source");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"Ada\nyes\nunknown\n")
        .expect("write standard input");
    let output = child
        .wait_with_output()
        .expect("collect stdin helper output");
    fs::remove_file(path).expect("remove stdin helper source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "name: Ada\ncontinue? [y/N]: true\ndefault? [Y/n]: true\n"
    );
    assert!(output.stderr.is_empty());
}
