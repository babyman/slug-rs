use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use slug::DesktopLoader as ModuleLoader;
use slug_desktop_loader::{ClutchRepository, FfiPrototypeLibrary};
use slug_vm::{
    NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus, RuntimeErrorKind, Vm,
};

trait VmWithModuleLoader {
    fn with_module_loader(loader: ModuleLoader) -> Self;
}

impl VmWithModuleLoader for Vm {
    fn with_module_loader(loader: ModuleLoader) -> Self {
        Self::with_host(Rc::new(loader))
    }
}

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "slug-ffi-prototype-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temporary FFI directory");
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

fn compile_fixture(directory: &TemporaryDirectory, source: &str, name: &str) -> PathBuf {
    compile_fixture_with_libraries(directory, source, name, &[])
}

fn current_native_platform() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        (os, architecture) => panic!("unsupported native test platform {os}-{architecture}"),
    }
}

fn build_native_clutch(
    directory: &TemporaryDirectory,
    module_name: &str,
    clutch_name: &str,
    module_source: &str,
    native_source: &str,
    library_name: &str,
    libraries: &[&str],
) -> ClutchRepository {
    let clutch_root = directory.path().join("clutch").join(clutch_name);
    let support_module_name = format!("{module_name}.support");
    fs::create_dir_all(directory.path().join("clutch")).expect("create clutch repository");
    fs::write(
        directory.path().join("clutch/manifest.toml"),
        format!(
            "[modules]\n\"{module_name}\" = \"{clutch_name}\"\n\"{support_module_name}\" = \"{clutch_name}\"\n"
        ),
    )
    .expect("write clutch repository manifest");
    fs::create_dir_all(clutch_root.join("modules")).expect("create clutch modules");
    let target = current_native_platform();
    fs::create_dir_all(clutch_root.join("native/source")).expect("create clutch native source dir");
    fs::create_dir_all(clutch_root.join("native").join(target))
        .expect("create clutch library directory");
    let module_file = Path::new(module_source)
        .file_name()
        .expect("module source has a file name");
    let native_file = Path::new(native_source)
        .file_name()
        .expect("native source has a file name");
    fs::copy(
        workspace_root().join(module_source),
        clutch_root.join("modules").join(module_file),
    )
    .expect("copy clutch declaration");
    if module_name == "slug.db.sqlite" {
        fs::copy(
            workspace_root().join("crates/slug/tests/ffi_prototype/statement.slug"),
            clutch_root.join("modules/statement.slug"),
        )
        .expect("copy SQLite statement module");
        fs::copy(
            workspace_root().join("crates/slug/tests/ffi_prototype/transaction.slug"),
            clutch_root.join("modules/transaction.slug"),
        )
        .expect("copy SQLite transaction module");
    }
    fs::write(
        clutch_root.join("modules/support.slug"),
        "export val kind = \"pure Slug\"\n",
    )
    .expect("write pure Slug companion module");
    fs::copy(
        workspace_root().join(native_source),
        clutch_root.join("native/source").join(native_file),
    )
    .expect("copy clutch native source");
    let built = compile_fixture_with_libraries(directory, native_source, library_name, libraries);
    let library = clutch_root.join("native").join(target).join(
        built
            .file_name()
            .expect("test-built library has a file name"),
    );
    fs::copy(built, &library).expect("place dynamic module in clutch");
    fs::write(
        clutch_root.join("clutch.toml"),
        format!(
            "[modules]\n\
             \"{module_name}\" = {{ source = \"modules/{}\" }}\n\
             \"{support_module_name}\" = {{ source = \"modules/support.slug\" }}\n\n\
             [native]\n\
             source = \"native/source\"\n\
             abi = \"slug-ffi-prototype/0.13\"\n\n\
             [native.libraries]\n\
             \"{target}\" = \"native/{target}/{}\"\n",
            module_file.to_string_lossy(),
            library
                .file_name()
                .expect("native library name")
                .to_string_lossy(),
        ),
    )
    .expect("write native clutch manifest");
    if module_name == "slug.db.sqlite" {
        fs::write(
            clutch_root.join("clutch.toml"),
            format!(
                "[modules]\n\"{module_name}\" = {{ source = \"modules/{}\" }}\n\"{module_name}.statement\" = {{ source = \"modules/statement.slug\" }}\n\"{module_name}.transaction\" = {{ source = \"modules/transaction.slug\" }}\n\"{support_module_name}\" = {{ source = \"modules/support.slug\" }}\n\n[native]\nsource = \"native/source\"\nabi = \"slug-ffi-prototype/0.13\"\n\n[native.libraries]\n\"{target}\" = \"native/{target}/{}\"\n",
                module_file.to_string_lossy(),
                library.file_name().expect("native library name").to_string_lossy(),
            ),
        )
        .expect("write multi-module SQLite clutch manifest");
        fs::write(
            directory.path().join("clutch/manifest.toml"),
            format!("[modules]\n\"{module_name}\" = \"{clutch_name}\"\n\"{module_name}.statement\" = \"{clutch_name}\"\n\"{module_name}.transaction\" = \"{clutch_name}\"\n\"{support_module_name}\" = \"{clutch_name}\"\n"),
        )
        .expect("write multi-module SQLite repository manifest");
    }
    ClutchRepository::from_manifest(directory.path().join("clutch")).expect("load clutch manifest")
}

fn build_core_clutch(directory: &TemporaryDirectory) -> ClutchRepository {
    let clutch_root = directory.path().join("clutch/slug.core.clutch");
    let target = current_native_platform();
    fs::create_dir_all(clutch_root.join("modules")).expect("create core clutch modules");
    fs::create_dir_all(clutch_root.join("native/source"))
        .expect("create core clutch native source");
    fs::create_dir_all(clutch_root.join("native").join(target))
        .expect("create core clutch native library directory");
    fs::write(
        directory.path().join("clutch/manifest.toml"),
        "[modules]\n\
         \"slug.std\" = \"slug.core.clutch\"\n\
         \"slug.io.stdin\" = \"slug.core.clutch\"\n\
         \"slug.math\" = \"slug.core.clutch\"\n",
    )
    .expect("write core clutch repository manifest");
    for (source, destination) in [
        ("clutch/slug.core.clutch/modules/std.slug", "std.slug"),
        ("crates/slug/tests/ffi_prototype/stdin.slug", "stdin.slug"),
        ("crates/slug/tests/ffi_prototype/math.slug", "math.slug"),
        ("crates/slug/tests/ffi_prototype/core.c", "core.c"),
    ] {
        let destination = if destination == "core.c" {
            clutch_root.join("native/source").join(destination)
        } else {
            clutch_root.join("modules").join(destination)
        };
        fs::copy(workspace_root().join(source), destination).expect("copy core clutch source");
    }
    let built = compile_fixture(directory, "crates/slug/tests/ffi_prototype/core.c", "core");
    let library = clutch_root
        .join("native")
        .join(target)
        .join(built.file_name().expect("core native library name"));
    fs::copy(built, &library).expect("place core native library");
    fs::write(
        clutch_root.join("clutch.toml"),
        format!(
            "[modules]\n\
             \"slug.std\" = {{ source = \"modules/std.slug\" }}\n\
             \"slug.io.stdin\" = {{ source = \"modules/stdin.slug\" }}\n\
             \"slug.math\" = {{ source = \"modules/math.slug\" }}\n\n\
             [native]\n\
             source = \"native/source\"\n\
             abi = \"slug-ffi-prototype/0.13\"\n\n\
             [native.libraries]\n\
             \"{target}\" = \"native/{target}/{}\"\n",
            library
                .file_name()
                .expect("core native library name")
                .to_string_lossy(),
        ),
    )
    .expect("write core clutch manifest");
    ClutchRepository::from_manifest(directory.path().join("clutch"))
        .expect("load core clutch repository")
}

