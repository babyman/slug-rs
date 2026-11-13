use super::*;

#[test]
fn imports_body_derived_parameter_signatures_without_reinferring_them() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-imported-inferred-parameters-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create inferred parameter fixture directory");
    fs::write(
        root.join("numeric.slug"),
        "export val divide = fn(value) { value / 2 }\n",
    )
    .expect("write inferred export module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val numeric = import(\"numeric\")\nprintln(numeric.divide(8))\n",
    )
    .expect("write compatible importing source");
    let output = slug().arg(&path).output().expect("run compatible import");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "4\n");

    fs::write(
        &path,
        "val numeric = import(\"numeric\")\nnumeric.divide(\"x\")\n",
    )
    .expect("write incompatible importing source");
    let output = slug().arg(&path).output().expect("run incompatible import");
    fs::remove_dir_all(root).expect("remove inferred parameter fixture directory");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );
}

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
fn a_number_main_returns_is_the_exit_status() {
    let path = fixture_path("program-entrypoint-status");
    let artifact = fixture_path("program-entrypoint-status.cslug");
    let cases = [
        ("3", Some(3), ""),
        ("2.0", Some(2), ""),
        ("\"done\"", Some(0), ""),
        (
            "256",
            Some(1),
            "slug: main returned 256; an exit status is an integer from 0 to 255\n",
        ),
        (
            "1.5",
            Some(1),
            "slug: main returned 1.5; an exit status is an integer from 0 to 255\n",
        ),
    ];
    for (result, status, error) in cases {
        fs::write(&path, format!("val main = fn() {{ {result} }}\n")).expect("write source");
        let output = slug().arg(&path).output().expect("run source");
        assert_eq!(output.status.code(), status, "{result}");
        assert!(
            String::from_utf8(output.stderr).unwrap().starts_with(error),
            "{result}"
        );

        let compile = slug()
            .args([
                "compile",
                path.to_str().unwrap(),
                artifact.to_str().unwrap(),
            ])
            .output()
            .expect("compile source");
        assert!(compile.status.success(), "{compile:?}");
        let output = slug()
            .args(["run", artifact.to_str().unwrap()])
            .output()
            .expect("run artifact");
        assert_eq!(output.status.code(), status, "{result} artifact");
    }

    // A program without `main` succeeds whatever its last value is.
    fs::write(&path, "7\n").expect("write top-level source");
    let output = slug().arg(&path).output().expect("run top-level source");
    fs::remove_file(path).expect("remove source");
    fs::remove_file(artifact).expect("remove artifact");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn passes_raw_program_arguments_to_a_list_main() {
    let path = fixture_path("program-entrypoint-list");
    fs::write(
        &path,
        "val main = fn(value:str) { println(\"wrong\") }\n\
         val main = fn(args:list) { println(args[0], args[1]) }\n",
    )
    .expect("write list entrypoint source");

    let output = slug()
        .arg(&path)
        .arg("--feature.enabled")
        .arg("first")
        .output()
        .expect("run list entrypoint source");
    fs::remove_file(path).expect("remove list entrypoint source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "--feature.enabled first\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn passes_parsed_program_arguments_to_a_map_main() {
    let path = fixture_path("program-entrypoint-map");
    fs::write(
        &path,
        "val main = fn(args:map) { println(args.options[\"feature.enabled\"], args.positional[0]) }\n",
    )
    .expect("write map entrypoint source");

    let output = slug()
        .arg(&path)
        .arg("--feature.enabled=true")
        .arg("first")
        .output()
        .expect("run map entrypoint source");
    fs::remove_file(path).expect("remove map entrypoint source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "true first\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_multiple_eligible_main_entrypoints() {
    let path = fixture_path("duplicate-program-entrypoint");
    fs::write(
        &path,
        "val main = fn() { println(\"zero\") }\nval main = fn(args:list) { println(args) }\n",
    )
    .expect("write duplicate entrypoint source");

    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate entrypoint source");
    fs::remove_file(path).expect("remove duplicate entrypoint source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr).unwrap().starts_with(
            "slug: semantic error: program has more than one eligible `main` entrypoint"
        )
    );
}

#[test]
fn does_not_expose_retired_argument_builtins() {
    let path = fixture_path("retired-argument-builtins");
    fs::write(&path, "println(argv())\n").expect("write retired builtin source");

    let output = slug()
        .arg(&path)
        .output()
        .expect("run retired builtin source");
    fs::remove_file(path).expect("remove retired builtin source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown name `argv`"), "{stderr}");
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
    fs::remove_file(&path).expect("remove failing entrypoint source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: not implemented\n"));
    assert!(stderr.contains(&format!("    --> {}:2:1\n", path.display())));
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
fn imported_collection_exports_remain_immutable_after_updates() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-imported-collection-aliases-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(
        root.join("library.slug"),
        "export val User = struct { name, active = true }\n\
         export val list = [1, 2]\n\
         export val bytes = 0x\"0203\"\n\
         export val map = {name: \"Slug\", version: 1}\n\
         export val user = User {name: \"Slug\"}\n",
    )
    .expect("write collection export module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val library = import(\"library\")\n\
         val list = library.list :+ 3\n\
         val bytes = library.bytes :+ 4\n\
         val map = library.map copy {version: 2}\n\
         val user = library.user copy {active: false}\n\
         println(library.list, list, library.bytes, bytes, library.map.version, map.version, library.user.active, user.active)\n",
    )
    .expect("write collection import source");

    let output = slug().arg(&path).output().expect("run importing source");
    fs::remove_dir_all(root).expect("remove import fixture directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[1, 2] [1, 2, 3] 0x\"0203\" 0x\"020304\" 1 2 true false\n"
    );
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

#[test]
fn a_bare_program_name_imports_sibling_modules() {
    let directory =
        std::env::temp_dir().join(format!("slug-cli-bare-entry-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("create bare entry directory");
    fs::write(directory.join("helper.slug"), "export val answer = 42\n").expect("write helper");
    fs::write(
        directory.join("app.slug"),
        "val helper = import(\"helper\")\nprintln(helper.answer)\n",
    )
    .expect("write entry");
    let output = slug()
        .current_dir(&directory)
        .arg("app.slug")
        .output()
        .expect("run bare entry");
    fs::remove_dir_all(&directory).expect("remove bare entry directory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn describes_a_modules_declarations_and_gates_unexported_ones() {
    let root = std::env::temp_dir().join(format!("slug-cli-describe-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create describe fixture directory");
    fs::write(
        root.join("lib.slug"),
        "/**\n * Adds two numbers.\n */\n\
         @testWith([1, 2], 3)\n\
         @testWith([0, 0], 0)\n\
         export val add = fn(a, b) { a + b }\n\
         \n\
         @test\n\
         val helper = fn() { add(2, 2) == 4 }\n\
         \n\
         export var count = 7\n",
    )
    .expect("write described module");
    let path = root.join("main.slug");
    let run = |source: &str, allow: bool| {
        fs::write(&path, source).expect("write describing source");
        let mut command = slug();
        if allow {
            command.arg("--allow-unexported");
        }
        let output = command.arg(&path).output().expect("run describing source");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // Exported declarations, in source order, with their documentation,
    // evaluated tags, and values.
    let exported = "val all = describe(\"lib\")\n\
         println(len(all), all[0].name, all[0].exported, all[0].mutable, all[0].documentation)\n\
         println(all[0].tags, all[0].value(20, 22))\n\
         println(all[1].name, all[1].mutable, all[1].documentation, all[1].value)\n";
    for allow in [false, true] {
        let (code, stdout, stderr) = run(exported, allow);
        assert_eq!(code, Some(0), "{stderr}");
        assert_eq!(
            stdout,
            "2 add true false Adds two numbers.\n\
             [{\"name\": \"testWith\", \"arguments\": [[1, 2], 3]}, {\"name\": \"testWith\", \"arguments\": [[0, 0], 0]}] 42\n\
             count true nil 7\n"
        );
    }

    // Unexported declarations need the user's permission.
    let unexported = "val all = describe(\"lib\", unexported = true)\n\
         println(len(all), all[1].name, all[1].exported, all[1].tags, all[1].value())\n";
    let (code, stdout, stderr) = run(unexported, true);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "3 helper false [{\"name\": \"test\", \"arguments\": []}] true\n"
    );
    for source in [unexported, "describe(\"lib\", true)\n"] {
        let (code, _, stderr) = run(source, false);
        assert_eq!(code, Some(1), "{source}");
        assert!(
            stderr.contains(
                "runtime error: `describe` with `unexported = true` requires running slug with --allow-unexported"
            ),
            "{source}: {stderr}"
        );
        assert!(stderr.contains("code: permission"), "{stderr}");
    }

    // A module that does not exist fails as an import does.
    let (code, _, stderr) = run("describe(\"missing\")\n", false);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("module `missing` was not found"),
        "{stderr}"
    );
    fs::remove_dir_all(root).expect("remove describe fixture directory");
}

#[test]
fn clutch_assets_embed_at_compile_time() {
    let root = std::env::temp_dir().join(format!("slug-cli-assets-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let clutch = root.join("home/clutch/example.clutch");
    fs::create_dir_all(clutch.join("modules")).expect("create clutch modules");
    fs::create_dir_all(clutch.join("assets")).expect("create clutch assets");
    fs::write(
        root.join("home/clutch/manifest.toml"),
        "[modules]\n\"example.assets\" = \"example.clutch\"\n",
    )
    .expect("write repository manifest");
    fs::write(
        clutch.join("clutch.toml"),
        "[modules]\n\"example.assets\" = { source = \"modules/assets.slug\" }\n",
    )
    .expect("write clutch manifest");
    fs::write(
        clutch.join("modules/assets.slug"),
        "export val greeting = asset(\"greeting.txt\")\n",
    )
    .expect("write clutch module");
    fs::write(clutch.join("assets/greeting.txt"), "hello\n").expect("write asset");
    fs::write(
        clutch.join("play.slug"),
        "println(len(asset(\"greeting.txt\")))\n",
    )
    .expect("write program in clutch");
    fs::write(
        root.join("main.slug"),
        "val {greeting} = import(\"example.assets\")\nprintln(greeting)\n",
    )
    .expect("write importer");
    fs::write(
        root.join("outside.slug"),
        "println(asset(\"greeting.txt\"))\n",
    )
    .expect("write program outside clutch");
    let run = |program: &std::path::Path, json: bool| {
        let mut command = slug();
        if json {
            command.arg("--diagnostic-format=json");
        }
        let output = command
            .env("SLUG_HOME", root.join("home"))
            .arg(program)
            .output()
            .expect("run asset program");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (code, stdout, stderr) = run(&root.join("main.slug"), false);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "0x\"68656c6c6f0a\"\n");
    let (code, stdout, stderr) = run(&clutch.join("play.slug"), false);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "6\n");

    let (code, stdout, stderr) = run(&root.join("outside.slug"), false);
    assert_eq!(code, Some(1));
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr.contains("semantic error: no asset `greeting.txt`:")
            && stderr.contains("is not in a clutch"),
        "{stderr}"
    );
    let (_, _, stderr) = run(&root.join("outside.slug"), true);
    let diagnostic: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("JSON diagnostic");
    assert_eq!(diagnostic["code"], "invalid_asset");
    fs::remove_dir_all(root).expect("remove asset fixture");
}

/// Runs the program at `path` from source, and compiled into an artifact,
/// returning both outputs.
fn run_as_source_and_artifact(path: &std::path::Path) -> [std::process::Output; 2] {
    let artifact = path.with_extension("cslug");
    let source = slug().arg(path).output().expect("run source");
    let compile = slug()
        .args([
            "compile",
            path.to_str().unwrap(),
            artifact.to_str().unwrap(),
        ])
        .output()
        .expect("compile artifact");
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let compiled = slug()
        .args(["run", artifact.to_str().unwrap()])
        .output()
        .expect("run artifact");
    [source, compiled]
}

#[test]
fn bytes_variables_updated_in_place_keep_value_semantics() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-bytes-variable-updates-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create update fixture directory");
    let program = root.join("main.slug");
    // A module-level and a captured bytes variable, appended to and
    // concatenated in place: values read before keep what they were, and
    // an append that fails (not a byte) leaves the variable as it was.
    fs::write(
        &program,
        "var g = 0x\"01\"\n\
         val id = fn(v) { v }\n\
         val main = fn() {\n\
             var c = 0x\"0a\"\n\
             val before = [g, c]\n\
             val grow = fn(i:num) { if (i < 3) { g = g :+ i; c = c :+ (i + 16); recur(i + 1) } }\n\
             grow(0)\n\
             val join = fn() { g = g + 0x\"ff\"; c = c + 0x\"ee\" }\n\
             join()\n\
             println(before)\n\
             println([g, c])\n\
             val fail = fn(change) {\n\
                 defer onerror(e) { println(e.msg) }\n\
                 change()\n\
             }\n\
             fail(fn() { g = g :+ id(300) })\n\
             fail(fn() { c = c :+ id(\"no\") })\n\
             println([g, c])\n\
         }\n",
    )
    .expect("write update program");
    for output in run_as_source_and_artifact(&program) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(
            lines,
            [
                "[0x\"01\", 0x\"0a\"]",
                "[0x\"01000102ff\", 0x\"0a101112ee\"]",
                "value for :+ on bytes must be an integer from 0 through 255",
                "value for :+ on bytes must be an integer from 0 through 255 or one byte",
                "[0x\"01000102ff\", 0x\"0a101112ee\"]",
            ],
            "{stdout}"
        );
    }
    fs::remove_dir_all(root).expect("remove update fixture");
}

#[test]
fn a_captured_variable_updated_in_place_keeps_value_semantics() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-captured-variable-updates-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create update fixture directory");
    let program = root.join("main.slug");
    // Closures update `var`s they capture from `main`. A value read before
    // keeps what it was, another closure sharing the variable sees each
    // update, a call in the right operand that assigns the variable is
    // overwritten as before, and an update that fails leaves it as it was.
    fs::write(
        &program,
        "val id = fn(v) { v }\n\
         val main = fn() {\n\
             var m:map = {a: 1}\n\
             var l:list = [1]\n\
             var s = \"x\"\n\
             val before = [m, l]\n\
             val bump = fn() { m = m + {bumped: true}; {c: 3} }\n\
             val read = fn() { [m, l, s] }\n\
             val update = fn() {\n\
                 val go = fn(i:num) { if (i < 3) { m = m + {[\"k\" + i]: i}; l = l :+ i; recur(i + 1) } }\n\
                 go(0)\n\
                 val calls = [bump]\n\
                 m = m + calls[0]()\n\
                 m = m - \"a\"\n\
                 l = l + [9]\n\
                 s = s + \"y\" + 5\n\
             }\n\
             update()\n\
             println(before)\n\
             println(read())\n\
             val fail = fn(change) {\n\
                 defer onerror(e) { println(e.msg) }\n\
                 change()\n\
             }\n\
             fail(fn() { l = l + id(5) })\n\
             fail(fn() { m = m - id(fn() { 1 }) })\n\
             println(read())\n\
         }\n",
    )
    .expect("write update program");
    for output in run_as_source_and_artifact(&program) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines = stdout.lines().collect::<Vec<_>>();
        let after = "[{\"c\": 3, \"k0\": 0, \"k1\": 1, \"k2\": 2}, [1, 0, 1, 2, 9], \"xy5\"]";
        assert_eq!(lines[0], "[{\"a\": 1}, [1]]", "{stdout}");
        assert_eq!(lines[1], after, "{stdout}");
        assert_eq!(lines[4], after, "{stdout}");
        assert_eq!(lines.len(), 5, "{stdout}");
    }
    fs::remove_dir_all(root).expect("remove update fixture");
}

#[test]
fn a_module_variable_updated_in_place_keeps_value_semantics() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-module-variable-updates-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create update fixture directory");
    let program = root.join("main.slug");
    // Each assignment updates the collection its module-level variable
    // holds. A value read from it before keeps what it was, a call in the
    // right operand that assigns the variable is overwritten as before, and
    // an update that fails leaves the variable as it was.
    fs::write(
        &program,
        "var m:map = {a: 1}\n\
         var l:list = [1]\n\
         var s = \"x\"\n\
         val S = struct { x, y }\n\
         val id = fn(v) { v }\n\
         var r = id(S { x: 1, y: 2 })\n\
         var c = id({hp: 3})\n\
         val bump = fn() { m = m + {bumped: true}; {c: 3} }\n\
         val main = fn() {\n\
             val before = [m, l, r, c]\n\
             m = m + {b: 2}\n\
             val calls = [bump]\n\
             m = m + calls[0]()\n\
             m = m - \"a\"\n\
             l = l :+ 2\n\
             l = l + [3]\n\
             s = s + \"y\" + 5\n\
             r = r copy {y: 9}\n\
             c = c copy {hp: c.hp - 1}\n\
             println(before)\n\
             println(m, l, s, r, c)\n\
             val fail = fn(update) {\n\
                 defer onerror(e) { println(e.msg) }\n\
                 update()\n\
             }\n\
             fail(fn() { l = l + id(5) })\n\
             fail(fn() { m = m - id(fn() { 1 }) })\n\
             fail(fn() { r = r copy {z: 1} })\n\
             fail(fn() { c = c + 1 })\n\
             println(m, l, r, c)\n\
         }\n",
    )
    .expect("write update program");
    for output in run_as_source_and_artifact(&program) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(
            lines[..2],
            [
                "[{\"a\": 1}, [1], struct {\"x\": 1, \"y\": 2}, {\"hp\": 3}]",
                "{\"c\": 3, \"b\": 2} [1, 2, 3] xy5 struct {\"x\": 1, \"y\": 9} {\"hp\": 2}",
            ],
            "{stdout}"
        );
        assert_eq!(
            lines[6], "{\"c\": 3, \"b\": 2} [1, 2, 3] struct {\"x\": 1, \"y\": 9} {\"hp\": 2}",
            "{stdout}"
        );
        assert_eq!(lines.len(), 7, "{stdout}");
    }
    fs::remove_dir_all(root).expect("remove update fixture");
}

