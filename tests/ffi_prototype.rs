#![cfg(feature = "ffi-prototype")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_vm::{FfiPrototypeModule, ModuleLoader, RuntimeErrorKind, Vm};

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

fn compile_fixture_with_libraries(
    directory: &TemporaryDirectory,
    source: &str,
    name: &str,
    libraries: &[&str],
) -> PathBuf {
    let output = directory.path().join(format!("lib{name}.dylib"));
    let mut command = Command::new("cc");
    command
        .args(["-dynamiclib", "-I", "include", source, "-o"])
        .arg(output.to_str().expect("temporary library path is UTF-8"))
        .arg("-lm")
        .args(libraries)
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    let status = command.status().expect("start C compiler");
    assert!(status.success(), "compile C fixture");
    output
}

#[test]
fn wraps_an_in_memory_sqlite_database_as_a_c_resource() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture_with_libraries(
        &directory,
        "tests/ffi/sqlite_module.c",
        "sqlite",
        &["-lsqlite3"],
    );
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/sqlite.slug"),
        "export foreign openMemory = fn():resource\n\
         export foreign exec = fn(database:resource, sql:str):num\n\
         export foreign queryInt = fn(database:resource, sql:str):num\n\
         export foreign close = fn(database:resource):num\n\
         export foreign prepare = fn(database:resource, sql:str):resource\n\
         export foreign bindInt = fn(statement:resource, index:num, value:num):num\n\
         export foreign stepInt = fn(statement:resource):num\n\
         export foreign closeStatement = fn(statement:resource):num\n",
    )
    .expect("write sqlite module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let module = FfiPrototypeModule::load(library).expect("load C sqlite module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module.register(&mut vm).expect("register C sqlite module");

    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val sqlite = import(\"slug.sqlite\")\n\
             val database = sqlite.openMemory()\n\
             sqlite.exec(database, \"create table answers(value integer)\")\n\
             sqlite.exec(database, \"insert into answers values (42)\")\n\
             sqlite.queryInt(database, \"select value from answers\") + sqlite.close(database)\n",
            false,
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
            false,
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
            false,
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
            false,
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
fn loads_a_c_math_module_and_preserves_checked_native_errors() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(&directory, "tests/ffi/math_module.c", "slug_math");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/math.slug"),
        "export foreign add = fn(left:num, right:num):num\n\
         export foreign sqrt = fn(value:num):num\n",
    )
    .expect("write math module source");
    let main = directory.path().join("main.slug");
    let loader = ModuleLoader::new(directory.path(), None);
    let program = loader
        .compile_source(
            &main.to_string_lossy(),
            "val math = import(\"slug.math\")\nmath.add(20, 22) + math.sqrt(9.0)\n",
            false,
        )
        .expect("compile program using C module");
    let module = FfiPrototypeModule::load(library).expect("load C math module");
    let mut vm = Vm::with_module_loader(loader.clone());
    module.register(&mut vm).expect("register C math module");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "45");

    let failing = loader
        .compile_source(
            &main.to_string_lossy(),
            "val math = import(\"slug.math\")\nmath.sqrt(-1.0)\n",
            false,
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
            false,
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
            false,
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
fn keeps_libraries_resident_while_destroying_each_module_state() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(&directory, "tests/ffi/stateful_module.c", "stateful");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/stateful.slug"),
        "export foreign stateInfo = fn():num\n",
    )
    .expect("write stateful module source");

    for expected in [100, 201] {
        let main = directory.path().join("main.slug");
        let loader = ModuleLoader::new(directory.path(), None);
        let program = loader
            .compile_source(
                &main.to_string_lossy(),
                "val stateful = import(\"slug.stateful\")\nstateful.stateInfo()\n",
                false,
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
fn owns_c_resources_with_checked_borrow_and_close_semantics() {
    let directory = TemporaryDirectory::new();
    let library = compile_fixture(&directory, "tests/ffi/resource_module.c", "resources");
    fs::create_dir_all(directory.path().join("slug")).expect("create Slug module directory");
    fs::write(
        directory.path().join("slug/resources.slug"),
        "export foreign create = fn(value:num):resource\n\
         export foreign read = fn(handle:resource):num\n\
         export foreign close = fn(handle:resource):num\n\
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
            false,
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
            false,
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
        "export foreign create = fn(value:num):resource\n\
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
            false,
        )
        .expect("compile unwinding program");
    vm.run_named(&unwinding, "main")
        .expect_err("throw must unwind the C resource");

    let destroyed = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.destroyed()\n",
            false,
        )
        .expect("compile destruction counter program");
    assert_eq!(vm.run_named(&destroyed, "main").unwrap().to_string(), "1");

    let create = loader
        .compile_source(
            &main.to_string_lossy(),
            "val resources = import(\"slug.resources\")\nresources.create(2)\n",
            false,
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
            false,
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
            false,
        )
        .expect("compile C async producer program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "73");
}

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
            false,
        )
        .expect("compile C backpressure program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "13");
}

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
            false,
        )
        .expect("compile C producer revocation program");
    assert_eq!(vm.run_named(&program, "main").unwrap().to_string(), "2");
}

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
            false,
        )
        .expect("compile C text backpressure program");
    assert_eq!(
        vm.run_named(&program, "main").unwrap().to_string(),
        "first:second:1:2"
    );
}