fn run_clutch_cli(directory: &TemporaryDirectory, source: &str) -> std::process::Output {
    let program = directory.path().join("cli.slug");
    fs::write(&program, source).expect("write native clutch CLI program");
    Command::new(env!("CARGO_BIN_EXE_slug"))
        .env("SLUG_HOME", directory.path())
        .arg(program)
        .output()
        .expect("run CLI through native clutch layout")
}

fn slug_path_literal(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[test]
fn slug_path_literal_escapes_slug_string_delimiters() {
    assert_eq!(
        slug_path_literal(Path::new(r#"C:\workspace\quoted"name.txt"#)),
        r#"C:\\workspace\\quoted\"name.txt"#
    );
}

#[test]
fn loads_the_filesystem_clutch_through_a_test_built_dynamic_module() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.io.fs",
        "slug.io.fs.clutch",
        "crates/slug/tests/ffi_prototype/fs.slug",
        "crates/slug/tests/ffi_prototype/fs.c",
        "slug_io_fs",
        &[],
    );
    let cli_file = directory.path().join("cli-written.txt");
    let cli_program = directory.path().join("cli.slug");
    fs::write(
        &cli_program,
        format!(
            "val fs = import(\"slug.io.fs\")\n\
             val output:fs.File = fs.openWrite(\"{}\")\n\
             fs.write(output, \"from cli\")\n\
             fs.close(output)\n",
            slug_path_literal(&cli_file)
        ),
    )
    .expect("write native clutch CLI program");
    let status = Command::new(env!("CARGO_BIN_EXE_slug"))
        .env("SLUG_HOME", directory.path())
        .arg(&cli_program)
        .status()
        .expect("run CLI through native clutch layout");
    assert!(status.success(), "CLI native clutch run failed: {status}");
    assert_eq!(
        fs::read_to_string(&cli_file).expect("read CLI native output"),
        "from cli"
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let file = directory.path().join("written.txt");
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
            &format!(
                "val fs = import(\"slug.io.fs\")\n\
                 val output:fs.File = fs.openWrite(\"{}\")\n\
                 defer fs.close(output)\n\
                 fs.write(output, \"hello\")\n\
                 fs.close(output)\n\
                 val input:fs.File = fs.openRead(\"{}\")\n\
                 defer fs.close(input)\n\
                 export val line:str|nil = fs.readLine(input)\n\
                 export val eof:str|nil = fs.readLine(input)\n",
                slug_path_literal(&file),
                slug_path_literal(&file)
            ),
        )
        .expect("compile filesystem clutch consumer");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("run dynamic filesystem clutch consumer");
    assert_eq!(
        fs::read_to_string(&file).expect("read dynamic output"),
        "hello"
    );
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"line\": \"hello\", \"eof\": nil}"
    );
    vm.shutdown();
    assert_eq!(
        vm.run_named(&program, "main")
            .expect_err("shutdown VM rejects new work")
            .kind,
        RuntimeErrorKind::InvalidCall
    );
}

#[test]
fn filesystem_clutch_rejects_lines_larger_than_its_memory_limit() {
    let directory = TemporaryDirectory::new();
    let _repository = build_native_clutch(
        &directory,
        "slug.io.fs",
        "slug.io.fs.clutch",
        "crates/slug/tests/ffi_prototype/fs.slug",
        "crates/slug/tests/ffi_prototype/fs.c",
        "limited_slug_io_fs",
        &[],
    );
    let file = directory.path().join("too-long.txt");
    fs::write(&file, vec![b'x'; 16 * 1024 * 1024 + 1]).expect("write oversized line");
    let output = run_clutch_cli(
        &directory,
        &format!(
            "val fs = import(\"slug.io.fs\")\n\
             val input = fs.openRead(\"{}\")\n\
             fs.readLine(input)\n",
            slug_path_literal(&file)
        ),
    );
    assert!(!output.status.success(), "oversized line must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("file line exceeds 16 MiB limit"),
        "oversized line returned an unexpected diagnostic: {stderr}"
    );
}

#[test]
fn one_native_clutch_supports_pure_and_native_backed_modules() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.io.fs",
        "slug.io.fs.clutch",
        "crates/slug/tests/ffi_prototype/fs.slug",
        "crates/slug/tests/ffi_prototype/fs.c",
        "shared_slug_io_fs",
        &[],
    );
    let file = directory.path().join("shared.txt");
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &directory.path().join("shared.slug").to_string_lossy(),
            &format!(
                "val support = import(\"slug.io.fs.support\")\n\
                 val fs = import(\"slug.io.fs\")\n\
                 val output:fs.File = fs.openWrite(\"{}\")\n\
                 fs.write(output, support.kind)\n\
                 fs.close(output)\n",
                slug_path_literal(&file)
            ),
        )
        .expect("compile mixed clutch consumer");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&program, "main")
        .expect("load pure and native-backed modules from one clutch");
    assert_eq!(
        fs::read_to_string(file).expect("read mixed clutch output"),
        "pure Slug"
    );
}

fn compile_fixture_with_libraries(
    directory: &TemporaryDirectory,
    source: &str,
    name: &str,
    libraries: &[&str],
) -> PathBuf {
    let output = directory.path().join(format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        name,
        std::env::consts::DLL_SUFFIX
    ));
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("cc");
        command.args(["-I", "include", source]);
        if cfg!(target_os = "macos") {
            command.arg("-dynamiclib");
        } else {
            command.args(["-shared", "-fPIC"]);
        }
        command
            .arg("-o")
            .arg(output.to_str().expect("temporary library path is UTF-8"))
            .arg("-lm")
            .args(libraries);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let _ = libraries;
        let mut command = Command::new("clang");
        command
            .args(["-shared", "-I", "include", source, "-o"])
            .arg(output.to_str().expect("temporary library path is UTF-8"));
        command
    };
    command.current_dir(workspace_root());
    let status = command.status().expect("start C compiler");
    assert!(status.success(), "compile C fixture");
    output
}

