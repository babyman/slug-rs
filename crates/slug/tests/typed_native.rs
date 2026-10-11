//! Typed native clutches through the production desktop path: a manifest-bound
//! shim built by clutch staging, loaded by the desktop loader, and called
//! through the shared adapter.

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use slug::DesktopLoader as ModuleLoader;
use slug_desktop_loader::ClutchRepository;
use slug_native_manifest::{NativeManifest, build_dynamic};
use slug_vm::{RuntimeErrorKind, Value, Vm};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

const MODULE: &str = include_str!("typed_native/files.slug");
const MANIFEST: &str = include_str!("typed_native/clutch.toml");

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "slug-typed-native-{}-{}",
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

fn manifest_for_platform(manifest: &str) -> String {
    let library = format!(
        "{}fixture_files{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    );
    manifest
        .replace("PLATFORM", platform())
        .replace("LIBRARY", &library)
}

/// Writes a typed clutch repository and stages its library from
/// `staged_manifest`, then installs `manifest` as the clutch's manifest.
fn typed_clutch(
    directory: &TemporaryDirectory,
    module: &str,
    staged_manifest: &str,
    manifest: &str,
) -> ClutchRepository {
    typed_clutch_with_modules(
        directory,
        &[("fixture.files", "files.slug", module)],
        staged_manifest,
        manifest,
    )
}

/// Like [`typed_clutch`], with each `(module, file, source)` in `modules/`.
fn typed_clutch_with_modules(
    directory: &TemporaryDirectory,
    modules: &[(&str, &str, &str)],
    staged_manifest: &str,
    manifest: &str,
) -> ClutchRepository {
    let repository = directory.path().join("clutch");
    let root = repository.join("fixture.clutch");
    fs::create_dir_all(root.join("modules")).expect("create clutch modules");
    fs::create_dir_all(root.join("native/source")).expect("create clutch native source");
    let mut index = String::from("[modules]\n");
    for (name, file, source) in modules {
        let _ = writeln!(index, "\"{name}\" = \"fixture.clutch\"");
        fs::write(root.join("modules").join(file), source).expect("write module source");
    }
    fs::write(repository.join("manifest.toml"), index).expect("write repository manifest");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/typed_native");
    for file in ["fixture.h", "fixture.c"] {
        fs::copy(fixture.join(file), root.join("native/source").join(file))
            .expect("copy native fixture");
    }
    let staged = NativeManifest::from_clutch_source(&manifest_for_platform(staged_manifest))
        .expect("valid typed manifest");
    build_dynamic(&root, &staged, "fixture_files", platform())
        .unwrap_or_else(|error| panic!("stage typed clutch: {error}"));
    fs::write(root.join("clutch.toml"), manifest_for_platform(manifest))
        .expect("write clutch manifest");
    ClutchRepository::from_manifest(&repository).expect("load clutch repository")
}

fn run(
    directory: &TemporaryDirectory,
    repository: ClutchRepository,
    body: &str,
) -> Result<Value, String> {
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let source = format!("val files = import(\"fixture.files\")\n{body}\n");
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
            &source,
        )
        .map_err(|error| error.to_string())?;
    Vm::with_host(Rc::new(loader))
        .run_named(&program, "main")
        .map_err(|error| match &error.native {
            Some(native) => format!("{}: {error}", native.code),
            None if error.kind == RuntimeErrorKind::NativeContract => {
                format!("native_contract: {error}")
            }
            None => error.to_string(),
        })
}

fn slug_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn typed_clutch_reads_writes_and_closes_files() {
    let directory = TemporaryDirectory::new();
    let repository = typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
    let file = slug_path(&directory.path().join("lines.txt"));
    let value = run(
        &directory,
        repository,
        &format!(
            "val output = files.openWrite(\"{file}\")\n\
             if (output != nil) {{\n\
               val written = files.write(output, \"first\\nsecond\\n\")\n\
               files.close(output)\n\
               val input = files.openRead(\"{file}\")\n\
               if (input != nil) {{\n\
                 val lines = [files.readLine(input), files.readLine(input), files.readLine(input)]\n\
                 [written, lines, files.close(input), files.close(input), \
                  files.openRead(\"{file}.missing\"), files.add(40, 2), files.describe(7)]\n\
               }}\n\
             }}"
        ),
    )
    .expect("run typed clutch program");
    assert_eq!(
        value.to_string(),
        "[13, [[0, \"first\"], [0, \"second\"], [1, nil]], nil, nil, nil, 42, \"<7>\"]"
    );
}

