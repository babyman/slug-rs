use super::*;

#[test]
fn invokes_a_local_zero_argument_main_after_top_level_evaluation() {
    let path = fixture_path("program-entrypoint");
    fs::write(
        &path,
        "println(\"top level\")\nval main = fn() { println(\"entrypoint\") }\n",
    )
    .expect("write entrypoint source");

    let output = slug().arg(&path).output().expect("run entrypoint source");
    fs::remove_file(path).expect("remove entrypoint source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "top level\nentrypoint\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn skips_defaulted_and_imported_main_functions() {
    let root = std::env::temp_dir().join(format!("slug-cli-main-selection-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create entrypoint fixture directory");
    fs::write(
        root.join("library.slug"),
        "export val main = fn() { println(\"imported\") }\n",
    )
    .expect("write imported main module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val library = import(\"library\")\n\
         val main = fn(value = \"defaulted\") { println(value) }\n\
         println(\"top level\")\n",
    )
    .expect("write non-entrypoint source");

    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-entrypoint source");
    fs::remove_dir_all(root).expect("remove entrypoint fixture directory");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "top level\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn does_not_invoke_main_when_top_level_evaluation_fails() {
    let path = fixture_path("entrypoint-top-level-failure");
    fs::write(&path, "val main = fn() { println(\"entrypoint\") }\n???\n")
        .expect("write failing entrypoint source");

    let output = slug().arg(&path).output().expect("run failing source");
    fs::remove_file(path).expect("remove failing entrypoint source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: not implemented at ")
    );
}

#[test]
fn imports_exported_values_through_the_public_cli() {
    let root =
        std::env::temp_dir().join(format!("slug-cli-imported-values-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(
        root.join("math.slug"),
        "export val answer = 42\nexport val hidden = \"visible\"\n",
    )
    .expect("write imported module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val math = import(\"math\")\nprintln(math.answer, math.hidden)\n",
    )
    .expect("write importing source");

    let output = slug().arg(&path).output().expect("run importing source");

    fs::remove_dir_all(root).expect("remove import fixture directory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42 visible\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn makes_native_println_available_during_imported_module_initialization() {
    let root =
        std::env::temp_dir().join(format!("slug-cli-imported-native-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(
        root.join("library.slug"),
        "println(\"from module\")\nexport val answer = 42\n",
    )
    .expect("write imported module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val library = import(\"library\")\nprintln(library.answer)\n",
    )
    .expect("write importing source");

    let output = slug().arg(&path).output().expect("run importing source");
    fs::remove_dir_all(root).expect("remove import fixture directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "from module\n42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn selects_all_imported_module_exports_into_the_top_level_scope() {
    let root =
        std::env::temp_dir().join(format!("slug-cli-import-all-values-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(
        root.join("math.slug"),
        "export val answer = 42\nexport val label = \"Slug\"\n",
    )
    .expect("write imported module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val {*} = import(\"math\")\nprintln(answer, label)\n",
    )
    .expect("write importing source");

    let output = slug().arg(&path).output().expect("run importing source");

    fs::remove_dir_all(root).expect("remove import fixture directory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42 Slug\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn reports_module_import_conflict_warnings() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-import-conflict-warning-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(root.join("first.slug"), "export val value = 1\n")
        .expect("write first imported module");
    fs::write(root.join("second.slug"), "export val value = 2\n")
        .expect("write second imported module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val values = import(\"first\", \"second\")\nprintln(values.value)\n",
    )
    .expect("write importing source");

    let output = slug().arg(&path).output().expect("run importing source");

    fs::remove_dir_all(root).expect("remove import fixture directory");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "slug: warning: imported binding `value` was ignored because an earlier module provided it\n"
    );
}

#[test]
fn evaluates_source_modulo_with_checked_zero_division() {
    let path = fixture_path("modulo");
    fs::write(&path, "println(17 % 5, 5.5 % 2)\n").expect("write modulo source");
    let output = slug().arg(&path).output().expect("run modulo source");
    fs::remove_file(&path).expect("remove modulo source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "2 1.5\n");

    fs::write(&path, "1 % 0\n").expect("write zero modulo source");
    let output = slug().arg(&path).output().expect("run zero modulo source");
    fs::remove_file(path).expect("remove zero modulo source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: division by zero")
    );
}

#[test]
fn reports_not_implemented_placeholders_as_checked_runtime_errors() {
    let path = fixture_path("not-implemented");
    fs::write(&path, "???\n").expect("write placeholder source");
    let output = slug().arg(&path).output().expect("run placeholder source");
    fs::remove_file(path).expect("remove placeholder source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: not implemented")
    );
}

#[test]
fn repeats_strings_with_non_negative_integer_counts() {
    let path = fixture_path("string-repetition");
    fs::write(&path, "println(\"-\" * 2, \"x\" * 0)\n").expect("write string repetition source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run string repetition source");
    fs::remove_file(&path).expect("remove string repetition source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "-- \n");

    fs::write(&path, "\"x\" * -1\n").expect("write invalid string repetition source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid string repetition source");
    fs::remove_file(path).expect("remove invalid string repetition source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: string repetition count must be non-negative")
    );
}
