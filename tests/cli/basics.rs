use super::*;

#[test]
fn help_describes_the_current_public_capability() {
    let output = slug().arg("--help").output().expect("run slug --help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains(
        "bindings, functions, blocks, conditionals, match, return, throw, defer, recur, collections, arithmetic and logic, calls, and println"
    ));
    assert!(output.stderr.is_empty());
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
fn does_not_expose_channel_close_as_a_global() {
    let path = fixture_path("no-global-channel-close");
    fs::write(&path, "println(close)\n").expect("write close lookup source");
    let output = slug().arg(&path).output().expect("run close lookup source");
    fs::remove_file(path).expect("remove close lookup source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown name `close`"));
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
        "val builtin = import(\"slug.builtin\")\nbuiltin.println(Error { msg: \"ready\" }.type, builtin.Error { msg: \"done\" }.type)\n",
    )
    .expect("write builtin import source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run builtin import source");
    fs::remove_file(path).expect("remove builtin import source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Error Error\n");
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