fn scalar_add_program(
    directory: &TemporaryDirectory,
    calls: usize,
) -> (ModuleLoader, slug_vm::Program) {
    fs::create_dir_all(directory.path().join("slug")).expect("create scalar fixture module dir");
    fs::write(
        directory.path().join("slug/scalar.slug"),
        "export foreign add = fn(left:num, right:num):num\n",
    )
    .expect("write scalar fixture module source");
    let loader = ModuleLoader::new(directory.path(), None);
    let main = directory.path().join("scalar-benchmark.slug");
    let source = format!(
        "val scalar = import(\"slug.scalar\")\n\
         val repeat = fn(remaining:num) {{\n\
           if (remaining == 0) {{ 0 }} else {{\n\
             scalar.add(remaining, 1)\n\
             recur(remaining - 1)\n\
           }}\n\
         }}\n\
         repeat({calls})\n"
    );
    let program = loader
        .compile_source(&main.to_string_lossy(), &source)
        .expect("compile scalar callback program");
    (loader, program)
}

fn scalar_add_slug_program(
    directory: &TemporaryDirectory,
    calls: usize,
) -> (ModuleLoader, slug_vm::Program) {
    let loader = ModuleLoader::new(directory.path(), None);
    let main = directory.path().join("scalar-slug-benchmark.slug");
    let source = format!(
        "val add = fn(left, right) {{ left + right }}\n\
         val repeat = fn(remaining:num) {{\n\
           if (remaining == 0) {{ 0 }} else {{\n\
             add(remaining, 1)\n\
             recur(remaining - 1)\n\
           }}\n\
         }}\n\
         repeat({calls})\n"
    );
    let program = loader
        .compile_source(&main.to_string_lossy(), &source)
        .expect("compile scalar Slug benchmark program");
    (loader, program)
}

fn native_scalar_add(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let left = call.argument(0)?.as_i64()?;
        let right = call.argument(1)?.as_i64()?;
        let value = left.checked_add(right).ok_or_else(|| {
            slug_vm::NativeError::new("native.range", "integer addition overflow")
        })?;
        Ok(NativeOwnedValue::integer(value))
    })();
    match result {
        Ok(value) => call.return_value(value),
        Err(error) => call.raise(error),
    }
}

fn scalar_add_static_rust_function() -> slug_vm::NativeFunction {
    NativeModule::new("slug.scalar", ())
        .expect("static scalar module is valid")
        .function("add", NativeArity::Exact(2), native_scalar_add)
        .expect("static scalar function is valid")
}

const SCALAR_WARMUP_CALLS: usize = 10_000;
const SCALAR_CALLS_PER_SAMPLE: usize = 100_000;
const SCALAR_CALLS_PER_SAMPLE_F64: f64 = 100_000.0;
const SCALAR_SAMPLE_COUNT: usize = 11;

fn report_scalar_samples(label: &str, vm: &mut Vm, sample: &slug_vm::Program) {
    for sample_index in 1..=SCALAR_SAMPLE_COUNT {
        let started = Instant::now();
        assert_eq!(vm.run_named(sample, "main").unwrap().to_string(), "0");
        let elapsed = started.elapsed();
        println!(
            "{label} sample {sample_index}: total_ns={} ns_per_call={:.2}",
            elapsed.as_nanos(),
            elapsed.as_secs_f64() * 1_000_000_000.0 / SCALAR_CALLS_PER_SAMPLE_F64,
        );
    }
}

/// Same loop as the call controls with the call removed, so the loop's own
/// compare, subtract, and `recur` cost can be subtracted from every other
/// control's ns/call.
fn measure_empty_loop_control() {
    let directory = TemporaryDirectory::new();
    let loader = ModuleLoader::new(directory.path(), None);
    let main = directory.path().join("scalar-empty-loop-benchmark.slug");
    let compile = |calls: usize| {
        let source = format!(
            "val repeat = fn(remaining:num) {{\n\
               if (remaining == 0) {{ 0 }} else {{\n\
                 recur(remaining - 1)\n\
               }}\n\
             }}\n\
             repeat({calls})\n"
        );
        loader
            .compile_source(&main.to_string_lossy(), &source)
            .expect("compile empty-loop benchmark program")
    };
    let warmup = compile(SCALAR_WARMUP_CALLS);
    let sample = compile(SCALAR_CALLS_PER_SAMPLE);
    let mut vm = Vm::with_module_loader(loader);
    assert_eq!(vm.run_named(&warmup, "main").unwrap().to_string(), "0");
    report_scalar_samples("empty-loop-control", &mut vm, &sample);
}

fn measure_slug_function_control() {
    let directory = TemporaryDirectory::new();
    let (loader, warmup) = scalar_add_slug_program(&directory, SCALAR_WARMUP_CALLS);
    let (_, sample) = scalar_add_slug_program(&directory, SCALAR_CALLS_PER_SAMPLE);
    let mut vm = Vm::with_module_loader(loader);
    assert_eq!(vm.run_named(&warmup, "main").unwrap().to_string(), "0");
    report_scalar_samples("slug-function-control", &mut vm, &sample);
}

fn measure_static_rust_native_control() {
    let directory = TemporaryDirectory::new();
    let (loader, warmup) = scalar_add_program(&directory, SCALAR_WARMUP_CALLS);
    let (_, sample) = scalar_add_program(&directory, SCALAR_CALLS_PER_SAMPLE);
    let mut vm = Vm::with_module_loader(loader);
    vm.define_foreign(scalar_add_static_rust_function())
        .expect("register static Rust benchmark control");
    assert_eq!(vm.run_named(&warmup, "main").unwrap().to_string(), "0");
    report_scalar_samples("static-rust-native-control", &mut vm, &sample);
}

#[test]
fn scalar_add_callback_fixture_uses_the_013_host_table_path() {
    let directory = TemporaryDirectory::new();
    let (loader, program) = scalar_add_program(&directory, 3);
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/scalar_add_module.c",
        "scalar_add",
    );
    let module = FfiPrototypeLibrary::load(library).expect("load scalar callback fixture");
    let mut vm = Vm::with_module_loader(loader);
    module
        .register(&mut vm)
        .expect("register scalar callback fixture");

    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "0");
}

/// Produces local, raw timing samples for the frozen 0.13 host-table path.
///
/// This stays ignored because benchmark numbers are local evidence, not an
/// assertion suitable for a shared CI machine.
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn scalar_add_callback_baseline() {
    const WARMUP_CALLS: usize = 10_000;
    const CALLS_PER_SAMPLE: usize = 100_000;
    const CALLS_PER_SAMPLE_F64: f64 = 100_000.0;
    const SAMPLE_COUNT: usize = 11;

    let directory = TemporaryDirectory::new();
    let (loader, warmup) = scalar_add_program(&directory, WARMUP_CALLS);
    let (_, sample) = scalar_add_program(&directory, CALLS_PER_SAMPLE);
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/scalar_add_module.c",
        "scalar_add",
    );
    let module = FfiPrototypeLibrary::load(library).expect("load scalar callback fixture");
    let mut vm = Vm::with_module_loader(loader);
    module
        .register(&mut vm)
        .expect("register scalar callback fixture");
    assert_eq!(vm.run_named(&warmup, "main").unwrap().to_string(), "0");

    println!(
        "configuration: profile=release samples={SAMPLE_COUNT} warmup_calls={WARMUP_CALLS} calls_per_sample={CALLS_PER_SAMPLE} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    for sample_index in 1..=SAMPLE_COUNT {
        let started = Instant::now();
        assert_eq!(vm.run_named(&sample, "main").unwrap().to_string(), "0");
        let elapsed = started.elapsed();
        println!(
            "sample {sample_index}: total_ns={} ns_per_call={:.2}",
            elapsed.as_nanos(),
            elapsed.as_secs_f64() * 1_000_000_000.0 / CALLS_PER_SAMPLE_F64,
        );
    }
}

