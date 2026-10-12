//! `slug.db.sqlite` on typed native bindings, built as clutch staging builds
//! it. Unix-only: the Windows runner has no system `SQLite`.
#![cfg(unix)]

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use slug::DesktopLoader as ModuleLoader;
use slug_desktop_loader::ClutchRepository;
use slug_native_manifest::{NativeManifest, build_dynamic};
use slug_vm::Program;

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "slug-typed-sqlite-{}-{}",
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
        (os, architecture) => panic!("unsupported native test platform {os}-{architecture}"),
    }
}

/// Builds the current `SQLite` clutch into a repository in `directory`.
fn sqlite_clutch(directory: &TemporaryDirectory) -> ClutchRepository {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../clutch/slug.db.sqlite.clutch");
    let repository = directory.path().join("clutch");
    let root = repository.join("slug.db.sqlite.clutch");
    for file in [
        "clutch.toml",
        "modules/sqlite.slug",
        "modules/statement.slug",
        "modules/transaction.slug",
        "native/source/sqlite.c",
        "native/source/slug_db_sqlite.h",
    ] {
        let target = root.join(file);
        fs::create_dir_all(target.parent().expect("clutch file has a parent"))
            .expect("create clutch directory");
        fs::copy(source.join(file), target).expect("copy SQLite clutch file");
    }
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\
         \"slug.db.sqlite\" = \"slug.db.sqlite.clutch\"\n\
         \"slug.db.sqlite.statement\" = \"slug.db.sqlite.clutch\"\n\
         \"slug.db.sqlite.transaction\" = \"slug.db.sqlite.clutch\"\n",
    )
    .expect("write repository manifest");
    let manifest = NativeManifest::from_clutch_source(
        &fs::read_to_string(root.join("clutch.toml")).expect("read SQLite manifest"),
    )
    .expect("valid SQLite manifest");
    build_dynamic(&root, &manifest, "slug_db_sqlite", platform())
        .unwrap_or_else(|error| panic!("build SQLite clutch: {error}"));
    ClutchRepository::from_manifest(&repository).expect("load SQLite repository")
}

/// The repository's `lib`, which supplies `slug.builtin` values such as
/// `Error` that the clutch's wrappers throw.
fn library_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib")
}

fn compile(directory: &TemporaryDirectory, loader: &ModuleLoader, source: &str) -> Program {
    loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
            source,
        )
        .unwrap_or_else(|error| panic!("compile SQLite program: {error}"))
}

/// Makes `directory` a `SLUG_HOME` with the `SQLite` clutch and the
/// repository's `slug.builtin`, which the clutch's wrappers rely on.
fn slug_home(directory: &TemporaryDirectory) {
    sqlite_clutch(directory);
    let library = directory.path().join("lib/slug");
    fs::create_dir_all(&library).expect("create SLUG_HOME library");
    fs::copy(
        library_root().join("slug/builtin.slug"),
        library.join("builtin.slug"),
    )
    .expect("copy builtin module");
}

fn run_cli(directory: &TemporaryDirectory, source: &str) -> std::process::Output {
    let program = directory.path().join("cli.slug");
    fs::write(&program, source).expect("write CLI program");
    Command::new(env!("CARGO_BIN_EXE_slug"))
        .env("SLUG_HOME", directory.path())
        .arg(&program)
        .output()
        .expect("run slug CLI")
}

/// Runs `source` through the CLI and returns its `export val` values as a
/// map, printed in declaration order.
fn exports(source: &str) -> String {
    let directory = TemporaryDirectory::new();
    slug_home(&directory);
    let mut names = Vec::new();
    let mut program = String::new();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("export val ") {
            let name = rest.split([' ', ':', '=']).next().unwrap_or_default();
            names.push(name.to_owned());
            program.push_str("val ");
            program.push_str(rest);
        } else {
            program.push_str(line);
        }
        program.push('\n');
    }
    let entries = names
        .iter()
        .map(|name| format!("{name}: {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(program, "println({{{entries}}})");
    let output = run_cli(&directory, &program);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

#[test]
fn binds_slug_values_and_returns_rows() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val statement = import(\"slug.db.sqlite.statement\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             sqlite.exec(db, \"create table person(id integer primary key, name text)\")\n\
             val insert = statement.prepare(db, \"insert into person(name) values (?)\")\n\
             defer { statement.close(insert) }\n\
             statement.exec(insert, \"Alice\")\n\
             statement.exec(insert, \"Bob\")\n\
             val query = statement.prepare(db, \"select id, name from person where id > ? order by id\")\n\
             defer { statement.close(query) }\n\
             export val rows = statement.query(query, 0)\n",
        ),
        "{\"rows\": [{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]}"
    );
}

#[test]
fn preserves_database_and_statement_identities() {
    let directory = TemporaryDirectory::new();
    let repository = sqlite_clutch(&directory);
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    compile(
        &directory,
        &loader,
        "val sqlite = import(\"slug.db.sqlite\")\n\
         val statement = import(\"slug.db.sqlite.statement\")\n\
         val db = sqlite.open(\":memory:\")\n\
         val stmt:statement.Statement = statement.prepare(db, \"select 1\")\n\
         statement.close(stmt)\n\
         sqlite.close(db)\n",
    );
    let error = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
            "val statement = import(\"slug.db.sqlite.statement\")\n\
             resource Other\n\
             foreign openOther = fn():Other\n\
             statement.prepare(openOther(), \"select 1\")\n",
        )
        .expect_err("an unrelated resource cannot be passed as a SQLite database");
    assert!(
        error.to_string().contains("expected Database, got Other"),
        "{error}"
    );
}

