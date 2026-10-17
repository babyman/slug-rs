//! `slug.io.stdin`, `slug.math`, and `slug.string` on typed native bindings, built from the
//! `slug.core` clutch as clutch staging builds it.

use std::{
    env,
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_native_manifest::{NativeManifest, build_dynamic};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "slug-typed-core-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn platform() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        (os, architecture) => panic!("unsupported native test platform {os}-{architecture}"),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Makes `directory` a `SLUG_HOME` with the current core clutch, built into
/// it, and the repository's `slug.builtin`, which the clutch's wrappers rely
/// on.
fn slug_home(directory: &TemporaryDirectory) {
    let source = workspace_root().join("clutch/slug.core.clutch");
    let repository = directory.path().join("clutch");
    let root = repository.join("slug.core.clutch");
    for file in [
        "clutch.toml",
        "modules/std.slug",
        "modules/stdin.slug",
        "modules/math.slug",
        "modules/string.slug",
        "native/source/core.c",
        "native/source/slug_core.h",
    ] {
        let target = root.join(file);
        fs::create_dir_all(target.parent().expect("clutch file has a parent"))
            .expect("create clutch directory");
        fs::copy(source.join(file), target).expect("copy core clutch file");
    }
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\
         \"slug.std\" = \"slug.core.clutch\"\n\
         \"slug.io.stdin\" = \"slug.core.clutch\"\n\
         \"slug.math\" = \"slug.core.clutch\"\n\
         \"slug.string\" = \"slug.core.clutch\"\n",
    )
    .expect("write repository manifest");
    let manifest = NativeManifest::from_clutch_source(
        &fs::read_to_string(root.join("clutch.toml")).expect("read core manifest"),
    )
    .expect("valid core manifest");
    build_dynamic(&root, &manifest, "slug_core", platform())
        .unwrap_or_else(|error| panic!("build core clutch: {error}"));

    let library = directory.path().join("lib/slug");
    fs::create_dir_all(&library).expect("create SLUG_HOME library");
    fs::copy(
        workspace_root().join("lib/slug/builtin.slug"),
        library.join("builtin.slug"),
    )
    .expect("copy builtin module");
}

/// Runs `source` through the CLI with `input` on standard input.
fn run_cli(source: &str, input: &[u8]) -> Output {
    let directory = TemporaryDirectory::new();
    slug_home(&directory);
    let program = directory.path().join("cli.slug");
    fs::write(&program, source).expect("write CLI program");
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug"))
        .env("SLUG_HOME", directory.path())
        .arg(&program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run slug CLI");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input)
        .expect("write standard input");
    child.wait_with_output().expect("collect CLI output")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

#[test]
fn reads_standard_input_lines_through_a_producer() {
    let output = run_cli(
        "val { readLine, readLines } = import(\"slug.io.stdin\")\n\
         println(readLine())\n\
         println(readLine())\n\
         println(readLines() == readLines())\n\
         println(readLine())\n\
         println(readLine())\n",
        b"first\r\n\nlast",
    );
    assert_eq!(stdout(&output), "first\n\ntrue\nlast\nnil\n");
}

#[test]
fn streams_more_lines_than_the_channel_buffers() {
    let mut input = String::new();
    for n in 1..=500 {
        let _ = writeln!(input, "{n}");
    }
    let output = run_cli(
        "val { readLines } = import(\"slug.io.stdin\")\n\
         val count = fn(lines, total, last) {\n\
             match select { recv lines } {\n\
                 nil => [total, last]\n\
                 line => recur(lines, total + 1, line)\n\
             }\n\
         }\n\
         println(count(readLines(), 0, nil))\n",
        input.as_bytes(),
    );
    assert_eq!(stdout(&output), "[500, \"500\"]\n");
}

#[test]
fn adds_and_takes_square_roots() {
    let output = run_cli(
        "val math = import(\"slug.math\")\n\
         println(math.add(20, 22), math.add(-5, 2), math.sqrt(9.0), math.sqrt(16), math.sqrt(0))\n",
        b"",
    );
    assert_eq!(stdout(&output), "42 -3 3 4 0\n");
}

#[test]
fn throws_typed_math_errors() {
    let output = run_cli(
        "val math = import(\"slug.math\")\n\
         val failure = fn(work) { defer onerror(err) { err.type + \": \" + err.msg }; work() }\n\
         println(failure(fn() { math.add(9223372036854775807, 1) }))\n\
         println(failure(fn() { math.add(-9223372036854775807, -2) }))\n\
         println(failure(fn() { math.sqrt(-1.0) }))\n",
        b"",
    );
    assert_eq!(
        stdout(&output),
        "math.range: integer addition overflowed\n\
         math.range: integer addition overflowed\n\
         math.domain: sqrt requires a non-negative number\n"
    );
}

#[test]
fn converts_characters_and_unicode_scalar_values() {
    let output = run_cli(
        "val {ord, chr} = import(\"slug.string\")\n\
         println(ord(\"A\"), ord(\"\u{e9}\"), ord(\"\u{20ac}\"), ord(\"\u{1f600}\"), ord(chr(1114111)))\n\
         println(chr(65), chr(233), chr(8364), chr(128512), chr(0) == \"\\0\")\n",
        b"",
    );
    assert_eq!(
        stdout(&output),
        "65 233 8364 128512 1114111\nA \u{e9} \u{20ac} \u{1f600} true\n"
    );
}

#[test]
fn throws_typed_string_errors() {
    let output = run_cli(
        "val {ord, chr} = import(\"slug.string\")\n\
         val failure = fn(work) { defer onerror(err) { err.type + \": \" + err.msg }; work() }\n\
         println(failure(fn() { ord(\"\") }))\n\
         println(failure(fn() { ord(\"ab\") }))\n\
         println(failure(fn() { chr(55296) }))\n\
         println(failure(fn() { chr(1114112) }))\n\
         println(failure(fn() { chr(-1) }))\n\
         println(failure(fn() { chr(65.5) }))\n",
        b"",
    );
    assert_eq!(
        stdout(&output),
        "string.range: ord requires a string of exactly one character\n\
         string.range: ord requires a string of exactly one character\n\
         string.range: chr requires a Unicode scalar value\n\
         string.range: chr requires a Unicode scalar value\n\
         string.range: chr requires a Unicode scalar value\n\
         string.range: chr requires an integer code\n"
    );
}
