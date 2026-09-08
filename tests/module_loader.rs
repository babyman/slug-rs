use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

use slug_vm::{
    ClutchPluginRegistrar, ClutchRepository, ClutchRepositoryError, ModuleLoadError, ModuleLoader,
    NativeArity, NativeCall, NativeDescriptorError, NativeModule, NativeOwnedValue, NativeStatus,
    RuntimeErrorKind, Value, Vm, compile,
};

fn returns_nil(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::nil())
}

fn returns_native(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::string("native"))
}

fn returns_seven(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::integer(7))
}

static ANSWER_PLUGIN_CLEANUPS: AtomicUsize = AtomicUsize::new(0);
static INVALID_PLUGIN_CLEANUPS: AtomicUsize = AtomicUsize::new(0);
static RETRY_PLUGIN_CLEANUPS: AtomicUsize = AtomicUsize::new(0);
static RESOURCE_PLUGIN_CLEANUPS: AtomicUsize = AtomicUsize::new(0);

fn record_answer_plugin_cleanup() {
    ANSWER_PLUGIN_CLEANUPS.fetch_add(1, Ordering::SeqCst);
}

fn record_invalid_plugin_cleanup() {
    INVALID_PLUGIN_CLEANUPS.fetch_add(1, Ordering::SeqCst);
}

fn record_retry_plugin_cleanup() {
    RETRY_PLUGIN_CLEANUPS.fetch_add(1, Ordering::SeqCst);
}

fn record_resource_plugin_cleanup() {
    RESOURCE_PLUGIN_CLEANUPS.fetch_add(1, Ordering::SeqCst);
}

fn answer_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    let module = NativeModule::new(registrar.module_name(), ())?;
    registrar.define_foreign(module.function("answer", NativeArity::Exact(0), returns_seven)?)?;
    registrar.set_cleanup(record_answer_plugin_cleanup)
}

fn missing_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    registrar.set_cleanup(record_invalid_plugin_cleanup)
}

fn failing_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    registrar.set_cleanup(record_invalid_plugin_cleanup)?;
    NativeModule::new("", ()).map(|_| ())
}

fn wrong_module_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    let module = NativeModule::new("example.unrelated", ())?;
    registrar.define_foreign(module.function("answer", NativeArity::Exact(0), returns_seven)?)
}

static RETRY_PLUGIN_CALLS: AtomicUsize = AtomicUsize::new(0);

fn retry_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    let module = NativeModule::new(registrar.module_name(), ())?;
    let arity = if RETRY_PLUGIN_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
        NativeArity::Exact(1)
    } else {
        NativeArity::Exact(0)
    };
    registrar.define_foreign(module.function("answer", arity, returns_seven)?)?;
    registrar.set_cleanup(record_retry_plugin_cleanup)
}

fn close_unit(_: &mut ()) {}

fn destroy_unit(_: ()) {}

fn resource_plugin(registrar: &mut ClutchPluginRegistrar) -> Result<(), NativeDescriptorError> {
    let module = NativeModule::new(registrar.module_name(), ())?;
    let _file = module.resource_type("File", close_unit, destroy_unit)?;
    registrar.define_foreign(module.function("open", NativeArity::Exact(0), returns_nil)?)?;
    registrar.set_cleanup(record_resource_plugin_cleanup)
}