/// VM controls for the scalar call benchmarks: the loop alone, an ordinary
/// Slug function, and a statically registered Rust native. The production
/// typed path is measured in `typed_native.rs`.
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn scalar_call_controls() {
    println!(
        "controls configuration: profile=release samples={SCALAR_SAMPLE_COUNT} warmup_calls={SCALAR_WARMUP_CALLS} calls_per_sample={SCALAR_CALLS_PER_SAMPLE} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    measure_empty_loop_control();
    measure_slug_function_control();
    measure_static_rust_native_control();
}

#[cfg(unix)]
#[test]
fn sqlite_database_clutch_binds_slug_values_and_returns_rows() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.db.sqlite",
        "slug.db.sqlite.clutch",
        "crates/slug/tests/ffi_prototype/sqlite.slug",
        "crates/slug/tests/ffi_prototype/sqlite.c",
        "slug_db_sqlite",
        &["-lsqlite3"],
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
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
        )
        .expect("compile SQLite clutch consumer");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("run SQLite clutch consumer");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"rows\": [{\"id\": 1, \"name\": \"Alice\"}, {\"id\": 2, \"name\": \"Bob\"}]}"
    );
    vm.shutdown();
}

#[cfg(unix)]
#[test]
fn sqlite_clutch_preserves_database_and_statement_identities() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.db.sqlite",
        "slug.db.sqlite.clutch",
        "crates/slug/tests/ffi_prototype/sqlite.slug",
        "crates/slug/tests/ffi_prototype/sqlite.c",
        "slug_db_sqlite",
        &["-lsqlite3"],
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let main = directory.path().join("main.slug");
    loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val statement = import(\"slug.db.sqlite.statement\")\n\
             val db = sqlite.open(\":memory:\")\n\
             val stmt = statement.prepare(db, \"select 1\")\n\
             statement.close(stmt)\n\
             sqlite.close(db)\n",
        )
        .expect("SQLite exports retain their nominal identities through the clutch snapshot");
    let error = loader
        .compile_source(
            &main.to_string_lossy(),
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

#[cfg(unix)]
#[test]
fn sqlite_transaction_module_commits_successful_work() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.db.sqlite",
        "slug.db.sqlite.clutch",
        "crates/slug/tests/ffi_prototype/sqlite.slug",
        "crates/slug/tests/ffi_prototype/sqlite.c",
        "slug_db_sqlite",
        &["-lsqlite3"],
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
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
        )
        .expect("compile successful transaction");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("commit successful transaction");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"outcome\": \"committed\", \"rows\": [{\"name\": \"Alice\"}]}"
    );
    vm.shutdown();
}

#[cfg(unix)]
#[test]
fn sqlite_transaction_module_rolls_back_and_rethrows_work_errors() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.db.sqlite",
        "slug.db.sqlite.clutch",
        "crates/slug/tests/ffi_prototype/sqlite.slug",
        "crates/slug/tests/ffi_prototype/sqlite.c",
        "slug_db_sqlite",
        &["-lsqlite3"],
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
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
        )
        .expect("compile failing transaction");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("recover the rethrown transaction error");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"error\": \"transaction failed\", \"rows\": []}"
    );
    vm.shutdown();
}

#[cfg(unix)]
#[test]
fn sqlite_statement_failures_clear_partial_bindings_before_reuse() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.db.sqlite",
        "slug.db.sqlite.clutch",
        "crates/slug/tests/ffi_prototype/sqlite.slug",
        "crates/slug/tests/ffi_prototype/sqlite.c",
        "slug_db_sqlite",
        &["-lsqlite3"],
    );
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
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
        )
        .expect("compile statement cleanup program");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("reuse statements after failed partial bindings");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"rows\": [{\"left_value\": \"old\", \"right_value\": \"stale\"}, {\"left_value\": \"fresh\", \"right_value\": nil}], \"query_rows\": [{\"left_value\": \"fresh\", \"right_value\": nil}]}"
    );
    vm.shutdown();
}

#[test]
fn loads_math_through_the_native_core_clutch() {
    let directory = TemporaryDirectory::new();
    let main = directory.path().join("main.slug");
    let repository = build_core_clutch(&directory);
    let output = run_clutch_cli(
        &directory,
        "val math = import(\"slug.math\")\nprintln(math.add(20, 22), math.sqrt(9.0))\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42 3\n");
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val math = import(\"slug.math\")\nmath.add(20, 22) + math.sqrt(9.0)\n",
        )
        .expect("compile program using math clutch");
    let mut vm = Vm::with_module_loader(loader.clone());
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "45");

    let failing = loader
        .compile_source(
            &main.to_string_lossy(),
            "val math = import(\"slug.math\")\nmath.sqrt(-1.0)\n",
        )
        .expect("compile failing math call");
    let error = vm
        .run_named(&failing, "main")
        .expect_err("negative square root must be checked");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("math.domain")
    );
}

#[test]
fn rejects_a_c_module_with_an_incompatible_abi_major() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/invalid_module.c",
        "invalid",
    );
    let Err(error) = FfiPrototypeLibrary::load(library) else {
        panic!("incompatible ABI must fail");
    };
    assert!(error.to_string().contains("ABI major 99"));
}

#[test]
fn rejects_an_undersized_c_function_descriptor() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/undersized_function_module.c",
        "undersized",
    );
    let Err(error) = FfiPrototypeLibrary::load(library) else {
        panic!("undersized descriptor must fail");
    };
    assert!(error.to_string().contains("undersized function descriptor"));
}

#[test]
fn rejects_stale_c_collection_handles_without_corrupting_memory() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/invalid_collection_handle_module.c",
        "invalid_collection_handle",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/handles.slug"),
        "export foreign staleMap = fn():nil\n",
    )
    .expect("write collection-handle module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val handles = import(\"slug.handles\")\nhandles.staleMap()\n",
        )
        .expect("compile program using collection-handle module");
    let module = FfiPrototypeLibrary::load(library).expect("load collection-handle module");
    let mut vm = Vm::with_module_loader(loader);
    module
        .register(&mut vm)
        .expect("register collection-handle module");
    let error = vm
        .run_named(&program, "main")
        .expect_err("stale collection handle must be rejected");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("native.contract")
    );
    assert!(error.message.contains("FFI map handle is invalid"));
}

#[test]
fn rejects_a_c_resource_type_without_a_destroy_callback() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/missing_resource_destroy_module.c",
        "missing_resource_destroy",
    );
    let Err(error) = FfiPrototypeLibrary::load(library) else {
        panic!("resource descriptor without a destructor must fail");
    };
    assert!(error.to_string().contains("has no destroy callback"));
}