#[test]
fn typed_clutch_reports_checked_boundary_failures() {
    let directory = TemporaryDirectory::new();
    let file = slug_path(&directory.path().join("closed.txt"));
    for (body, expected) in [
        (
            format!(
                "val output = files.openWrite(\"{file}\")\n\
                 if (output != nil) {{ files.close(output)\nfiles.write(output, \"late\") }}"
            ),
            "native.resource_closed",
        ),
        ("files.negate(200)".into(), "native.range"),
        (
            "files.openRead(\"bad\\0path\")".into(),
            "native.interior_nul",
        ),
    ] {
        let repository = typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
        let error = run(&directory, repository, &body).expect_err(&body);
        assert!(error.starts_with(expected), "{body}: {error}");
        if expected == "native.resource_closed" {
            assert!(error.contains("native resource is closed"), "{error}");
        }
    }
}

#[test]
fn typed_clutch_rejects_a_stale_shim() {
    let directory = TemporaryDirectory::new();
    let edited = MANIFEST.replace(
        "(i64 left, i64 right) -> i64",
        "(i64 left, i32 right) -> i64",
    );
    let repository = typed_clutch(&directory, MODULE, MANIFEST, &edited);
    let error = run(&directory, repository, "files.add(1, 2)").expect_err("stale shim");
    assert!(
        error.contains("cannot load typed native library"),
        "{error}"
    );
    assert!(error.contains("`fixture.files.add` is stale"), "{error}");
    assert!(error.contains("(i64,i32)->i64"), "{error}");
}

