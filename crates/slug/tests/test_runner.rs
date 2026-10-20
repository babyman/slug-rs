//! `slug test`, the Slug test runner in `lib/test.slug`, run against clutches
//! installed in a temporary `SLUG_HOME`.

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_HOME: AtomicUsize = AtomicUsize::new(0);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_directory(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create copied directory");
    for entry in fs::read_dir(source).expect("read directory to copy") {
        let entry = entry.expect("read directory entry");
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("read entry type").is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy file");
        }
    }
}

/// A `SLUG_HOME` with the runner, `slug.builtin`, the core and filesystem
/// clutches as the repository has them, and a `demo` clutch whose module and
/// test file are given.
struct Home(PathBuf);

impl Home {
    fn new(module: &str, tests: &str) -> Self {
        let home = env::temp_dir().join(format!(
            "slug-test-runner-{}-{}",
            std::process::id(),
            NEXT_HOME.fetch_add(1, Ordering::Relaxed)
        ));
        let repository = home.join("clutch");
        for clutch in ["slug.core.clutch", "slug.io.fs.clutch"] {
            copy_directory(
                &workspace_root().join("clutch").join(clutch),
                &repository.join(clutch),
            );
        }
        // The repository's index, limited to the two clutches copied here.
        let manifest = fs::read_to_string(workspace_root().join("clutch/manifest.toml"))
            .expect("read repository manifest")
            .lines()
            .filter(|line| {
                line.starts_with('[')
                    || line.ends_with("\"slug.core.clutch\"")
                    || line.ends_with("\"slug.io.fs.clutch\"")
            })
            .fold(String::new(), |mut manifest, line| {
                let _ = writeln!(manifest, "{line}");
                manifest
            });
        fs::write(
            repository.join("manifest.toml"),
            format!("{manifest}\"demo.lib\" = \"demo.clutch\"\n"),
        )
        .expect("write repository manifest");
        let demo = repository.join("demo.clutch");
        fs::create_dir_all(demo.join("modules")).expect("create demo modules");
        fs::create_dir_all(demo.join("tests")).expect("create demo tests");
        fs::write(
            demo.join("clutch.toml"),
            "[modules]\n\"demo.lib\" = { source = \"modules/lib.slug\" }\n\n[tests]\nfiles = [\"tests/lib.slug\"]\n",
        )
        .expect("write demo manifest");
        fs::write(demo.join("modules/lib.slug"), module).expect("write demo module");
        fs::write(demo.join("tests/lib.slug"), tests).expect("write demo tests");
        let library = home.join("lib/slug");
        fs::create_dir_all(&library).expect("create library");
        fs::copy(
            workspace_root().join("lib/slug/builtin.slug"),
            library.join("builtin.slug"),
        )
        .expect("copy builtin module");
        fs::copy(
            workspace_root().join("lib/test.slug"),
            home.join("lib/test.slug"),
        )
        .expect("copy test runner");
        Self(home)
    }

    fn run(&self, flag: bool, clutches: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_slug"));
        command.env("SLUG_HOME", &self.0).current_dir(&self.0);
        if flag {
            command.arg("--allow-unexported");
        }
        command.arg("test");
        for clutch in clutches {
            command.arg(self.0.join("clutch").join(clutch));
        }
        command.output().expect("run slug test")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn runs_test_and_test_with_declarations_and_reports_failures() {
    let home = Home::new(
        "@testWith([1, 2], 3, [2, 2], 5)\n\
         export val add = fn(a, b) { a + b }\n\
         \n\
         @test\n\
         val helper = fn() { add(2, 2) == 4 }\n",
        "val lib = import(\"demo.lib\")\n\
         val {assertEqual} = import(\"slug.test\")\n\
         \n\
         @test\n\
         export val adds = fn() { assertEqual(lib.add(1, 2), 3) }\n\
         \n\
         @test\n\
         val failsOnPurpose = fn() { assertEqual(lib.add(1, 1), 3, \"one plus one\") }\n\
         \n\
         @test\n\
         val throwsOnPurpose = fn() { 1 / 0 }\n",
    );
    let output = home.run(true, &["demo.clutch"]);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "FAIL demo.lib.add, example 2: wrong result\n  \
         arguments: [2, 2]\n  \
         expected:  5\n  \
         actual:    4\n\
         FAIL demo.tests.lib.failsOnPurpose: one plus one\n  \
         expected:  3\n  \
         actual:    2\n\
         FAIL demo.tests.lib.throwsOnPurpose: threw divide_by_zero: division by zero\n  \
         error:     {\"type\": \"divide_by_zero\", \"msg\": \"division by zero\", \"data\": nil}\n\
         demo: 3 passed, 3 failed\n"
    );
    assert!(
        text(&output.stderr).contains("uncaught throw: 3 tests failed"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn passes_the_core_clutch_and_needs_the_permission() {
    let home = Home::new("export val one = 1\n", "@test\nval ok = fn() { true }\n");
    let output = home.run(true, &["demo.clutch", "slug.core.clutch"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("demo: 1 passed, 0 failed\nslug.core: "),
        "{stdout}"
    );
    assert!(stdout.ends_with(" passed, 0 failed\n"), "{stdout}");

    // Without the flag, no module can be described with its unexported
    // declarations.
    let output = home.run(false, &["demo.clutch"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("FAIL demo.lib: cannot load the module\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("requires running slug with --allow-unexported"),
        "{stdout}"
    );

    let output = home.run(true, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output.stderr)
            .contains("usage: slug --allow-unexported test ./path/to/name.clutch ..."),
        "{}",
        text(&output.stderr)
    );
}