fn describes_enum_case(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call
        .argument(0)
        .and_then(slug_vm::NativeValueRef::as_enum_case)
    {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    call.return_value(NativeOwnedValue::string(format!(
        "{}.{}",
        value.enum_name(),
        value.case_name()
    )))
}

#[test]
fn foreign_batch_registration_does_not_partially_install_descriptors() {
    let loader = ModuleLoader::new(".", None);
    let mut vm = Vm::with_module_loader(loader);
    let module = NativeModule::new("atomic", ()).expect("native module is valid");
    let first = module
        .function("call", NativeArity::Exact(0), returns_nil)
        .expect("first native function is valid");
    let duplicate = module
        .function("call", NativeArity::Exact(1), returns_nil)
        .expect("second native function is valid");

    assert!(
        vm.define_foreign_batch(vec![first.clone(), duplicate])
            .is_err()
    );
    vm.define_foreign(first)
        .expect("failed batch must not register its first descriptor");
}

fn root(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-module-{kind}-{}", std::process::id()))
}

fn write_source_clutch(
    root: &std::path::Path,
    modules: &[(&str, &str, &str)],
) -> std::path::PathBuf {
    let clutch = root.join("example.clutch");
    fs::create_dir_all(clutch.join("modules")).expect("create clutch module directory");
    let entries = modules
        .iter()
        .map(|(name, path, source)| {
            fs::write(clutch.join("modules").join(path), source)
                .expect("write clutch source module");
            format!("\"{name}\" = {{ source = \"modules/{path}\" }}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        clutch.join("clutch.toml"),
        format!(
            "[modules]\n\
             {entries}\n"
        ),
    )
    .expect("write clutch manifest");
    clutch
}

fn write_plugin_clutch(
    root: &std::path::Path,
    module_name: &str,
    source: &str,
    plugin_entry: &str,
) -> std::path::PathBuf {
    let clutch = root.join("plugin.clutch");
    fs::create_dir_all(clutch.join("modules")).expect("create plugin clutch module directory");
    fs::write(clutch.join("modules/module.slug"), source).expect("write plugin clutch module");
    fs::write(
        clutch.join("clutch.toml"),
        format!(
            "[modules]\n\
             \"{module_name}\" = {{ source = \"modules/module.slug\", plugin = \"{plugin_entry}\" }}\n"
        ),
    )
    .expect("write plugin clutch manifest");
    clutch
}

#[test]
fn imports_source_modules_from_an_explicit_clutch_repository() {
    let root = root("clutch-source");
    fs::create_dir_all(&root).expect("create source root");
    let clutch = write_source_clutch(
        &root,
        &[(
            "example.library",
            "library.slug",
            "export val answer = 42\n",
        )],
    );
    let repository = ClutchRepository::new(vec![("example.library".into(), clutch)])
        .expect("create clutch repository");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
    let main_path = root.join("main.slug");
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val library = import(\"example.library\")\nexport val answer = library.answer\n",
        )
        .expect("compile clutch importer");
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main").expect("run clutch importer");

    assert_eq!(loader.cached_module_count(), 1);
    assert_eq!(loader.initialized_module_count(), 1);
    assert_eq!(vm.exported_values(&program).to_string(), "{\"answer\": 42}");
    fs::remove_dir_all(root).expect("remove clutch source root");
}

#[test]
fn repository_manifest_selects_importable_clutches() {
    let root = root("clutch-repository-manifest");
    let repository_root = root.join("clutch");
    fs::create_dir_all(&repository_root).expect("create clutch repository root");
    let clutch = write_source_clutch(
        &repository_root,
        &[(
            "example.indexed",
            "indexed.slug",
            "export val answer = 42\n",
        )],
    );
    let hidden = repository_root.join("hidden.clutch");
    fs::create_dir_all(hidden.join("modules")).expect("create hidden clutch directory");
    fs::write(
        hidden.join("clutch.toml"),
        "[modules]\n\"example.hidden\" = { source = \"modules/hidden.slug\" }\n",
    )
    .expect("write hidden clutch manifest");
    fs::write(
        hidden.join("modules/hidden.slug"),
        "export val answer = 7\n",
    )
    .expect("write hidden clutch source");
    fs::write(
        repository_root.join("manifest.toml"),
        "[modules]\n\"example.indexed\" = \"example.clutch\"\n",
    )
    .expect("write repository manifest");

    let repository =
        ClutchRepository::from_manifest(&repository_root).expect("load repository manifest");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
    let main_path = root.join("main.slug");
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val indexed = import(\"example.indexed\")\nexport val answer = indexed.answer\n",
        )
        .expect("compile indexed clutch importer");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("run indexed clutch importer");
    assert_eq!(vm.exported_values(&program).to_string(), "{\"answer\": 42}");
    assert!(matches!(
        loader.load(None, "example.hidden"),
        Err(ModuleLoadError::NotFound { .. })
    ));
    drop(vm);
    fs::remove_dir_all(root).expect("remove clutch repository root");
    drop(clutch);
}

#[test]
fn repository_manifest_rejects_escaping_and_mismatched_clutch_entries() {
    let root = root("clutch-repository-manifest-invalid");
    fs::create_dir_all(root.join("clutch")).expect("create clutch repository root");
    fs::write(
        root.join("clutch/manifest.toml"),
        "[modules]\n\"example.invalid\" = \"../outside.clutch\"\n",
    )
    .expect("write escaping repository manifest");
    assert!(matches!(
        ClutchRepository::from_manifest(root.join("clutch")),
        Err(ClutchRepositoryError::Manifest { .. })
    ));

    let clutch = write_source_clutch(
        &root.join("clutch"),
        &[("example.actual", "actual.slug", "export val answer = 42\n")],
    );
    fs::write(
        root.join("clutch/manifest.toml"),
        "[modules]\n\"example.expected\" = \"example.clutch\"\n",
    )
    .expect("write mismatched repository manifest");
    assert!(matches!(
        ClutchRepository::from_manifest(root.join("clutch")),
        Err(ClutchRepositoryError::Manifest { .. })
    ));
    drop(clutch);
    fs::remove_dir_all(root).expect("remove invalid clutch repository root");
}

#[test]
fn existing_source_providers_take_precedence_over_clutches() {
    let root = root("clutch-precedence");
    fs::create_dir_all(root.join("example")).expect("create source module directory");
    fs::write(
        root.join("example/library.slug"),
        "export val source = \"local\"\n",
    )
    .expect("write local source module");
    let clutch = write_source_clutch(
        &root,
        &[(
            "example.library",
            "library.slug",
            "export val source = \"clutch\"\n",
        )],
    );
    let repository = ClutchRepository::new(vec![("example.library".into(), clutch)])
        .expect("create clutch repository");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);

    assert_eq!(
        loader
            .load(None, "example.library")
            .expect("local module takes precedence")
            .text,
        "export val source = \"local\"\n"
    );
    fs::remove_dir_all(root).expect("remove clutch precedence root");
}

#[test]
fn clutch_resolution_rejects_invalid_manifests_without_caching_modules() {
    let root = root("clutch-invalid");
    fs::create_dir_all(&root).expect("create clutch test root");

    let loader = ModuleLoader::with_clutch_repository(
        &root,
        None,
        ClutchRepository::new(vec![(
            "example.missing".into(),
            root.join("missing.clutch"),
        )])
        .expect("create missing repository"),
    );
    assert!(matches!(
        loader.load(None, "example.missing"),
        Err(ModuleLoadError::Clutch { .. })
    ));
    assert_eq!(loader.cached_module_count(), 0);

    let malformed = root.join("malformed.clutch");
    fs::create_dir_all(&malformed).expect("create malformed clutch root");
    fs::write(malformed.join("clutch.toml"), "format = [\n").expect("write malformed manifest");
    let loader = ModuleLoader::with_clutch_repository(
        &root,
        None,
        ClutchRepository::new(vec![("example.malformed".into(), malformed)])
            .expect("create malformed repository"),
    );
    assert!(matches!(
        loader.load(None, "example.malformed"),
        Err(ModuleLoadError::Clutch { .. })
    ));
    assert_eq!(loader.cached_module_count(), 0);

    let escaping = write_source_clutch(
        &root,
        &[("example.escaping", "escape.slug", "export val value = 1\n")],
    );
    fs::write(
        escaping.join("clutch.toml"),
        "[modules]\n\
         \"example.escaping\" = { source = \"../escape.slug\" }\n",
    )
    .expect("write escaping manifest");
    let loader = ModuleLoader::with_clutch_repository(
        &root,
        None,
        ClutchRepository::new(vec![("example.escaping".into(), escaping)])
            .expect("create escaping repository"),
    );
    assert!(matches!(
        loader.load(None, "example.escaping"),
        Err(ModuleLoadError::Clutch { .. })
    ));

    let incompatible = write_source_clutch(
        &root,
        &[(
            "example.incompatible",
            "incompatible.slug",
            "export val value = 1\n",
        )],
    );
    fs::write(
        incompatible.join("clutch.toml"),
        "[modules]\n\
         \"example.incompatible\" = { source = \"modules/incompatible.slug\" }\n\
         unsupported = true\n",
    )
    .expect("write unsupported manifest field");
    let loader = ModuleLoader::with_clutch_repository(
        &root,
        None,
        ClutchRepository::new(vec![("example.incompatible".into(), incompatible)])
            .expect("create incompatible repository"),
    );
    assert!(matches!(
        loader.load(None, "example.incompatible"),
        Err(ModuleLoadError::Clutch { .. })
    ));

    fs::remove_dir_all(root).expect("remove invalid clutch root");
}

#[test]
fn clutch_repository_rejects_duplicate_module_providers() {
    let error = ClutchRepository::new(vec![
        (
            "example.library".into(),
            std::path::PathBuf::from("first.clutch"),
        ),
        (
            "example.library".into(),
            std::path::PathBuf::from("second.clutch"),
        ),
    ])
    .expect_err("duplicate providers must be rejected");
    assert_eq!(
        error,
        ClutchRepositoryError::DuplicateProvider {
            name: "example.library".into()
        }
    );
}

#[test]
fn clutch_modules_preserve_cyclic_import_initialization() {
    let root = root("clutch-cycle");
    fs::create_dir_all(&root).expect("create clutch cycle root");
    let clutch = write_source_clutch(
        &root,
        &[
            (
                "example.left",
                "left.slug",
                "val right = import(\"example.right\")\nexport val left = fn() { right.right() }\n",
            ),
            (
                "example.right",
                "right.slug",
                "val left = import(\"example.left\")\nexport val right = fn() { 7 }\n",
            ),
        ],
    );
    let repository = ClutchRepository::new(vec![
        ("example.left".into(), clutch.clone()),
        ("example.right".into(), clutch),
    ])
    .expect("create clutch repository");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
    let instance = loader
        .initialize(None, "example.left")
        .expect("initialize cyclic clutch module");
    let program = loader
        .compile_source(
            &root.join("main.slug").to_string_lossy(),
            "val left = import(\"example.left\")\nexport val value = left.left()\n",
        )
        .expect("compile cyclic clutch consumer");
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("run cyclic clutch consumer");

    assert_eq!(instance.exports.to_string(), "{\"left\": <fn>}");
    assert_eq!(loader.initialized_module_count(), 2);
    assert_eq!(vm.exported_values(&program).to_string(), "{\"value\": 7}");
    fs::remove_dir_all(root).expect("remove clutch cycle root");
}

#[test]
fn clutch_plugins_bind_only_their_declared_module_foreign_functions() {
    ANSWER_PLUGIN_CLEANUPS.store(0, Ordering::SeqCst);
    let root = root("clutch-plugin-success");
    fs::create_dir_all(&root).expect("create plugin test root");
    let clutch = write_plugin_clutch(
        &root,
        "example.plugin",
        "export foreign answer = fn():num\n",
        "test.answer",
    );
    {
        let mut repository =
            ClutchRepository::new(vec![("example.plugin".into(), clutch)]).expect("repository");
        repository
            .define_plugin("test.answer", answer_plugin)
            .expect("configure plugin");
        let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
        let program = loader
            .compile_source(
                &root.join("main.slug").to_string_lossy(),
                "val plugin = import(\"example.plugin\")\nexport val answer = plugin.answer()\n",
            )
            .expect("compile plugin consumer");
        let mut vm = Vm::with_module_loader(loader);

        vm.run_named(&program, "main").expect("run plugin consumer");

        assert_eq!(vm.exported_values(&program).to_string(), "{\"answer\": 7}");
        assert_eq!(ANSWER_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 0);
    }
    assert_eq!(ANSWER_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(root).expect("remove plugin test root");
}

#[test]
fn clutch_plugins_reject_unavailable_or_unrelated_registrations() {
    INVALID_PLUGIN_CLEANUPS.store(0, Ordering::SeqCst);
    let root = root("clutch-plugin-invalid");
    fs::create_dir_all(&root).expect("create plugin invalid root");
    let clutch = write_plugin_clutch(
        &root,
        "example.plugin",
        "export foreign answer = fn():num\n",
        "test.plugin",
    );
    let unavailable = ModuleLoader::with_clutch_repository(
        &root,
        None,
        ClutchRepository::new(vec![("example.plugin".into(), clutch.clone())]).expect("repository"),
    );
    assert!(matches!(
        unavailable.initialize(None, "example.plugin"),
        Err(ModuleLoadError::Clutch { .. })
    ));

    let mut repository =
        ClutchRepository::new(vec![("example.plugin".into(), clutch.clone())]).expect("repository");
    repository
        .define_plugin("test.plugin", failing_plugin)
        .expect("configure failing plugin");
    let failing = ModuleLoader::with_clutch_repository(&root, None, repository);
    assert!(matches!(
        failing.initialize(None, "example.plugin"),
        Err(ModuleLoadError::Clutch { .. })
    ));
    assert_eq!(INVALID_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 1);

    let mut repository =
        ClutchRepository::new(vec![("example.plugin".into(), clutch.clone())]).expect("repository");
    repository
        .define_plugin("test.plugin", missing_plugin)
        .expect("configure missing plugin");
    let missing = ModuleLoader::with_clutch_repository(&root, None, repository);
    assert!(matches!(
        missing.initialize(None, "example.plugin"),
        Err(ModuleLoadError::Source { .. })
    ));
    assert_eq!(INVALID_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 2);

    let mut repository =
        ClutchRepository::new(vec![("example.plugin".into(), clutch)]).expect("repository");
    repository
        .define_plugin("test.plugin", wrong_module_plugin)
        .expect("configure plugin");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
    assert!(matches!(
        loader.initialize(None, "example.plugin"),
        Err(ModuleLoadError::Clutch { .. })
    ));
    assert_eq!(loader.initialized_module_count(), 0);
    fs::remove_dir_all(root).expect("remove plugin invalid root");
}

#[test]
fn clutch_plugin_failures_cleanup_and_do_not_leak_foreign_registrations() {
    RETRY_PLUGIN_CLEANUPS.store(0, Ordering::SeqCst);
    RETRY_PLUGIN_CALLS.store(0, Ordering::SeqCst);
    let root = root("clutch-plugin-cleanup");
    fs::create_dir_all(&root).expect("create plugin cleanup root");
    let clutch = write_plugin_clutch(
        &root,
        "example.plugin",
        "export foreign answer = fn():num\n",
        "test.retry",
    );
    let mut repository =
        ClutchRepository::new(vec![("example.plugin".into(), clutch)]).expect("repository");
    repository
        .define_plugin("test.retry", retry_plugin)
        .expect("configure retry plugin");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);

    assert!(matches!(
        loader.initialize(None, "example.plugin"),
        Err(ModuleLoadError::Source { .. })
    ));
    assert_eq!(loader.initialized_module_count(), 0);
    assert_eq!(RETRY_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 1);
    loader
        .initialize(None, "example.plugin")
        .expect("clean retry after arity validation failure");
    assert_eq!(RETRY_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 1);
    drop(loader);
    assert_eq!(RETRY_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 2);
    fs::remove_dir_all(root).expect("remove plugin cleanup root");
}

#[test]
fn clutch_plugins_validate_declared_resource_type_ownership() {
    RESOURCE_PLUGIN_CLEANUPS.store(0, Ordering::SeqCst);
    let root = root("clutch-plugin-resource");
    fs::create_dir_all(&root).expect("create plugin resource root");
    let clutch = write_plugin_clutch(
        &root,
        "example.resource",
        "export resource File\nexport foreign open = fn():File\n",
        "test.resource",
    );
    {
        let mut repository =
            ClutchRepository::new(vec![("example.resource".into(), clutch)]).expect("repository");
        repository
            .define_plugin("test.resource", resource_plugin)
            .expect("configure resource plugin");
        let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
        loader
            .initialize(None, "example.resource")
            .expect("resource registration matches declaration");
    }
    assert_eq!(RESOURCE_PLUGIN_CLEANUPS.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(root).expect("remove plugin resource root");
}

#[test]
fn filesystem_clutch_provides_nominal_files_and_cleans_up_after_error_unwinding() {
    let root = root("clutch-filesystem");
    fs::create_dir_all(&root).expect("create filesystem clutch root");
    let repository = ClutchRepository::from_manifest(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("clutch"),
    )
    .expect("load filesystem clutch repository manifest");
    let loader = ModuleLoader::with_clutch_repository(&root, None, repository);
    let file = root.join("written.txt");
    let path = file.to_string_lossy();
    let program = loader
        .compile_source(
            &root.join("main.slug").to_string_lossy(),
            &format!(
                "val fs = import(\"slug.io.fs\")\n\
                 val output:fs.File = fs.openWrite(\"{path}\")\n\
                 defer fs.close(output)\n\
                 val written:num = fs.write(output, \"hello\")\n\
                 val input:fs.File = fs.openRead(\"{path}\")\n\
                 defer fs.close(input)\n\
                 export val line:str|nil = fs.readLine(input)\n\
                 export val count:num = written\n"
            ),
        )
        .expect("compile filesystem clutch consumer");
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("run filesystem clutch consumer");

    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"line\": \"hello\", \"count\": 5}"
    );
    drop(vm);

    let failing = loader
        .compile_source(
            &root.join("failing.slug").to_string_lossy(),
            &format!(
                "val fs = import(\"slug.io.fs\")\n\
                 val output:fs.File = fs.openAppend(\"{path}\")\n\
                 defer fs.close(output)\n\
                 fs.write(output, \"!\")\n\
                 throw \"expected\"\n"
            ),
        )
        .expect("compile error-unwinding filesystem consumer");
    let mut vm = Vm::with_module_loader(loader.clone());
    assert!(vm.run_named(&failing, "main").is_err());
    drop(vm);

    let shutdown_program = loader
        .compile_source(
            &root.join("shutdown.slug").to_string_lossy(),
            &format!(
                "val fs = import(\"slug.io.fs\")\n\
                 export val output:fs.File = fs.openAppend(\"{path}\")\n"
            ),
        )
        .expect("compile shutdown filesystem consumer");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&shutdown_program, "main")
        .expect("open file before shutdown");
    vm.shutdown();
    let error = vm
        .run_named(&shutdown_program, "main")
        .expect_err("shutdown VM rejects new execution");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert!(error.message.contains("has shut down"));

    assert_eq!(
        fs::read_to_string(&file).expect("read closed file"),
        "hello!"
    );
    fs::remove_dir_all(root).expect("remove filesystem clutch root");
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
fn imported_schema_bindings_preserve_nominal_construction_types() {
    let root = root("imported-schema-types");
    fs::create_dir_all(&root).expect("create schema module directory");
    fs::write(
        root.join("shapes.slug"),
        "export val S = struct { name:str, age:num = 0 }\n",
    )
    .expect("write schema module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val {S} = import(\"shapes\")\nval Alias = S\nexport val value: struct<S> = S {name: \"Slug\"}\nexport val alias:struct<Alias> = value\nexport val name:str = alias.name\nexport val updated:struct<S> = alias copy {age: 2}\nexport val age:num = updated.age\n",
        )
        .expect("type-check importer using a schema binding");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&program, "main")
        .expect("run importer using a schema binding");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"value\": struct {\"name\": \"Slug\", \"age\": 0}, \"alias\": struct {\"name\": \"Slug\", \"age\": 0}, \"name\": \"Slug\", \"updated\": struct {\"name\": \"Slug\", \"age\": 2}, \"age\": 2}"
    );
    fs::remove_dir_all(root).expect("remove schema module directory");
}

