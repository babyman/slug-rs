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
    let output = directory.path().join(format!("lib{name}.dylib"));
    let status = Command::new("cc")
        .args([
            "-dynamiclib",
            "-I",
            "include",
            source,
            "-o",
            output.to_str().expect("temporary library path is UTF-8"),
            "-lm",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("start C compiler");
    assert!(status.success(), "compile C fixture");
    output
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
    let module = FfiPrototypeModule::load(library).expect("load C resource module");
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