#[test]
fn transaction_module_commits_successful_work() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val transaction = import(\"slug.db.sqlite.transaction\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             sqlite.exec(db, \"create table person(name text)\")\n\
             val result = transaction.transaction(db, fn(database:sqlite.Database) {\n\
                 sqlite.exec(database, \"insert into person(name) values (?)\", \"Alice\")\n\
                 \"committed\"\n\
             })\n\
             export val outcome = result\n\
             export val rows = sqlite.query(db, \"select name from person\")\n",
        ),
        "{\"outcome\": \"committed\", \"rows\": [{\"name\": \"Alice\"}]}"
    );
}

#[test]
fn transaction_module_rolls_back_and_rethrows_work_errors() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val transaction = import(\"slug.db.sqlite.transaction\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             sqlite.exec(db, \"create table person(name text)\")\n\
             val attempt = fn() {\n\
                 defer onerror(err) { err }\n\
                 transaction.transaction(db, fn(database:sqlite.Database) {\n\
                     sqlite.exec(database, \"insert into person(name) values (?)\", \"discarded\")\n\
                     throw \"transaction failed\"\n\
                 })\n\
             }\n\
             export val error = attempt()\n\
             export val rows = sqlite.query(db, \"select name from person\")\n",
        ),
        "{\"error\": \"transaction failed\", \"rows\": []}"
    );
}

#[test]
fn statement_failures_clear_partial_bindings_before_reuse() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val statement = import(\"slug.db.sqlite.statement\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             sqlite.exec(db, \"create table pair(left_value text, right_value text)\")\n\
             val insert = statement.prepare(db, \"insert into pair(left_value, right_value) values (?, ?)\")\n\
             defer { statement.close(insert) }\n\
             statement.exec(insert, \"old\", \"stale\")\n\
             val recover = fn(work) { defer onerror(err) { nil }; work() }\n\
             recover(fn() { statement.exec(insert, \"discard\", []) })\n\
             statement.exec(insert, \"fresh\")\n\
             val query = statement.prepare(db, \"select ? as left_value, ? as right_value\")\n\
             defer { statement.close(query) }\n\
             statement.query(query, \"old\", \"stale\")\n\
             recover(fn() { statement.query(query, \"discard\", []) })\n\
             export val rows = sqlite.query(db, \"select left_value, right_value from pair order by rowid\")\n\
             export val query_rows = statement.query(query, \"fresh\")\n",
        ),
        "{\"rows\": [{\"left_value\": \"old\", \"right_value\": \"stale\"}, {\"left_value\": \"fresh\", \"right_value\": nil}], \"query_rows\": [{\"left_value\": \"fresh\", \"right_value\": nil}]}"
    );
}

#[test]
fn round_trips_text_with_nul_blobs_reals_and_empty_values() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             export val rows = sqlite.query(db, \
             \"select ? as text, ? as empty_text, ? as blob, ? as empty_blob, ? as real, ? as whole, ? as large, ? as missing\", \
             \"a\\0b\", \"\", 0x\"cafe\", 0x\"\", 2.5, 2.0, 9223372036854775807, nil)\n",
        ),
        "{\"rows\": [{\"text\": \"a\\0b\", \"empty_text\": \"\", \"blob\": 0x\"cafe\", \"empty_blob\": 0x\"\", \"real\": 2.5, \"whole\": 2, \"large\": 9223372036854775807, \"missing\": nil}]}"
    );
}

#[test]
fn failures_throw_errors_with_sqlite_types() {
    assert_eq!(
        exports(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val statement = import(\"slug.db.sqlite.statement\")\n\
             val report = fn(work) {\n\
                 defer onerror(err) { err.type + \": \" + err.msg }\n\
                 work()\n\
             }\n\
             val db = sqlite.open(\":memory:\")\n\
             export val syntax = report(fn() { sqlite.exec(db, \"selekt 1\") })\n\
             export val parameter = report(fn() { sqlite.query(db, \"select ?\", [1]) })\n\
             export val table = report(fn() { sqlite.query(db, \"select * from missing\") })\n\
             val open = statement.prepare(db, \"select 1\")\n\
             export val busy = report(fn() { sqlite.close(db) })\n\
             export val stillOpen = sqlite.query(db, \"select 7 as seven\")\n\
             statement.close(open)\n\
             export val closed = sqlite.close(db)\n",
        ),
        "{\"syntax\": \"sqlite.error: near \\\"selekt\\\": syntax error\", \
          \"parameter\": \"sqlite.bind: unsupported SQL parameter\", \
          \"table\": \"sqlite.error: no such table: missing\", \
          \"busy\": \"sqlite.error: unable to close due to unfinalized statements or unfinished backups\", \
          \"stillOpen\": [{\"seven\": 7}], \"closed\": 0}"
    );
}

#[test]
fn the_statement_module_can_be_imported_first() {
    assert_eq!(
        exports(
            "val statement = import(\"slug.db.sqlite.statement\")\n\
             val sqlite = import(\"slug.db.sqlite\")\n\
             val db = sqlite.open(\":memory:\")\n\
             defer { sqlite.close(db) }\n\
             val one = statement.prepare(db, \"select 1 as one\")\n\
             defer { statement.close(one) }\n\
             export val rows = statement.query(one)\n",
        ),
        "{\"rows\": [{\"one\": 1}]}"
    );
}

#[test]
fn uncaught_failures_report_the_thrown_error() {
    let directory = TemporaryDirectory::new();
    slug_home(&directory);
    let output = run_cli(
        &directory,
        "val sqlite = import(\"slug.db.sqlite\")\n\
         val db = sqlite.open(\":memory:\")\n\
         sqlite.exec(db, \"selekt 1\")\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("uncaught throw"), "{stderr}");
    assert!(stderr.contains("sqlite.error"), "{stderr}");
    assert!(stderr.contains("syntax error"), "{stderr}");
}