#[test]
fn imported_bindings_preserve_inferred_value_and_function_results() {
    let root = root("imported-inferred-types");
    fs::create_dir_all(&root).expect("create inferred module directory");
    fs::write(
        root.join("values.slug"),
        "export val name = \"Slug\"\nexport val count = fn() { 10 }\n",
    )
    .expect("write inferred export module");

    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val values = import(\"values\")\n\
             val name:str = values.name\n\
             val count:num = values.count()\n\
             export val result = [name, count]\n",
        )
        .expect("type-check importer using inferred exports");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&program, "main")
        .expect("run importer using inferred exports");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"result\": [\"Slug\", 10]}"
    );

    fs::remove_dir_all(root).expect("remove inferred module directory");
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
fn imports_enum_namespaces_and_nominal_type_metadata() {
    let root = root("enum-imports");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("options.slug"),
        "export enum SeekFrom { Start, End }\n",
    )
    .expect("write enum module");
    fs::write(
        root.join("main.slug"),
        "val options = import(\"options\")\n\
         val from:options.SeekFrom = options.SeekFrom.Start\n\
         export val result = match from {\n\
           options.SeekFrom.Start => \"start\"\n\
           _ => \"other\"\n\
         }\n",
    )
    .expect("write importer");
    let loader = ModuleLoader::new(&root, None);
    let program = loader.compile(None, "main").expect("compile enum importer");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&program, "main").expect("run enum importer");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"result\": \"start\"}"
    );
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn foreign_callbacks_can_inspect_checked_enum_cases() {
    let root = root("native-enum-cases");
    fs::create_dir_all(&root).expect("create enum module directory");
    fs::write(
        root.join("options.slug"),
        "export enum SeekFrom { Start, End }\n\
         export foreign describe = fn(value:SeekFrom):str\n",
    )
    .expect("write enum module");
    let loader = ModuleLoader::new(&root, None);
    let module = NativeModule::new("options", ()).expect("create native enum module");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.define_foreign(
        module
            .function("describe", NativeArity::Exact(1), describes_enum_case)
            .expect("register enum callback"),
    )
    .expect("install enum callback");
    let program = loader
        .compile_source(
            &root.join("main.slug").to_string_lossy(),
            "val options = import(\"options\")\n\
             export val description = options.describe(options.SeekFrom.Start)\n",
        )
        .expect("compile enum callback program");
    vm.run_named(&program, "main")
        .expect("run enum callback program");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"description\": \"SeekFrom.Start\"}"
    );
    fs::remove_dir_all(root).expect("remove enum module directory");
}

