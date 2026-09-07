use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_vm::{ClutchRepository, FfiPrototypeModule, ModuleLoader, RuntimeErrorKind, Vm};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

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
    fs::create_dir_all(directory.path().join("clutch")).expect("create clutch repository");
    fs::write(
        directory.path().join("clutch/manifest.toml"),
        format!("[modules]\n\"{module_name}\" = \"{clutch_name}\"\n"),
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
    fs::copy(module_source, clutch_root.join("modules").join(module_file))
        .expect("copy clutch declaration");
    fs::copy(
        native_source,
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
             \"{module_name}\" = {{ source = \"modules/{}\", native = true }}\n\n\
             [native]\n\
             source = \"native/source\"\n\
             abi = \"slug-ffi-prototype/0.7\"\n\n\
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
    ClutchRepository::from_manifest(directory.path().join("clutch")).expect("load clutch manifest")
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

#[test]
fn loads_the_filesystem_clutch_through_a_test_built_dynamic_module() {
    let directory = TemporaryDirectory::new();
    let repository = build_native_clutch(
        &directory,
        "slug.io.fs",
        "slug.io.fs.clutch",
        "clutch/slug.io.fs.clutch/modules/fs.slug",
        "clutch/slug.io.fs.clutch/native/source/fs.c",
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
            cli_file.display()
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
                file.display(),
                file.display()
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
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
    let status = command.status().expect("start C compiler");
    assert!(status.success(), "compile C fixture");
    output
}

#[cfg(unix)]
#[test]
fn loads_sqlite_through_an_exploded_native_clutch() {
    let directory = TemporaryDirectory::new();
    let main = directory.path().join("main.slug");
    let repository = build_native_clutch(
        &directory,
        "slug.sqlite",
        "slug.sqlite.clutch",
        "clutch/slug.sqlite.clutch/modules/sqlite.slug",
        "clutch/slug.sqlite.clutch/native/source/sqlite.c",
        "slug_sqlite",
        &["-lsqlite3"],
    );
    let output = run_clutch_cli(
        &directory,
        "val sqlite = import(\"slug.sqlite\")\n\
         val database = sqlite.openMemory()\n\
         defer sqlite.close(database)\n\
         sqlite.exec(database, \"create table answers(value integer)\")\n\
         sqlite.exec(database, \"insert into answers values (42)\")\n\
         println(sqlite.queryInt(database, \"select value from answers\"))\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
    let loader = ModuleLoader::with_clutch_repository(directory.path(), None, repository);
    let mut vm = Vm::with_module_loader(loader.clone());

    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.sqlite\")\n\
             val database = sqlite.openMemory()\n\
             sqlite.exec(database, \"create table answers(value integer)\")\n\
             sqlite.exec(database, \"insert into answers values (42)\")\n\
             sqlite.queryInt(database, \"select value from answers\") + sqlite.close(database)\n",
        )
        .expect("compile sqlite resource program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "42");

    let statement = loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.sqlite\")\n\
             val database = sqlite.openMemory()\n\
             val query = sqlite.prepare(database, \"select ?1 + ?2\")\n\
             sqlite.bindInt(query, 1, 20)\n\
             sqlite.bindInt(query, 2, 22)\n\
             sqlite.stepInt(query) + sqlite.closeStatement(query) + sqlite.close(database)\n",
        )
        .expect("compile SQLite statement program");
    assert_eq!(vm.run_named(&statement, "main").unwrap().to_string(), "42");

    let close_with_statement = loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.sqlite\")\n\
             val attempt = fn() {\n\
               val database = sqlite.openMemory()\n\
               val query = sqlite.prepare(database, \"select 1\")\n\
               sqlite.close(database)\n\
             }\n\
             attempt()\n",
        )
        .expect("compile parent-close rejection program");
    let error = vm
        .run_named(&close_with_statement, "main")
        .expect_err("database close must reject active statements");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("sqlite.error")
    );

    let invalid = loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.sqlite\")\n\
             val database = sqlite.openMemory()\n\
             sqlite.exec(database, \"definitely not SQL\")\n",
        )
        .expect("compile failing sqlite program");
    let error = vm
        .run_named(&invalid, "main")
        .expect_err("SQLite errors must remain checked");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("sqlite.error")
    );
}