#[test]
fn rejects_source_resource_types_missing_from_the_native_module() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/resource_module.c",
        "resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Missing\n\
         export foreign create = fn(value:num):Missing\n",
    )
    .expect("write mismatched resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.create(1)\n",
        )
        .expect("compile program using mismatched resource module");

    let error = vm
        .run_named(&program, "main")
        .expect_err("the missing public resource registration must fail");
    assert_eq!(error.kind, RuntimeErrorKind::Module);
    assert!(
        error
            .to_string()
            .contains("does not register declared resource type `Missing`")
    );
}

#[test]
fn rejects_native_resource_types_missing_from_the_source_module() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/resource_module.c",
        "resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export foreign destroyed = fn():num\n",
    )
    .expect("write mismatched resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.destroyed()\n",
        )
        .expect("compile program using mismatched resource module");

    let error = vm
        .run_named(&program, "main")
        .expect_err("the extra native resource registration must fail");
    assert_eq!(error.kind, RuntimeErrorKind::Module);
    assert!(
        error
            .to_string()
            .contains("registers resource type `Counter` without a matching source declaration")
    );
}

#[test]
fn turns_an_unknown_c_status_into_a_checked_contract_error() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/unknown_status_module.c",
        "unknown_status",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/status.slug"),
        "export foreign status = fn():nil\n",
    )
    .expect("write status module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val status = import(\"slug.status\")\nstatus.status()\n",
        )
        .expect("compile program using status module");
    let module = FfiPrototypeLibrary::load(library).expect("load status module");
    let mut vm = Vm::with_module_loader(loader);
    module.register(&mut vm).expect("register status module");
    let error = vm
        .run_named(&program, "main")
        .expect_err("unknown C status must fail");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(error.message.contains("unknown status 99"));
}

#[test]
fn dispatches_same_arity_c_functions_by_opaque_member_key() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/same_arity_module.c",
        "same_arity",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/same.slug"),
        "export foreign first = fn():num\nexport foreign second = fn():num\n",
    )
    .expect("write same-arity module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val same = import(\"slug.same\")\nsame.first() + same.second()\n",
        )
        .expect("compile program using same-arity module");
    let module = FfiPrototypeLibrary::load(library).expect("load same-arity module");
    let mut vm = Vm::with_module_loader(loader);
    module
        .register(&mut vm)
        .expect("register same-arity module");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "3");
}

#[test]
fn unloads_libraries_after_destroying_each_library_state() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/stateful_module.c",
        "stateful",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/stateful.slug"),
        "export foreign stateInfo = fn():num\n",
    )
    .expect("write stateful module source");

    for expected in [100, 100] {
        let main = directory.path().join("main.slug");
        let loader = ModuleLoader::new(directory.path(), None);
        let program = loader
            .compile_source(
                &main.to_string_lossy(),
                "val stateful = import(\"slug.stateful\")\nstateful.stateInfo()\n",
            )
            .expect("compile program using stateful module");
        let module = FfiPrototypeLibrary::load(&library).expect("load stateful module");
        let mut vm = Vm::with_module_loader(loader);
        module.register(&mut vm).expect("register stateful module");
        assert_eq!(
            vm.run_named(&program, "main").unwrap().to_string(),
            expected.to_string()
        );
    }
}

#[test]
fn rejects_stale_native_functions_after_explicit_plugin_shutdown() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/stateful_module.c",
        "inactive_stateful",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/stateful.slug"),
        "export foreign stateInfo = fn():num\n",
    )
    .expect("write stateful module source");
    let loader = ModuleLoader::new(directory.path(), None);
    let program = loader
        .compile_source(
            &directory.path().join("main.slug").to_string_lossy(),
            "val stateful = import(\"slug.stateful\")\nstateful.stateInfo()\n",
        )
        .expect("compile stateful module consumer");
    let module = FfiPrototypeLibrary::load(library).expect("load stateful module");
    let mut vm = Vm::with_module_loader(loader);
    module.register(&mut vm).expect("register stateful module");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "100");

    module.shutdown();
    let error = vm
        .run_named(&program, "main")
        .expect_err("stale function must not enter unloaded native code");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("native.plugin_inactive")
    );
}

#[test]
fn owns_c_resources_with_checked_borrow_and_close_semantics() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/resource_module.c",
        "resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Counter\n\
         export foreign create = fn(value:num):Counter\n\
         export foreign read = fn(handle:Counter):num\n\
         export foreign close = fn(handle:Counter):num\n\
         export foreign destroyed = fn():num\n",
    )
    .expect("write resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(&library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");

    let success = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\n\
             val counter = resources.create(41)\n\
             resources.read(counter) + resources.close(counter) + resources.destroyed()\n",
        )
        .expect("compile resource ownership program");
    assert_eq!(vm.run_named(&success, "main").unwrap().to_string(), "43");

    let closed = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\n\
             val counter = resources.create(7)\n\
             resources.close(counter)\n\
             resources.read(counter)\n",
        )
        .expect("compile closed-resource program");
    let error = vm
        .run_named(&closed, "main")
        .expect_err("C callbacks cannot borrow closed resources");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("native.resource_closed")
    );
}

#[test]
fn validates_resource_arguments_through_dynamic_foreign_calls() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/resource_module.c",
        "resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Counter\n\
         export foreign read = fn(handle:Counter):num\n",
    )
    .expect("write resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\n\
             val invoke = fn(callback, value) { callback(value) }\n\
             invoke(resources.read, 1)\n",
        )
        .expect("compile dynamically dispatched foreign call");

    let error = vm
        .run_named(&program, "main")
        .expect_err("dynamic foreign calls must validate declared resource arguments");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(
        error
            .message
            .contains("non-resource argument where `slug.resources.Counter` is declared")
    );
}

#[test]
fn rejects_foreign_resource_results_with_the_wrong_declared_type() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/wrong_resource_result_module.c",
        "wrong_resource_result",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resource_result.slug"),
        "export resource Counter\n\
         export resource Other\n\
         export foreign create = fn():Other\n",
    )
    .expect("write resource result module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resource_result\")\nresources.create()\n",
        )
        .expect("compile resource result program");

    let error = vm
        .run_named(&program, "main")
        .expect_err("foreign results must match their declared resource type");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(
        error
            .message
            .contains("wrong resource type for its result; expected `slug.resource_result.Other`")
    );
}

#[test]
fn keeps_same_named_resources_in_distinct_module_namespaces() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/same_named_resource_modules.c",
        "same_named_resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/alpha.slug"),
        "export resource Handle\n\
         export foreign create = fn():Handle\n\
         export foreign read = fn(handle:Handle):num\n",
    )
    .expect("write alpha source module");
    fs::write(
        directory.path().join("slug/beta.slug"),
        "export resource Handle\n\
         export foreign create = fn():Handle\n\
         export foreign read = fn(handle:Handle):num\n",
    )
    .expect("write beta source module");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let library = FfiPrototypeLibrary::load(library).expect("load C library");
    let mut vm = Vm::with_module_loader(loader.clone());
    library
        .register(&mut vm)
        .expect("register both C module descriptors");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val alpha = import(\"slug.alpha\")\n\
             val beta = import(\"slug.beta\")\n\
             val alpha_handle = alpha.create()\n\
             val beta_handle = beta.create()\n\
             [alpha.read(alpha_handle), beta.read(beta_handle)]\n",
        )
        .expect("compile same-named resource program");

    assert_eq!(
        vm.run_named(&program, "main")
            .expect("read resources from their declaring modules")
            .to_string(),
        "[1, 2]"
    );
}