#[test]
fn nominal_types_remain_distinct_between_modules() {
    let root = root("nominal-module-identities");
    fs::create_dir_all(&root).expect("create nominal module directory");
    for module in ["a", "b"] {
        fs::write(
            root.join(format!("{module}.slug")),
            "export resource Handle\n\
             export foreign create = fn():Handle\n\
             export foreign accept = fn(handle:Handle):num\n\
             export enum Mode { Text, Binary }\n\
             export val Schema = struct { value:num }\n\
             export val acceptMode = fn(mode:Mode) { mode }\n\
             export val acceptSchema = fn(value:struct<Schema>) { value }\n",
        )
        .expect("write nominal module");
    }
    let loader = ModuleLoader::new(&root, None);
    loader
        .compile_source(
            &root.join("same.slug").to_string_lossy(),
            "val a = import(\"a\")\n\
             val { Schema: ASchema } = import(\"a\")\n\
             a.accept(a.create())\n\
             a.acceptMode(a.Mode.Text)\n\
             a.acceptSchema(ASchema { value: 1 })\n",
        )
        .expect("same module nominal identities are accepted");

    for (source, expected) in [
        (
            "val a = import(\"a\")\nval b = import(\"b\")\nb.accept(a.create())\n",
            "expected Handle, got Handle",
        ),
        (
            "val a = import(\"a\")\nval b = import(\"b\")\nb.acceptMode(a.Mode.Text)\n",
            "expected Mode, got Mode",
        ),
        (
            "val a = import(\"a\")\nval b = import(\"b\")\nval { Schema: ASchema } = import(\"a\")\nb.acceptSchema(ASchema { value: 1 })\n",
            "expected struct<Schema>, got struct<Schema>",
        ),
    ] {
        let error = loader
            .compile_source(&root.join("main.slug").to_string_lossy(), source)
            .expect_err("cross-module nominal identities must be rejected");
        assert!(error.to_string().contains(expected), "{error}");
    }
    fs::remove_dir_all(root).expect("remove nominal module directory");
}