#[test]
fn loads_math_through_an_exploded_native_clutch() {
    let directory = TemporaryDirectory::new();
    let main = directory.path().join("main.slug");
    let repository = build_native_clutch(
        &directory,
        "slug.math",
        "slug.math.clutch",
        "clutch/slug.math.clutch/modules/math.slug",
        "clutch/slug.math.clutch/native/source/math.c",
        "slug_math",
        &[],
    );
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
    let library = compile_fixture(&directory, "tests/ffi/invalid_module.c", "invalid");
    let Err(error) = FfiPrototypeModule::load(library) else {
        panic!("incompatible ABI must fail");
    };
    assert!(error.to_string().contains("ABI major 99"));
}

#[test]
fn rejects_an_undersized_c_function_descriptor() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "tests/ffi/undersized_function_module.c",
        "undersized",
    );
    let Err(error) = FfiPrototypeModule::load(library) else {
        panic!("undersized descriptor must fail");
    };
    assert!(error.to_string().contains("undersized function descriptor"));
}

#[test]
fn rejects_a_c_resource_type_without_a_destroy_callback() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "tests/ffi/missing_resource_destroy_module.c",
        "missing_resource_destroy",
    );
    let Err(error) = FfiPrototypeModule::load(library) else {
        panic!("resource descriptor without a destructor must fail");
    };
    assert!(error.to_string().contains("has no destroy callback"));
}

#[test]
fn rejects_source_resource_types_missing_from_the_native_module() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(&directory, "tests/ffi/resource_module.c", "resources");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Missing\n\
         export foreign create = fn(value:num):Missing\n",
    )
    .expect("write mismatched resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C resource module");
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
    let library = compile_fixture(&directory, "tests/ffi/resource_module.c", "resources");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export foreign destroyed = fn():num\n",
    )
    .expect("write mismatched resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C resource module");
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
        "tests/ffi/unknown_status_module.c",
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
    let module = FfiPrototypeModule::load(library).expect("load status module");
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
    let library = compile_fixture(&directory, "tests/ffi/same_arity_module.c", "same_arity");
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
    let module = FfiPrototypeModule::load(library).expect("load same-arity module");
    let mut vm = Vm::with_module_loader(loader);
    module
        .register(&mut vm)
        .expect("register same-arity module");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "3");
}

#[test]
fn unloads_libraries_after_destroying_each_module_state() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(&directory, "tests/ffi/stateful_module.c", "stateful");
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
        let module = FfiPrototypeModule::load(&library).expect("load stateful module");
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
        "tests/ffi/stateful_module.c",
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
    let module = FfiPrototypeModule::load(library).expect("load stateful module");
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
    let library = compile_fixture(&directory, "tests/ffi/resource_module.c", "resources");
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
    let module = FfiPrototypeModule::load(&library).expect("load C resource module");
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
    let library = compile_fixture(&directory, "tests/ffi/resource_module.c", "resources");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export resource Counter\n\
         export foreign read = fn(handle:Counter):num\n",
    )
    .expect("write resource module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C resource module");
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
            .contains("non-resource argument where `Counter` is declared")
    );
}

#[test]
fn rejects_foreign_resource_results_with_the_wrong_declared_type() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "tests/ffi/wrong_resource_result_module.c",
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
    let module = FfiPrototypeModule::load(library).expect("load C resource module");
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
            .contains("wrong resource type for its result; expected `Other`")
    );
}

#[test]
fn cleans_up_c_resources_during_error_unwinding_and_vm_teardown() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(
        &directory,
        "tests/ffi/resource_module.c",
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
    let module = FfiPrototypeModule::load(&library).expect("load C resource module");
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
    let replacement_module = FfiPrototypeModule::load(&library).expect("reload C resource module");
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
    let library = compile_fixture(&directory, "tests/ffi/async_module.c", "async");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/async.slug"),
        "export foreign delayed = fn():chan<num>\n",
    )
    .expect("write async module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C async module");
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
        "tests/ffi/backpressure_module.c",
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
    let module = FfiPrototypeModule::load(library).expect("load C backpressure module");
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
    let library = compile_fixture(&directory, "tests/ffi/revocation_module.c", "revocation");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/revocation.slug"),
        "export foreign delayed = fn():chan<num>;\n\
         export foreign waitStatus = fn():num\n",
    )
    .expect("write revocation module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C revocation module");
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
        "tests/ffi/text_backpressure_module.c",
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
    let module = FfiPrototypeModule::load(library).expect("load C text backpressure module");
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