#[test]
fn cleans_up_c_resources_during_error_unwinding_and_vm_teardown() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/resource_module.c",
        "cleanup_resources",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Counter\n\
         export foreign create = fn(value:num):Counter\n\
         export foreign destroyed = fn():num\n",
    )
    .expect("write resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(&library).expect("load C resource module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C resource module");

    let unwinding = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\n\
             val attempt = fn() { val counter = resources.create(1); throw \"stop\" }\n\
             attempt()\n",
        )
        .expect("compile unwinding program");
    vm.run_named(&unwinding, "main")
        .expect_err("throw must unwind the C resource");

    let destroyed = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.destroyed()\n",
        )
        .expect("compile destruction counter program");
    assert_eq!(vm.run_named(&destroyed, "main").unwrap().to_string(), "1");

    let create = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.create(2)\n",
        )
        .expect("compile escaping resource program");
    let escaped = vm
        .run_named(&create, "main")
        .expect("create resource that outlives the VM");
    drop(vm);
    drop(loader);

    let replacement_loader = ModuleLoader::new(directory.path(), None);
    let mut replacement = Vm::with_module_loader(replacement_loader.clone());
    let replacement_module = FfiPrototypeLibrary::load(&library).expect("reload C resource module");
    replacement_module
        .register(&mut replacement)
        .expect("register module in replacement VM");
    let destroyed = replacement_loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.destroyed()\n",
        )
        .expect("compile replacement destruction counter program");
    assert_eq!(
        replacement
            .run_named(&destroyed, "main")
            .unwrap()
            .to_string(),
        "2"
    );
    drop(escaped);
}

#[cfg(not(windows))]
#[test]
fn lets_a_c_thread_send_through_an_owned_producer_capability() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/async_module.c",
        "async",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/async.slug"),
        "export foreign delayed = fn():chan<num>\n",
    )
    .expect("write async module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C async module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module.register(&mut vm).expect("register C async module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val async = import(\"slug.async\")\n\
             select { recv async.delayed() }\n",
        )
        .expect("compile C async producer program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "73");
}

#[cfg(not(windows))]
#[test]
fn lets_a_c_producer_retain_and_retry_an_integer_after_backpressure() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/backpressure_module.c",
        "backpressure",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/backpressure.slug"),
        "export foreign backpressured = fn():chan<num>;\n\
         export foreign sawFull = fn():num\n",
    )
    .expect("write backpressure module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C backpressure module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C backpressure module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val backpressure = import(\"slug.backpressure\")\n\
             val inbox = backpressure.backpressured()\n\
             val first = select { recv inbox }\n\
             val second = select { recv inbox }\n\
             first * 10 + second + backpressure.sawFull()\n",
        )
        .expect("compile C backpressure program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "13");
}

#[cfg(not(windows))]
#[test]
fn reports_closed_when_slug_revokes_a_c_producer_receiver() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/revocation_module.c",
        "revocation",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/revocation.slug"),
        "export foreign delayed = fn():chan<num>;\n\
         export foreign waitStatus = fn():num\n",
    )
    .expect("write revocation module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C revocation module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C revocation module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val revocation = import(\"slug.revocation\")\n\
             val discard = fn() { val inbox = revocation.delayed(); nil }\n\
             discard()\n\
             revocation.waitStatus()\n",
        )
        .expect("compile C producer revocation program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "2");
}

#[cfg(not(windows))]
#[test]
fn transfers_owned_c_text_only_after_a_backpressured_retry_succeeds() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/text_backpressure_module.c",
        "text_backpressure",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/textbackpressure.slug"),
        "export foreign backpressuredText = fn():chan<str>;\n\
         export foreign sawFull = fn():num\n\
         export foreign freed = fn():num\n",
    )
    .expect("write text backpressure module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C text backpressure module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C text backpressure module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val text = import(\"slug.textbackpressure\")\n\
             val inbox = text.backpressuredText()\n\
             val first = select { recv inbox }\n\
             val second = select { recv inbox }\n\
             first + \":\" + second + \":\" + text.sawFull() + \":\" + text.freed()\n",
        )
        .expect("compile C text backpressure program");
    assert_eq!(
        vm.run_named(&program, "main").unwrap().to_string(),
        "first:second:1:2"
    );
}

#[test]
fn producer_abi_013_sends_owned_scalar_and_byte_values_then_closes() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "crates/slug-vm/tests/ffi/producer_values_module.c",
        "producer_values",
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/producer.slug"),
        "export foreign values = fn():chan<any|nil>;\n\
         export foreign accepted = fn():num;\n\
         export foreign closedStatus = fn():num;\n\
         export foreign bytesFreed = fn():num\n",
    )
    .expect("write producer module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeLibrary::load(library).expect("load C producer module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module
        .register(&mut vm)
        .expect("register C producer module");
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val producer = import(\"slug.producer\")\n\
             val inbox = producer.values()\n\
             select { recv inbox }\n\
             select { recv inbox }\n\
             val decimal = select { recv inbox }\n\
             select { recv inbox }\n\
             decimal + producer.accepted() + producer.closedStatus() + producer.bytesFreed()\n",
        )
        .expect("compile ABI 0.13 producer program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "8.5");
}

/// Builds the current typed `slug.io.fs` clutch into a repository in
/// `directory`, as clutch staging does.
fn build_typed_filesystem_clutch(directory: &TemporaryDirectory) -> ClutchRepository {
    let source = workspace_root().join("clutch/slug.io.fs.clutch");
    let repository = directory.path().join("clutch");
    let root = repository.join("slug.io.fs.clutch");
    for file in [
        "clutch.toml",
        "modules/fs.slug",
        "native/source/fs.c",
        "native/source/slug_io_fs.h",
    ] {
        let target = root.join(file);
        fs::create_dir_all(target.parent().expect("clutch file has a parent"))
            .expect("create typed clutch directory");
        fs::copy(source.join(file), target).expect("copy typed filesystem clutch file");
    }
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\"slug.io.fs\" = \"slug.io.fs.clutch\"\n",
    )
    .expect("write typed repository manifest");
    let manifest = slug_native_manifest::NativeManifest::from_clutch_source(
        &fs::read_to_string(root.join("clutch.toml")).expect("read typed manifest"),
    )
    .expect("valid typed filesystem manifest");
    slug_native_manifest::build_dynamic(&root, &manifest, "slug_io_fs", current_native_platform())
        .unwrap_or_else(|error| panic!("build typed filesystem clutch: {error}"));
    ClutchRepository::from_manifest(&repository).expect("load typed filesystem repository")
}