#[test]
fn imports_transparent_type_aliases_through_module_type_paths() {
    let root = root("alias-imports");
    fs::create_dir_all(&root).expect("create alias module directory");
    fs::write(
        root.join("paths.slug"),
        "export type Path = str\nexport type Paths = list<Path>\n",
    )
    .expect("write alias module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val paths = import(\"paths\")\n\
             val values:paths.Paths = [\"Slug\"]\n\
             export val value:paths.Path = values[0]\n",
        )
        .expect("compile importer using aliases");
    let mut vm = Vm::with_module_loader(loader);
    vm.run_named(&program, "main")
        .expect("run importer using aliases");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"value\": \"Slug\"}"
    );
    fs::remove_dir_all(root).expect("remove alias module directory");
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
fn selected_foreign_and_local_overloads_dispatch_by_declared_identity() {
    let root = root("foreign-local-overloads");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("api.slug"),
        "export val render = fn(value:str):str { \"local\" }\n\
         export foreign render = fn(value:num):str\n",
    )
    .expect("write foreign overload module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"api\")\nexport val result = [api.render(1), api.render(\"x\")]\n",
        )
        .expect("compile foreign and local overloads");
    let mut vm = Vm::with_module_loader(loader.clone());
    let module = NativeModule::new("api", ()).expect("native module is valid");
    vm.define_foreign(
        module
            .function("render", NativeArity::Exact(1), returns_native)
            .expect("native function is valid"),
    )
    .expect("register foreign function");

    vm.run_named(&program, "main")
        .expect("run foreign and local overloads");

    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"result\": [\"native\", \"local\"]}"
    );
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn selected_foreign_overloads_retain_each_declaration_identity() {
    let root = root("foreign-overloads");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("api.slug"),
        "export foreign render = fn(value:num):str\n\
         export foreign render = fn(value:str):str\n",
    )
    .expect("write foreign overload module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"api\")\nexport val result = [api.render(1), api.render(\"x\")]\n",
        )
        .expect("compile foreign overloads");
    let mut vm = Vm::with_module_loader(loader.clone());
    let module = NativeModule::new("api", ()).expect("native module is valid");
    vm.define_foreign(
        module
            .function("render", NativeArity::Exact(1), returns_native)
            .expect("native function is valid"),
    )
    .expect("register foreign function");

    vm.run_named(&program, "main")
        .expect("run foreign overloads");

    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"result\": [\"native\", \"native\"]}"
    );
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn exports_local_callable_overloads_as_one_live_binding() {
    let root = root("local-export-overloads");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("api.slug"),
        "export val select = fn(value:num):str { \"number\" }\n\
         export val select = fn(value:str):str { \"text\" }\n",
    )
    .expect("write overloaded module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"api\")\nexport val result = [api.select(1), api.select(\"x\")]\n",
        )
        .expect("compile local exported overloads");
    let mut vm = Vm::with_module_loader(loader.clone());

    vm.run_named(&program, "main")
        .expect("run local exported overloads");

    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"result\": [\"number\", \"text\"]}"
    );
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
            .compile_source(&main_path.to_string_lossy(), source)
            .expect_err("imported signature rejects number argument");
        assert!(error.to_string().starts_with("expected str, got num"));
    }

    let error = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val { identity } = import(\"typed\")\nidentity(nil)\n",
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
fn concrete_overloads_take_priority_over_generic_fallbacks() {
    let root = root("concrete-generic-overloads");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("generic.slug"),
        "export val choose = fn<T>(value:T):str { \"generic\" }\n",
    )
    .expect("write generic overload");
    fs::write(
        root.join("strings.slug"),
        "export val choose = fn(value:str):str { \"specific\" }\n",
    )
    .expect("write concrete overload");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"generic\", \"strings\")\n\
             export val text = api.choose(\"value\")\n\
             export val number = api.choose(1)\n",
        )
        .expect("compile concrete and generic overloads");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main")
        .expect("run concrete and generic overloads");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"text\": \"specific\", \"number\": \"generic\"}"
    );
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn exported_overload_shapes_resolve_under_unified_compilation() {
    let root = root("overload-shape-conformance");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("api.slug"),
        "export val choose = fn<T>(value:T):str { \"generic\" }\n\
         export val choose = fn(value:str):str { \"concrete\" }\n\
         export val classify = fn(value:str):str { \"text\" }\n\
         export val classify = fn(value:str|nil):str { \"nilable\" }\n\
         export val join = fn(value:str, suffix:str = \"!\"):str { value + suffix }\n\
         export val join = fn(value:num, ...rest:num):str { \"numbers\" }\n",
    )
    .expect("write overloaded API");
    let main_path = root.join("main.slug");
    let source = "val api = import(\"api\")\n\
                  export val explicit = api.choose<str>(\"value\")\n\
                  export val concrete = api.choose(\"value\")\n\
                  export val text = api.classify(\"value\")\n\
                  export val nilable = api.classify(nil)\n\
                  export val named = api.join(value = \"named\")\n\
                  export val variadic = api.join(1, 2, 3)\n\
                  export val piped = \"pipe\" /> api.join\n";

    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(&main_path.to_string_lossy(), source)
        .expect("compile overload shapes");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.run_named(&program, "main").expect("run overload shapes");
    assert_eq!(
        vm.exported_values(&program).to_string(),
        "{\"explicit\": \"generic\", \"concrete\": \"concrete\", \"text\": \"text\", \"nilable\": \"nilable\", \"named\": \"named!\", \"variadic\": \"numbers\", \"piped\": \"pipe!\"}"
    );
    assert!(loader.take_warnings().is_empty());
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn equal_and_incomparable_overload_candidates_remain_ambiguous() {
    let root = root("ambiguous-overload-candidates");
    fs::create_dir_all(&root).expect("create module directory");
    let main_path = root.join("main.slug");
    for source in [
        "val same = fn<T>(left:T, right:T) { left }\n\
         val same = fn<T>(left:T, right:str) { left }\n\
         same(\"a\", \"b\")\n",
        "val overlap = fn(value:str|num) { value }\n\
         val overlap = fn(value:str|bool) { value }\n\
         overlap(\"value\")\n",
    ] {
        let error = ModuleLoader::new(&root, None)
            .compile_source(&main_path.to_string_lossy(), source)
            .expect_err("ambiguous overloads are rejected");
        assert!(error.to_string().starts_with("ambiguous overload"));
    }
    fs::remove_dir_all(root).expect("remove module test directory");
}

