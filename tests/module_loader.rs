use std::fs;

use slug_vm::{ModuleLoadError, ModuleLoader};

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
