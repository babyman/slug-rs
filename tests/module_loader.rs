use std::fs;

use slug_vm::{
    ModuleLoadError, ModuleLoader, NativeArity, NativeCall, NativeModule, NativeOwnedValue,
    NativeStatus, RuntimeErrorKind, Value, Vm, compile,
};

fn returns_nil(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::nil())
}

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
fn source_imports_use_the_configured_library_fallback() {
    let root = root("library-fallback");
    let source = root.join("source");
    let library = root.join("library");
    fs::create_dir_all(&source).expect("create source directory");
    fs::create_dir_all(library.join("slug")).expect("create library module directory");
    fs::write(library.join("slug/std.slug"), "export val answer = 42\n")
        .expect("write library module");
    let main_path = source.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val std = import(\"slug.std\")\nexport val answer = std.answer\n",
    )
    .expect("compile library importer");
    let loader = ModuleLoader::new(&source, Some(library));
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("import library fallback module");

    assert_eq!(loader.cached_module_count(), 1);
    assert_eq!(loader.initialized_module_count(), 1);
    assert_eq!(vm.exported_values(&program).to_string(), "{\"answer\": 42}");
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
        Some(Value::Map(std::rc::Rc::new(vec![
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
fn imported_module_failures_retain_the_imported_source_location() {
    let root = root("failure-location");
    fs::create_dir_all(&root).expect("create module directory");
    let broken = root.join("broken.slug");
    fs::write(&broken, "???\n").expect("write broken module");
    let main_path = root.join("main.slug");
    let program =
        compile(&main_path.to_string_lossy(), "import(\"broken\")\n").expect("compile importer");
    let loader = ModuleLoader::new(&root, None);

    let error = Vm::with_module_loader(loader)
        .run_named(&program, "main")
        .expect_err("broken module must fail");

    assert_eq!(error.kind, RuntimeErrorKind::Module);
    let expected_location = format!("{}:1:1", broken.display());
    assert!(error.message.contains(&expected_location), "{error}");
    assert!(error.message.contains("not implemented"), "{error}");
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
fn import_conflicts_keep_the_first_binding_and_report_a_warning() {
    let root = root("import-conflicts");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(root.join("first.slug"), "export val value = 1\n").expect("write first module");
    fs::write(root.join("second.slug"), "export val value = 2\n").expect("write second module");
    let main_path = root.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val imports = import(\"first\", \"second\")\nexport val value = imports.value\n",
    )
    .expect("compile importer");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main").expect("run imports");

    assert_eq!(vm.exported_values(&program).to_string(), "{\"value\": 1}");
    assert_eq!(
        loader.take_warnings(),
        ["imported binding `value` was ignored because an earlier module provided it"]
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn local_bindings_shadow_all_imports_with_a_warning() {
    let root = root("import-shadowing");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(root.join("values.slug"), "export val value = 1\n").expect("write imported module");
    let main_path = root.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val {*} = import(\"values\")\nval value = 2\nexport val result = value\n",
    )
    .expect("compile importer");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main").expect("run importer");

    assert_eq!(vm.exported_values(&program).to_string(), "{\"result\": 2}");
    assert_eq!(
        loader.take_warnings(),
        ["local binding `value` shadows an imported binding"]
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn imports_distinct_callable_signatures_as_an_overload_set() {
    let root = root("import-overloads");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("zero.slug"),
        "export val select = fn():num { 1 }\n",
    )
    .expect("write zero-argument module");
    fs::write(
        root.join("increment.slug"),
        "export val select = fn(value:num):num { value + 1 }\n",
    )
    .expect("write one-argument module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val values = import(\"zero\", \"increment\")\n\
         export val result = values.select() + values.select(4)\n",
            false,
        )
        .expect("compile overloaded import");
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("run overloaded imports");

    assert_eq!(vm.exported_values(&program).to_string(), "{\"result\": 6}");
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn imported_callable_snapshots_preserve_access_paths_generics_and_cache_identity() {
    let root = root("imported-callable-snapshots");
    fs::create_dir_all(&root).expect("create module directory");
    let typed_path = root.join("typed.slug");
    fs::write(
        &typed_path,
        "export val render = fn(value:str):str { value }\n\
         export val identity = fn<T>(value:T):T { value }\n",
    )
    .expect("write typed module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);

    for source in [
        "val api = import(\"typed\")\napi.render(1)\n",
        "val { render } = import(\"typed\")\nrender(1)\n",
        "val {*} = import(\"typed\")\nrender(1)\n",
    ] {
        let error = loader
            .compile_source(&main_path.to_string_lossy(), source, false)
            .expect_err("imported signature rejects number argument");
        assert!(error.to_string().starts_with("expected str, got num"));
    }

    let error = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val { identity } = import(\"typed\")\nidentity(nil)\n",
            false,
        )
        .expect_err("imported generic rejects nil inference");
    assert!(
        error
            .to_string()
            .starts_with("generic type argument cannot include nil")
    );

    fs::write(
        &typed_path,
        "export val render = fn(value:num):num { value }\n",
    )
    .expect("replace typed module after snapshot");
    let error = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"typed\")\napi.render(1)\n",
            false,
        )
        .expect_err("cached snapshot remains immutable");
    assert!(error.to_string().starts_with("expected str, got num"));

    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn selected_signatures_dispatch_same_shape_typed_overloads() {
    let root = root("typed-overload-selection");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("strings.slug"),
        "export val render = fn(value:str):str { value }\n",
    )
    .expect("write string overload");
    fs::write(
        root.join("numbers.slug"),
        "export val render = fn(value:num):num { value }\n",
    )
    .expect("write number overload");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"strings\", \"numbers\")\n\
             val { render: destructured } = import(\"strings\", \"numbers\")\n\
             val {*} = import(\"strings\", \"numbers\")\n\
             export val text = api.render(\"ready\")\n\
             export val number = api.render(41)\n\
             export val piped = 42 /> api.render\n\
             export val destructuredResult = destructured(43)\n\
             export val selectedResult = render(44)\n",
            false,
        )
        .expect("compile typed same-shape overloads");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("run statically selected overloads");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"text\": \"ready\", \"number\": 41, \"piped\": 42, \"destructuredResult\": 43, \"selectedResult\": 44}"
    );
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn selected_signatures_guard_live_overload_bindings() {
    let root = root("live-overload-selection");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("mutable.slug"),
        "export var render = fn(value:str):str { value }\n\
         export val replace = fn() { render = fn(value:num):num { value } }\n",
    )
    .expect("write mutable overload");
    fs::write(
        root.join("numbers.slug"),
        "export val render = fn(value:num):num { value }\n",
    )
    .expect("write number overload");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"mutable\", \"numbers\")\n\
             api.replace()\n\
             api.render(\"stale\")\n",
            false,
        )
        .expect("compile live-binding overload call");
    let error = Vm::with_module_loader(loader)
        .run_named(&program, "main")
        .expect_err("changed live binding rejects stale selection");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert!(
        error
            .message
            .contains("selected callable signature is no longer present in the live binding")
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn duplicate_callable_signatures_keep_the_first_import_with_a_warning() {
    let root = root("duplicate-callable-imports");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("first.slug"),
        "export val select = fn(value) { value + 1 }\n",
    )
    .expect("write first callable module");
    fs::write(
        root.join("second.slug"),
        "export val select = fn(value) { value + 2 }\n",
    )
    .expect("write second callable module");
    let main_path = root.join("main.slug");
    let program = compile(
        &main_path.to_string_lossy(),
        "val values = import(\"first\", \"second\")\n\
         export val result = values.select(4)\n",
    )
    .expect("compile duplicate callable import");
    let loader = ModuleLoader::new(&root, None);
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("run duplicate callable imports");

    assert_eq!(vm.exported_values(&program).to_string(), "{\"result\": 5}");
    assert_eq!(
        loader.take_warnings(),
        [
            "imported callable `select` with a duplicate signature was ignored because an earlier module provided it"
        ]
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn retains_top_level_declaration_documentation_and_evaluated_tags() {
    let root = root("module-metadata");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("metadata.slug"),
        "/**\n * A public value.\n */\n@label(\"stable\", 2)\nexport val value = 1\n\n/**\n * A host callable.\n */\nexport foreign chan = fn(capacity:num = 0):chan<any|nil>\n",
    )
    .expect("write metadata module");
    let loader = ModuleLoader::new(&root, None);
    let module = NativeModule::new("metadata", ()).expect("native module is valid");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.define_foreign(
        module
            .function(
                "chan",
                NativeArity::Range {
                    minimum: 0,
                    maximum: 1,
                },
                returns_nil,
            )
            .expect("native function is valid"),
    )
    .expect("foreign binding is unique");

    let instance = loader
        .initialize(None, "metadata")
        .expect("initialize metadata module");

    assert_eq!(instance.metadata.len(), 2);
    let declaration = &instance.metadata[0];
    assert_eq!(declaration.bindings, ["value"]);
    assert!(declaration.exported);
    assert!(!declaration.mutable);
    assert_eq!(
        declaration.documentation.as_deref(),
        Some("\n * A public value.\n ")
    );
    assert_eq!(declaration.tags.len(), 1);
    assert_eq!(declaration.tags[0].name, "label");
    assert_eq!(
        declaration.tags[0].arguments,
        [Value::string("stable"), Value::Int(2)]
    );
    let foreign = &instance.metadata[1];
    assert_eq!(foreign.bindings, ["chan"]);
    assert!(foreign.exported);
    assert!(!foreign.mutable);
    assert_eq!(
        foreign.documentation.as_deref(),
        Some("\n * A host callable.\n ")
    );
    assert!(foreign.tags.is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn rejects_foreign_bindings_that_cannot_accept_the_declared_arity() {
    let root = root("foreign-arity");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("arity.slug"),
        "export foreign call = fn(capacity:num = 0, foo):chan<any|nil>\n",
    )
    .expect("write foreign module");
    let loader = ModuleLoader::new(&root, None);
    let module = NativeModule::new("arity", ()).expect("native module is valid");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.define_foreign(
        module
            .function(
                "call",
                NativeArity::Range {
                    minimum: 0,
                    maximum: 1,
                },
                returns_nil,
            )
            .expect("native function is valid"),
    )
    .expect("foreign binding is unique");

    let error = loader
        .initialize(None, "arity")
        .expect_err("incompatible foreign arity must fail");
    assert!(
        error
            .to_string()
            .contains("foreign function `arity.call` does not accept its declared arity")
    );
    fs::remove_dir_all(root).expect("remove module directory");
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
