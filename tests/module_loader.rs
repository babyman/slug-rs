use std::fs;

use slug_vm::{ModuleLoadError, ModuleLoader, RuntimeErrorKind, Value, Vm, compile};

fn root(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-module-{kind}-{}", std::process::id()))
}

#[test]
fn resolves_importer_relative_source_and_library_roots() {
    let root = root("resolution");
    let source = root.join("source");
    let library = root.join("library");
    fs::create_dir_all(source.join("local")).expect("create source module directory");
    fs::create_dir_all(library.join("slug")).expect("create library module directory");
    fs::write(source.join("local/math.slug"), "export val value = 1\n")
        .expect("write local module");
    fs::write(library.join("slug/std.slug"), "val value = 2\n").expect("write library module");

    let loader = ModuleLoader::new(&source, Some(library.clone()));
    assert_eq!(
        loader
            .load(None, "local.math")
            .expect("load source module")
            .text,
        "export val value = 1\n"
    );
    assert_eq!(loader.initialized_module_count(), 0);
    loader
        .initialize(None, "local.math")
        .expect("reuse initialized module");
    assert_eq!(loader.initialized_module_count(), 1);
    assert_eq!(
        loader
            .load(None, "slug.std")
            .expect("load library module")
            .text,
        "val value = 2\n"
    );
    assert!(matches!(
        loader.load(None, "../escape"),
        Err(ModuleLoadError::InvalidName(_))
    ));
    let program = loader
        .compile(None, "local.math")
        .expect("compile source module");
    assert_eq!(program.chunk_count(), 1);
    assert_eq!(loader.cached_module_count(), 1);
    assert_eq!(
        loader
            .initialize(None, "local.math")
            .expect("initialize module")
            .exports
            .to_string(),
        "{\"value\": 1}"
    );
    loader
        .compile(None, "local.math")
        .expect("reuse cached module");
    assert_eq!(loader.cached_module_count(), 1);
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn source_imports_return_cached_export_maps_in_module_order() {
    let root = root("source-import");
    fs::create_dir_all(root.join("local")).expect("create module directory");
    fs::write(
        root.join("local/inner.slug"),
        "export val answer = 42\nexport val shared = \"inner\"\n",
    )
    .expect("write inner module");
    fs::write(
        root.join("local/outer.slug"),
        "val inner = import(\"inner\")\nexport val answer = inner.answer\nexport val shared = \"outer\"\n",
    )
    .expect("write outer module");
    fs::write(
        root.join("fallback.slug"),
        "export val shared = \"fallback\"\nexport val extra = 7\n",
    )
    .expect("write fallback module");
    let main_path = root.join("main.slug");
    let source =
        "export val modules = import(\"local.outer\", \"fallback\")\nimport(\"local.outer\")\n";
    let program = compile(&main_path.to_string_lossy(), source).expect("compile importer");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main").expect("execute imports");

    assert_eq!(loader.cached_module_count(), 3);
    assert_eq!(loader.initialized_module_count(), 3);
    assert_eq!(
        vm.global("modules"),
        Some(&Value::Map(std::rc::Rc::new(vec![
            (Value::string("answer"), Value::Int(42)),
            (Value::string("shared"), Value::string("outer")),
            (Value::string("extra"), Value::Int(7)),
        ])))
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn source_imports_check_module_name_values_and_loader_failures() {
    let root = root("source-import-errors");
    fs::create_dir_all(&root).expect("create module directory");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);

    let program = compile(&main_path.to_string_lossy(), "import(42)\n").expect("compile import");
    let error = Vm::with_module_loader(loader.clone())
        .run_named(&program, "main")
        .expect_err("non-string imports must fail");
    assert_eq!(error.kind, RuntimeErrorKind::Type);
    assert_eq!(error.message, "import expects string module names, got num");

    let program = compile(&main_path.to_string_lossy(), "import(\"missing\")\n")
        .expect("compile missing import");
    let error = Vm::with_module_loader(loader)
        .run_named(&program, "main")
        .expect_err("missing imports must fail");
    assert_eq!(error.kind, RuntimeErrorKind::Module);
    assert_eq!(error.message, "module `missing` was not found");
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn cyclic_imports_resolve_predeclared_function_bindings() {
    let root = root("cyclic-functions");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("a.slug"),
        "val b = import(\"b\")\nexport val a = fn() { b.b() }\n",
    )
    .expect("write first cyclic module");
    fs::write(
        root.join("b.slug"),
        "val a = import(\"a\")\nexport val b = fn() { 7 }\n",
    )
    .expect("write second cyclic module");
    let main_path = root.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val a = import(\"a\")\nexport val output = a.a()\n",
    )
    .expect("compile cycle importer");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("execute cyclic imports");

    assert_eq!(loader.initialized_module_count(), 2);
    assert_eq!(vm.exported_values(&program).to_string(), "{\"output\": 7}");
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn imported_functions_run_in_their_defining_module_and_observe_live_exports() {
    let root = root("live-function-imports");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("counter.slug"),
        "export var count = 1\n\
         export val next = fn() { count = count + 1; count }\n",
    )
    .expect("write counter module");
    let main_path = root.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val counter = import(\"counter\")\n\
         export val total = counter.next() + counter.next() + counter.count\n",
    )
    .expect("compile importer");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader);

    vm.run_named(&program, "main")
        .expect("run imported functions");

    assert_eq!(vm.exported_values(&program).to_string(), "{\"total\": 8}");
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn cyclic_imports_reject_reads_before_the_defining_binding_initializes() {
    let root = root("cyclic-uninitialized");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("a.slug"),
        "val b = import(\"b\")\nexport val from_a = b.from_b\n",
    )
    .expect("write first cyclic module");
    fs::write(
        root.join("b.slug"),
        "val a = import(\"a\")\nexport val from_b = a.from_a\n",
    )
    .expect("write second cyclic module");
    let loader = ModuleLoader::new(&root, None);

    let error = loader
        .initialize(None, "a")
        .expect_err("use before initialization must fail");

    assert!(
        error
            .to_string()
            .contains("binding `from_a` is not initialized")
    );
    assert_eq!(loader.initialized_module_count(), 0);
    fs::remove_dir_all(root).expect("remove module test directory");
}