#[test]
fn typed_clutch_checks_foreign_declarations_against_bindings() {
    let directory = TemporaryDirectory::new();
    for (module, expected) in [
        (
            MODULE.replace("fn(left:num, right:num):num", "fn(left:str, right:num):num"),
            "foreign `fixture.files.add` parameter `left` is annotated `str`, but its native binding needs `num`",
        ),
        (
            MODULE.replace(
                "openRead = fn(path:str):File|nil",
                "openRead = fn(path:str):File",
            ),
            "foreign `fixture.files.openRead` result is annotated `File`, but its native binding needs `File|nil`",
        ),
        (
            MODULE.replace("fn(file:File):list", "fn(file:File):str|nil"),
            "foreign `fixture.files.readLine` result is annotated `nil|str`, but its native binding needs `list`",
        ),
        (
            MODULE.replace("fn(value:num):str", "fn(value:num, extra:num):str"),
            "foreign `fixture.files.describe` declares 2 parameters, but its native binding takes 1",
        ),
        (
            MODULE.replace("export foreign negate = fn(value:num):num\n", ""),
            "native binding `fixture.files.negate` has no foreign declaration",
        ),
        (
            format!("{MODULE}export foreign extra = fn():nil\n"),
            "foreign `fixture.files.extra` has no native binding in the clutch manifest",
        ),
    ] {
        let repository = typed_clutch(&directory, &module, MANIFEST, MANIFEST);
        let error = run(&directory, repository, "nil").expect_err(expected);
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn typed_clutch_manifests_cannot_mix_in_the_prototype_abi() {
    let directory = TemporaryDirectory::new();
    let mixed = MANIFEST.replace(
        "[native]\n",
        "[native]\nabi = \"slug-ffi-prototype/0.13\"\n",
    );
    let repository = directory.path().join("clutch");
    fs::create_dir_all(repository.join("fixture.clutch/modules")).expect("create clutch");
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\"fixture.files\" = \"fixture.clutch\"\n",
    )
    .expect("write repository manifest");
    fs::write(repository.join("fixture.clutch/modules/files.slug"), MODULE).expect("write module");
    fs::write(
        repository.join("fixture.clutch/clutch.toml"),
        manifest_for_platform(&mixed),
    )
    .expect("write manifest");
    let error = ClutchRepository::from_manifest(&repository).expect_err("mixed native forms");
    assert!(
        error.to_string().contains(
            "`abi` selects the prototype native form, which cannot be combined with typed `profile`"
        ),
        "{error}"
    );
}

#[test]
fn typed_clutch_runs_through_the_cli() {
    let directory = TemporaryDirectory::new();
    typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
    let program = directory.path().join("cli.slug");
    fs::write(
        &program,
        "val files = import(\"fixture.files\")\nprint(files.describe(files.add(40, 2)))\n",
    )
    .expect("write CLI program");
    let output = Command::new(env!("CARGO_BIN_EXE_slug"))
        .env("SLUG_HOME", directory.path())
        .arg(&program)
        .output()
        .expect("run slug CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "<42>");
}

#[test]
fn typed_clutch_checks_every_module_it_provides() {
    let directory = TemporaryDirectory::new();
    let manifest = MANIFEST.replace(
        "\"fixture.files\" = { source = \"modules/files.slug\" }\n",
        "\"fixture.files\" = { source = \"modules/files.slug\" }\n\
             \"fixture.more\" = { source = \"modules/more.slug\" }\n",
    ) + "\n[native.functions.\"fixture.more\"]\n\
           sum = { symbol = \"fx_add\", signature = \"(i64 left, i64 right) -> i64\" }\n";
    let repository = typed_clutch_with_modules(
        &directory,
        &[
            ("fixture.files", "files.slug", MODULE),
            (
                "fixture.more",
                "more.slug",
                "export foreign sum = fn(left:num, right:str):num\n",
            ),
        ],
        &manifest,
        &manifest,
    );
    // `fixture.files` activates the clutch; `fixture.more` reuses that
    // activation and must still be checked.
    let error = run(
        &directory,
        repository,
        "val first = files.add(1, 2)\nval more = import(\"fixture.more\")\nmore.sum(1, \"2\")",
    )
    .expect_err("mismatched second module");
    assert!(
        error.contains("foreign `fixture.more.sum` parameter `right` is annotated `str`"),
        "{error}"
    );
}

#[test]
fn typed_clutch_rejects_null_text_for_a_str_result() {
    let directory = TemporaryDirectory::new();
    let repository = typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
    let value = run(
        &directory,
        repository,
        "[files.label(1), files.labelOrNil(1), files.labelOrNil(-1)]",
    )
    .expect("labels");
    assert_eq!(value.to_string(), "[\"positive\", \"positive\", nil]");

    let repository = typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
    let error = run(&directory, repository, "files.label(-1)").expect_err("NULL for `str`");
    assert!(error.starts_with("native.type"), "{error}");
    assert!(
        error.contains("`fixture.files.label` returned NULL for a `str` result"),
        "{error}"
    );
}

const BENCH_WARMUP_CALLS: usize = 10_000;
const BENCH_CALLS_PER_SAMPLE: usize = 100_000;
const BENCH_SAMPLE_COUNT: usize = 11;

/// Raw timing samples for one scalar call through the production typed path:
/// a staged clutch library, the desktop loader, and the shared adapter. The
/// loop matches the 0.13 baseline and the controls in `ffi_prototype.rs`.
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn typed_scalar_add_benchmark() {
    let directory = TemporaryDirectory::new();
    let repository = typed_clutch(&directory, MODULE, MANIFEST, MANIFEST);
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let compile = |calls: usize| {
        loader
            .compile_source(
                &directory.path().join("bench.slug").to_string_lossy(),
                &format!(
                    "val files = import(\"fixture.files\")\n\
                     val repeat = fn(remaining:num) {{\n\
                       if (remaining == 0) {{ 0 }} else {{\n\
                         files.add(remaining, 1)\n\
                         recur(remaining - 1)\n\
                       }}\n\
                     }}\n\
                     repeat({calls})\n"
                ),
            )
            .expect("compile typed scalar benchmark")
    };
    let warmup = compile(BENCH_WARMUP_CALLS);
    let sample = compile(BENCH_CALLS_PER_SAMPLE);
    let mut vm = Vm::with_host(Rc::new(loader.clone()));
    assert_eq!(vm.run_named(&warmup, "main").unwrap().to_string(), "0");
    println!(
        "typed configuration: profile=release samples={BENCH_SAMPLE_COUNT} warmup_calls={BENCH_WARMUP_CALLS} calls_per_sample={BENCH_CALLS_PER_SAMPLE} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    for sample_index in 1..=BENCH_SAMPLE_COUNT {
        let started = std::time::Instant::now();
        assert_eq!(vm.run_named(&sample, "main").unwrap().to_string(), "0");
        let elapsed = started.elapsed();
        #[allow(clippy::cast_precision_loss)]
        let per_call = elapsed.as_secs_f64() * 1_000_000_000.0 / BENCH_CALLS_PER_SAMPLE as f64;
        println!(
            "typed-production sample {sample_index}: total_ns={} ns_per_call={per_call:.2}",
            elapsed.as_nanos()
        );
    }
}
