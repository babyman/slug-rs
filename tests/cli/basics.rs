use super::*;

#[test]
fn help_describes_the_current_public_capability() {
    let output = slug().arg("--help").output().expect("run slug --help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains(
        "bindings, functions, blocks, conditionals, match, return, throw, defer, recur, collections, arithmetic and logic, calls, print, println, and len"
    ));
    assert!(output.stderr.is_empty());
}

#[test]
fn compiles_with_semantic_type_checking_by_default() {
    let path = fixture_path("default-semantic-checking");
    fs::write(&path, "1 + true\n").expect("write invalid typed source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid typed source");
    fs::remove_file(path).expect("remove invalid typed source");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: operator `+` does not accept num and bool")
    );
}

#[test]
fn does_not_recognize_the_removed_type_check_mode() {
    let output = slug()
        .arg("-type-check")
        .output()
        .expect("run removed type-check mode");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: cannot read")
    );
}

#[test]
fn version_is_available_without_loading_source() {
    let output = slug()
        .arg("--version")
        .output()
        .expect("run slug --version");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version is UTF-8"),
        "slug-vm 0.1.0\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_a_minimal_calculation_through_the_public_cli() {
    let path = fixture_path("minimal-calculation");
    fs::write(&path, "println(1 + 1)\n").expect("write minimal Slug source");
    let output = slug().arg(&path).output().expect("run minimal Slug source");
    fs::remove_file(path).expect("remove minimal Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "2\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn treats_diagnostic_format_after_the_program_path_as_a_program_argument() {
    let path = fixture_path("post-program-diagnostic-format-argument");
    fs::write(
        &path,
        "val main = fn(args:list) { println(args[0] == \"--diagnostic-format=json\") }\n",
    )
    .expect("write argument source");
    let output = slug()
        .arg(&path)
        .arg("--diagnostic-format=json")
        .output()
        .expect("run program with JSON-named argument");
    fs::remove_file(path).expect("remove argument source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "true\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_a_bare_program_name_from_the_library_directory() {
    let root = std::env::temp_dir().join(format!(
        "slug-cli-library-entrypoint-{}",
        std::process::id()
    ));
    let home = root.join("home");
    let working_directory = root.join("project");
    fs::create_dir_all(home.join("lib")).expect("create library directory");
    fs::create_dir_all(&working_directory).expect("create working directory");
    fs::write(home.join("lib/hello.slug"), "println(\"Hello Slug!\")\n")
        .expect("write installed Slug program");

    let output = slug()
        .current_dir(&working_directory)
        .arg("hello")
        .env("SLUG_HOME", &home)
        .env_remove("SLUG_FIXTURE_LIBRARY_ROOT")
        .output()
        .expect("run installed Slug program");

    fs::remove_dir_all(root).expect("remove entrypoint fixture directory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Hello Slug!\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn writes_with_print_without_a_newline_and_returns_nil() {
    let path = fixture_path("print");
    fs::write(
        &path,
        "val result = print(\"first\", 2)\nprint()\nprintln(result, \"last\")\n",
    )
    .expect("write print source");
    let output = slug().arg(&path).output().expect("run print source");
    fs::remove_file(path).expect("remove print source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "first 2nil last\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn returns_lengths_for_supported_values_and_rejects_other_values() {
    let path = fixture_path("len");
    fs::write(
        &path,
        "println(len(\"aé😀\"), len(0x\"414243\"), len([1, 2]), len({name: 1, active: true}))\n",
    )
    .expect("write len source");
    let output = slug().arg(&path).output().expect("run len source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3 3 2 2\n");
    assert!(output.stderr.is_empty());

    fs::write(&path, "len(nil)\n").expect("write invalid len source");
    let output = slug().arg(&path).output().expect("run invalid len source");
    fs::remove_file(path).expect("remove len source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("`len` expects str, bytes, list, or map, got Nil")
    );
}

#[test]
fn does_not_expose_the_internal_channel_constructor_as_a_global() {
    let path = fixture_path("no-global-channel");
    fs::write(&path, "println(channel)\n").expect("write channel lookup source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run channel lookup source");
    fs::remove_file(path).expect("remove channel lookup source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown name `channel`"));
}

#[test]
fn exposes_channel_close_as_a_builtin() {
    let path = fixture_path("builtin-channel-close");
    fs::write(
        &path,
        "val channel = chan()\nclose(channel)\nprintln(channel)\n",
    )
    .expect("write close source");
    let output = slug().arg(&path).output().expect("run close lookup source");
    fs::remove_file(path).expect("remove close lookup source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "<chan>\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn does_not_expose_task_await_as_a_global() {
    let path = fixture_path("no-global-await");
    fs::write(&path, "println(await)\n").expect("write await lookup source");
    let output = slug().arg(&path).output().expect("run await lookup source");
    fs::remove_file(path).expect("remove await lookup source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown name `await`"));
}

#[test]
fn does_not_expose_channel_operations_as_globals() {
    for name in ["send", "recv"] {
        let path = fixture_path(&format!("no-global-{name}"));
        fs::write(&path, format!("println({name})\n"))
            .expect("write channel operation lookup source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run channel operation lookup source");
        fs::remove_file(path).expect("remove channel operation lookup source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&format!("unknown name `{name}`"))
        );
    }
}

#[test]
fn exposes_builtin_bindings_implicitly_and_by_explicit_import() {
    let path = fixture_path("builtin-module");
    fs::write(
        &path,
        "val builtin = import(\"slug.builtin\")\nval implicit = chan(1)\nbuiltin.close(implicit)\nbuiltin.print(builtin.len([1, 2]))\nbuiltin.println(Error { msg: \"ready\" }.type, builtin.Error { msg: \"done\" }.type)\nbuiltin.println(implicit == builtin.chan(1))\n",
    )
    .expect("write builtin import source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run builtin import source");
    fs::remove_file(path).expect("remove builtin import source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "2Error Error\nfalse\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn imports_library_modules_from_slug_home() {
    let path = fixture_path("slug-home-library");
    let home = std::env::temp_dir().join(format!("slug-home-library-{}", std::process::id()));
    fs::create_dir_all(home.join("lib/slug")).expect("create SLUG_HOME library directory");
    fs::write(
        home.join("lib/slug/example.slug"),
        "export val answer = 42\n",
    )
    .expect("write SLUG_HOME library module");
    fs::write(
        &path,
        "val builtin = import(\"slug.builtin\")\nval example = import(\"slug.example\")\nbuiltin.println(example.answer)\n",
    )
    .expect("write library-importing source");

    let output = slug()
        .arg(&path)
        .env("SLUG_HOME", &home)
        .env_remove("SLUG_FIXTURE_LIBRARY_ROOT")
        .output()
        .expect("run source with SLUG_HOME");
    fs::remove_file(&path).expect("remove library-importing source");
    fs::remove_dir_all(&home).expect("remove SLUG_HOME library directory");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
    assert!(output.stderr.is_empty());
}