fn build_prototype_filesystem_clutch(
    directory: &TemporaryDirectory,
    library_name: &str,
) -> ClutchRepository {
    build_native_clutch(
        directory,
        "slug.io.fs",
        "slug.io.fs.clutch",
        "crates/slug/tests/ffi_prototype/fs.slug",
        "crates/slug/tests/ffi_prototype/fs.c",
        library_name,
        &[],
    )
}

/// The typed clutch keeps the prototype's observable behavior on success
/// paths and for use after close.
#[test]
fn typed_filesystem_matches_the_prototype_end_to_end() {
    let run = |directory: &TemporaryDirectory| {
        let input = directory.path().join("input.txt");
        let output = directory.path().join("output.txt");
        fs::write(&input, "first\r\n\nlast").expect("write parity input");
        let result = run_clutch_cli(
            directory,
            &format!(
                "val fs = import(\"slug.io.fs\")\n\
                 val input:fs.File = fs.openRead(\"{}\")\n\
                 defer fs.close(input)\n\
                 println(fs.readLine(input))\n\
                 println(fs.readLine(input))\n\
                 println(fs.readLine(input))\n\
                 println(fs.readLine(input))\n\
                 val output = fs.openWrite(\"{}\")\n\
                 println(fs.write(output, \"one\"))\n\
                 println(fs.close(output))\n\
                 println(fs.close(output))\n\
                 val appended = fs.openAppend(\"{}\")\n\
                 println(fs.write(appended, \" two\"))\n\
                 fs.close(appended)\n\
                 fs.readLine(appended)\n",
                slug_path_literal(&input),
                slug_path_literal(&output),
                slug_path_literal(&output),
            ),
        );
        (
            result.status.code(),
            String::from_utf8_lossy(&result.stdout).into_owned(),
            String::from_utf8_lossy(&result.stderr).contains("native resource is closed"),
            fs::read_to_string(&output).expect("read parity output"),
        )
    };
    let prototype = TemporaryDirectory::new();
    build_prototype_filesystem_clutch(&prototype, "parity_slug_io_fs");
    let typed = TemporaryDirectory::new();
    build_typed_filesystem_clutch(&typed);

    let expected = run(&prototype);
    assert_eq!(
        expected,
        (
            Some(1),
            "first\n\nlast\nnil\n3\nnil\nnil\n4\n".to_owned(),
            true,
            "one two".to_owned()
        )
    );
    assert_eq!(run(&typed), expected);
}

/// Raw end-to-end timing for the filesystem clutch, typed versus prototype:
/// write and flush lines one at a time, then read them back.
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn filesystem_typed_versus_prototype_end_to_end() {
    const LINES: usize = 10_000;
    #[allow(clippy::cast_precision_loss)]
    const OPERATIONS: f64 = (2 * LINES) as f64;
    println!(
        "filesystem configuration: profile=release samples={SCALAR_SAMPLE_COUNT} lines={LINES} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    for label in ["prototype", "typed"] {
        let directory = TemporaryDirectory::new();
        let repository = if label == "typed" {
            build_typed_filesystem_clutch(&directory)
        } else {
            build_prototype_filesystem_clutch(&directory, "bench_slug_io_fs")
        };
        let path = directory.path().join("lines.txt");
        let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
        let program = loader
            .compile_source(
                &directory.path().join("bench.slug").to_string_lossy(),
                &format!(
                    "val fs = import(\"slug.io.fs\")\n\
                     val writeLines = fn(file:fs.File, remaining:num) {{\n\
                       if (remaining == 0) {{ nil }} else {{\n\
                         fs.write(file, \"a line of benchmark text\\n\")\n\
                         recur(file, remaining - 1)\n\
                       }}\n\
                     }}\n\
                     val readLines = fn(file:fs.File, count:num) {{\n\
                       if (fs.readLine(file) == nil) {{ count }} else {{ recur(file, count + 1) }}\n\
                     }}\n\
                     val output = fs.openWrite(\"{path}\")\n\
                     writeLines(output, {LINES})\n\
                     fs.close(output)\n\
                     val input = fs.openRead(\"{path}\")\n\
                     val count = readLines(input, 0)\n\
                     fs.close(input)\n\
                     count\n",
                    path = slug_path_literal(&path),
                ),
            )
            .expect("compile filesystem benchmark");
        let mut vm = Vm::with_module_loader(loader);
        assert_eq!(
            vm.run_named(&program, "main").unwrap().to_string(),
            LINES.to_string()
        );
        for sample_index in 1..=SCALAR_SAMPLE_COUNT {
            let started = Instant::now();
            assert_eq!(
                vm.run_named(&program, "main").unwrap().to_string(),
                LINES.to_string()
            );
            let elapsed = started.elapsed();
            println!(
                "filesystem-{label} sample {sample_index}: total_ns={} ns_per_operation={:.2}",
                elapsed.as_nanos(),
                elapsed.as_secs_f64() * 1_000_000_000.0 / OPERATIONS,
            );
        }
    }
}

/// Builds the current typed `slug.db.sqlite` clutch into a repository in
/// `directory`, as clutch staging does.
#[cfg(unix)]
fn build_typed_sqlite_clutch(directory: &TemporaryDirectory) -> ClutchRepository {
    let source = workspace_root().join("clutch/slug.db.sqlite.clutch");
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
            .expect("create typed clutch directory");
        fs::copy(source.join(file), target).expect("copy typed SQLite clutch file");
    }
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\
         \"slug.db.sqlite\" = \"slug.db.sqlite.clutch\"\n\
         \"slug.db.sqlite.statement\" = \"slug.db.sqlite.clutch\"\n\
         \"slug.db.sqlite.transaction\" = \"slug.db.sqlite.clutch\"\n",
    )
    .expect("write typed repository manifest");
    let manifest = slug_native_manifest::NativeManifest::from_clutch_source(
        &fs::read_to_string(root.join("clutch.toml")).expect("read typed manifest"),
    )
    .expect("valid typed SQLite manifest");
    slug_native_manifest::build_dynamic(
        &root,
        &manifest,
        "slug_db_sqlite",
        current_native_platform(),
    )
    .unwrap_or_else(|error| panic!("build typed SQLite clutch: {error}"));
    ClutchRepository::from_manifest(&repository).expect("load typed SQLite repository")
}