#[test]
fn selected_defaulted_pipeline_rejects_a_replaced_live_binding() {
    let root = root("live-defaulted-pipeline-overload");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("mutable.slug"),
        "export var decorate = fn(value:str, suffix:str = \"!\"):str { value + suffix }\n\
         export val replace = fn() { decorate = fn(value:num, suffix:num = 0):num { value + suffix } }\n",
    )
    .expect("write mutable overload module");
    fs::write(
        root.join("numbers.slug"),
        "export val decorate = fn(value:num, suffix:num = 0):num { value + suffix }\n",
    )
    .expect("write numeric overload module");
    let main_path = root.join("main.slug");
    let loader = ModuleLoader::new(&root, None);
    let program = loader
        .compile_source(
            &main_path.to_string_lossy(),
            "val api = import(\"mutable\", \"numbers\")\napi.replace()\n\"stale\" /> api.decorate\n",
        )
        .expect("compile selected defaulted pipeline");
    let error = Vm::with_module_loader(loader)
        .run_named(&program, "main")
        .expect_err("replaced selected pipeline rejects stale identity");
    assert_eq!(error.kind, RuntimeErrorKind::Module, "{error:?}");
    assert!(
        error
            .message
            .contains("expected fn<str, str, str>, got fn<num, num, num>")
    );
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
        )
        .expect("compile live-binding overload call");
    let error = Vm::with_module_loader(loader)
        .run_named(&program, "main")
        .expect_err("changed live binding rejects stale selection");
    assert_eq!(error.kind, RuntimeErrorKind::Module);
    assert!(
        error
            .message
            .contains("expected fn<str, str>, got fn<num, num>")
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
fn retains_evaluated_resource_and_enum_declaration_metadata() {
    let root = root("resource-metadata");
    fs::create_dir_all(&root).expect("create module directory");
    fs::write(
        root.join("resources.slug"),
        "/**\n * An exported resource.\n */\n@native(\"sqlite3\")\nexport resource Handle\n\n/**\n * A local resource.\n */\n@private(2)\nresource Cache\n\n/**\n * An exported enum.\n */\n@stable\nexport enum OpenMode { Read, Write }\n\n/**\n * A local enum.\n */\n@internal(\"test\")\nenum State { Idle, Busy }\nexport foreign noop = fn()\n",
    )
    .expect("write resource module");
    let loader = ModuleLoader::new(&root, None);
    let module = NativeModule::new("resources", ()).expect("native module is valid");
    module
        .resource_type("Handle", |_payload: &mut ()| {}, |_payload: ()| {})
        .expect("native resource type is valid");
    module
        .resource_type("Cache", |_payload: &mut ()| {}, |_payload: ()| {})
        .expect("native resource type is valid");
    let mut vm = Vm::with_module_loader(loader.clone());
    vm.define_foreign(
        module
            .function("noop", NativeArity::Exact(0), returns_nil)
            .expect("native function is valid"),
    )
    .expect("foreign binding is unique");

    let instance = loader
        .initialize(None, "resources")
        .expect("initialize resource module");

    assert_eq!(instance.metadata.len(), 5);
    for (declaration, binding, exported, resource_type, documentation, tag, arguments) in [
        (
            &instance.metadata[0],
            "Handle",
            true,
            Some("Handle"),
            "\n * An exported resource.\n ",
            "native",
            vec![Value::string("sqlite3")],
        ),
        (
            &instance.metadata[1],
            "Cache",
            false,
            Some("Cache"),
            "\n * A local resource.\n ",
            "private",
            vec![Value::Int(2)],
        ),
        (
            &instance.metadata[2],
            "OpenMode",
            true,
            None,
            "\n * An exported enum.\n ",
            "stable",
            Vec::new(),
        ),
        (
            &instance.metadata[3],
            "State",
            false,
            None,
            "\n * A local enum.\n ",
            "internal",
            vec![Value::string("test")],
        ),
    ] {
        assert_eq!(declaration.bindings, [binding]);
        assert_eq!(declaration.exported, exported);
        assert_eq!(declaration.resource_type.as_deref(), resource_type);
        assert_eq!(declaration.documentation.as_deref(), Some(documentation));
        assert_eq!(declaration.tags.len(), 1);
        assert_eq!(declaration.tags[0].name, tag);
        assert_eq!(declaration.tags[0].arguments, arguments);
    }
    assert_eq!(
        instance.exports.to_string(),
        "{\"OpenMode\": {\"Read\": OpenMode.Read, \"Write\": OpenMode.Write}, \"noop\": <native resources.noop>}"
    );
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