#[test]
fn a_module_variable_field_updated_in_place_keeps_value_semantics() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-module-field-updates-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create field update fixture directory");
    let program = root.join("main.slug");
    // Each copy's last field updates that field of the variable, which the
    // copy then holds alone. Values read before keep what they were, an
    // update that fails in its operator or an earlier field leaves the
    // variable as it was, and a copy of another value reads the variable's
    // field without changing it.
    fs::write(
        &program,
        "val id = fn(v) { v }\n\
         val P = struct { name, memory, log }\n\
         var p = id(P { name: \"a\", memory: {}, log: [] })\n\
         var d:map = {players: {}, levels: {\"zz\": 0}, n: 0}\n\
         val main = fn() {\n\
             p = p copy {memory: p.memory + {k1: 1}}\n\
             val keepP = p\n\
             val keepMemory = p.memory\n\
             p = p copy {memory: p.memory + {k2: 2}}\n\
             p = p copy {name: \"b\", log: p.log :+ \"x\"}\n\
             p = p copy {log: p.log + [\"y\"]}\n\
             println(keepP, keepMemory, p)\n\
             d = d copy {players: d.players + {a: true}}\n\
             d = d copy {n: d.n + 1, levels: d.levels - \"zz\"}\n\
             val keepD = d\n\
             d = d copy {levels: d.levels + {\"0,0\": 1}}\n\
             println(keepD, d)\n\
             val fail = fn(update) {\n\
                 defer onerror(e) { println(e.msg) }\n\
                 update()\n\
             }\n\
             fail(fn() { p = p copy {memory: p.memory + id(1)} })\n\
             fail(fn() { p = p copy {name: id(nil).x, memory: p.memory + {k3: 3}} })\n\
             fail(fn() { d = d copy {players: d.players - id(fn() { 1 })} })\n\
             fail(fn() { p = id(P { name: \"c\", memory: {}, log: [] }) copy {memory: p.memory + {k4: 4}} })\n\
             println(p, d)\n\
         }\n",
    )
    .expect("write field update program");
    for output in run_as_source_and_artifact(&program) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "struct {\"name\": \"a\", \"memory\": {\"k1\": 1}, \"log\": []} {\"k1\": 1} \
             struct {\"name\": \"b\", \"memory\": {\"k1\": 1, \"k2\": 2}, \"log\": [\"x\", \"y\"]}\n\
             {\"players\": {\"a\": true}, \"levels\": {}, \"n\": 1} \
             {\"players\": {\"a\": true}, \"levels\": {\"0,0\": 1}, \"n\": 1}\n\
             expected numbers, got map and num\n\
             cannot index nil\n\
             fn cannot be used as a map key\n\
             struct {\"name\": \"c\", \"memory\": {\"k1\": 1, \"k2\": 2, \"k4\": 4}, \"log\": []} \
             {\"players\": {\"a\": true}, \"levels\": {\"0,0\": 1}, \"n\": 1}\n"
        );
    }
    fs::remove_dir_all(root).expect("remove field update fixture");
}