/// Raw end-to-end timing for the `SQLite` clutch, typed versus prototype:
/// rows inserted through a prepared statement, then read back with one query.
#[cfg(unix)]
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn sqlite_typed_versus_prototype_end_to_end() {
    const ROWS: usize = 2_000;
    #[allow(clippy::cast_precision_loss)]
    const ROWS_F64: f64 = ROWS as f64;
    println!(
        "sqlite configuration: profile=release samples={SCALAR_SAMPLE_COUNT} rows={ROWS} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    let source = |query: bool| {
        format!(
            "val sqlite = import(\"slug.db.sqlite\")\n\
             val statement = import(\"slug.db.sqlite.statement\")\n\
             val db = sqlite.open(\":memory:\")\n\
             sqlite.exec(db, \"create table person(id integer, name text, score real)\")\n\
             val insert = statement.prepare(db, \"insert into person(id, name, score) values (?, ?, ?)\")\n\
             val fill = fn(remaining:num) {{\n\
               if (remaining == 0) {{ nil }} else {{\n\
                 statement.exec(insert, remaining, \"person name\", 1.5)\n\
                 recur(remaining - 1)\n\
               }}\n\
             }}\n\
             fill({ROWS})\n\
             statement.close(insert)\n\
             val result = {}\n\
             sqlite.close(db)\n\
             result\n",
            if query {
                "sqlite.query(db, \"select id, name, score from person order by id\")[ROWS_LAST].id"
            } else {
                "0"
            }
            .replace("ROWS_LAST", &(ROWS - 1).to_string())
        )
    };
    for label in ["prototype", "typed"] {
        let directory = TemporaryDirectory::new();
        let repository = if label == "typed" {
            build_typed_sqlite_clutch(&directory)
        } else {
            build_native_clutch(
                &directory,
                "slug.db.sqlite",
                "slug.db.sqlite.clutch",
                "crates/slug/tests/ffi_prototype/sqlite.slug",
                "crates/slug/tests/ffi_prototype/sqlite.c",
                "bench_slug_db_sqlite",
                &["-lsqlite3"],
            )
        };
        let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
        let mut vm = Vm::with_module_loader(loader.clone());
        for (phase, query) in [("insert", false), ("insert-and-query", true)] {
            let program = loader
                .compile_source(
                    &directory
                        .path()
                        .join(format!("{phase}.slug"))
                        .to_string_lossy(),
                    &source(query),
                )
                .expect("compile SQLite benchmark");
            let expected = if query { ROWS.to_string() } else { "0".into() };
            assert_eq!(
                vm.run_named(&program, "main").unwrap().to_string(),
                expected
            );
            for sample_index in 1..=SCALAR_SAMPLE_COUNT {
                let started = Instant::now();
                assert_eq!(
                    vm.run_named(&program, "main").unwrap().to_string(),
                    expected
                );
                let elapsed = started.elapsed();
                println!(
                    "sqlite-{label}-{phase} sample {sample_index}: total_ns={} ns_per_row={:.2}",
                    elapsed.as_nanos(),
                    elapsed.as_secs_f64() * 1_000_000_000.0 / ROWS_F64,
                );
            }
        }
    }
}

/// Builds the current typed `slug.core` clutch into a repository in
/// `directory`, as clutch staging does.
fn build_typed_core_clutch(directory: &TemporaryDirectory) {
    let source = workspace_root().join("clutch/slug.core.clutch");
    let repository = directory.path().join("clutch");
    let root = repository.join("slug.core.clutch");
    for file in [
        "clutch.toml",
        "modules/std.slug",
        "modules/stdin.slug",
        "modules/math.slug",
        "native/source/core.c",
        "native/source/slug_core.h",
    ] {
        let target = root.join(file);
        fs::create_dir_all(target.parent().expect("clutch file has a parent"))
            .expect("create typed clutch directory");
        fs::copy(source.join(file), target).expect("copy typed core clutch file");
    }
    fs::write(
        repository.join("manifest.toml"),
        "[modules]\n\
         \"slug.std\" = \"slug.core.clutch\"\n\
         \"slug.io.stdin\" = \"slug.core.clutch\"\n\
         \"slug.math\" = \"slug.core.clutch\"\n",
    )
    .expect("write typed repository manifest");
    let manifest = slug_native_manifest::NativeManifest::from_clutch_source(
        &fs::read_to_string(root.join("clutch.toml")).expect("read typed manifest"),
    )
    .expect("valid typed core manifest");
    slug_native_manifest::build_dynamic(&root, &manifest, "slug_core", current_native_platform())
        .unwrap_or_else(|error| panic!("build typed core clutch: {error}"));
}

/// Raw end-to-end timing for standard input, typed versus prototype: the CLI
/// counts piped lines through `readLines()`. An empty-input run of the same
/// program is the startup control, subtracted from the per-line cost.
#[test]
#[ignore = "run with make bench-ffi-prototype-scalar; records local timing evidence"]
fn stdin_typed_versus_prototype_end_to_end() {
    use std::io::Write as _;

    const LINES: usize = 100_000;
    #[allow(clippy::cast_precision_loss)]
    const LINES_F64: f64 = LINES as f64;
    const PROGRAM: &str = "val { readLines } = import(\"slug.io.stdin\")\n\
                           val count = fn(lines, total) {\n\
                             match select { recv lines } {\n\
                               nil => total\n\
                               _ => recur(lines, total + 1)\n\
                             }\n\
                           }\n\
                           println(count(readLines(), 0))\n";
    println!(
        "stdin configuration: profile=release samples={SCALAR_SAMPLE_COUNT} lines={LINES} target={}-{}",
        env::consts::ARCH,
        env::consts::OS,
    );
    let input = "a line of benchmark text\n".repeat(LINES);
    let run = |directory: &TemporaryDirectory, input: &str| {
        let started = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_slug"))
            .env("SLUG_HOME", directory.path())
            .arg(directory.path().join("cli.slug"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("run stdin benchmark");
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .expect("write benchmark input");
        let output = child.wait_with_output().expect("collect benchmark output");
        let elapsed = started.elapsed();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            elapsed,
        )
    };
    for label in ["prototype", "typed"] {
        let directory = TemporaryDirectory::new();
        if label == "typed" {
            build_typed_core_clutch(&directory);
        } else {
            build_core_clutch(&directory);
        }
        let library = directory.path().join("lib/slug");
        fs::create_dir_all(&library).expect("create SLUG_HOME library");
        fs::copy(
            workspace_root().join("lib/slug/builtin.slug"),
            library.join("builtin.slug"),
        )
        .expect("copy builtin module");
        fs::write(directory.path().join("cli.slug"), PROGRAM).expect("write benchmark program");
        assert_eq!(run(&directory, &input).0, LINES.to_string());
        let mut controls = Vec::new();
        let mut totals = Vec::new();
        for sample_index in 1..=SCALAR_SAMPLE_COUNT {
            let (count, control) = run(&directory, "");
            assert_eq!(count, "0");
            let (count, total) = run(&directory, &input);
            assert_eq!(count, LINES.to_string());
            println!(
                "stdin-{label} sample {sample_index}: control_ns={} total_ns={} ns_per_line={:.2}",
                control.as_nanos(),
                total.as_nanos(),
                total.saturating_sub(control).as_secs_f64() * 1_000_000_000.0 / LINES_F64,
            );
            controls.push(control);
            totals.push(total);
        }
        controls.sort();
        totals.sort();
        let (control, total) = (controls[controls.len() / 2], totals[totals.len() / 2]);
        println!(
            "stdin-{label} median: control_ns={} total_ns={} ns_per_line={:.2}",
            control.as_nanos(),
            total.as_nanos(),
            total.saturating_sub(control).as_secs_f64() * 1_000_000_000.0 / LINES_F64,
        );
    }
}