#[test]
fn an_import_in_a_function_resolves_from_the_module_that_made_it() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-import-in-function-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("sub")).expect("create import fixture directory");
    // `helper` is beside `lib`, not beside `main`, so it resolves only from
    // `lib`, whose function `main` calls.
    fs::write(
        root.join("sub/lib.slug"),
        "export val hello = fn() { import(\"helper\").message }\n",
    )
    .expect("write importing module");
    fs::write(
        root.join("sub/helper.slug"),
        "export val message = \"from helper\"\n",
    )
    .expect("write imported module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val lib = import(\"sub.lib\")\nval main = fn() { println(lib.hello()) }\n",
    )
    .expect("write program");

    let outputs = run_as_source_and_artifact(&path);
    fs::remove_dir_all(root).expect("remove import fixture directory");
    for output in outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "from helper\n");
    }
}

#[test]
fn a_module_that_fails_to_initialize_is_the_cause_of_its_import() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-initialization-failure-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create import fixture directory");
    fs::write(root.join("c.slug"), "export val x = [1][5]\n").expect("write failing module");
    fs::write(
        root.join("b.slug"),
        "val c = import(\"c\")\nexport val y = 1\n",
    )
    .expect("write middle module");
    fs::write(
        root.join("a.slug"),
        "val b = import(\"b\")\nexport val z = 1\n",
    )
    .expect("write outer module");
    let path = root.join("main.slug");
    fs::write(
        &path,
        "val a = import(\"a\")\nval main = fn() { println(a.z) }\n",
    )
    .expect("write program");

    let outputs = run_as_source_and_artifact(&path);
    let c = root
        .join("c.slug")
        .canonicalize()
        .expect("canonical module path");
    let b = root
        .join("b.slug")
        .canonicalize()
        .expect("canonical module path");
    fs::remove_dir_all(&root).expect("remove import fixture directory");
    for output in outputs {
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        let (import, cause) = stderr
            .split_once("\n\ncaused by: ")
            .unwrap_or_else(|| panic!("the failure is the cause: {stderr}"));
        assert!(
            import.starts_with("slug: runtime error: cannot initialize ")
                && import.contains(": list index is out of bounds\n")
                && import.ends_with("= code: module"),
            "{stderr}"
        );
        assert!(
            cause.starts_with(&format!(
                "runtime error: list index is out of bounds\n    --> {}:1:19\n",
                c.display()
            )),
            "{stderr}"
        );
        assert!(
            cause.contains(&format!("\n  in main at {}:1:9\n", b.display())),
            "{stderr}"
        );
        assert!(cause.ends_with("= code: out_of_bounds\n"), "{stderr}");
        assert_eq!(stderr.matches("runtime error:").count(), 2, "{stderr}");
    }
}
