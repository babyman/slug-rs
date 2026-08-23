use std::fs;

use slug_vm::{Configuration, ConfigurationValue, ModuleLoader, Value, Vm, compile};

fn root(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-configuration-{kind}-{}", std::process::id()))
}

#[test]
fn merges_toml_environment_and_options_in_precedence_order() {
    let root = root("precedence");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(home.join("lib")).expect("create library configuration directory");
    fs::create_dir_all(&project).expect("create project configuration directory");
    fs::write(
        home.join("lib/slug.toml"),
        "[server]\nport = 1000\nname = \"library\"\nvalues = [1, 2]\n",
    )
    .expect("write library configuration");
    fs::write(
        project.join("slug.toml"),
        "[server]\nport = 2000\nname = \"project\"\n",
    )
    .expect("write project configuration");
    let configuration = Configuration::load(
        &project,
        Some(&home),
        [("SLUG__server__port".into(), "3000".into())],
        &[
            "--port=4000".into(),
            "--feature.enabled".into(),
            "--tag".into(),
            "first".into(),
            "--tag".into(),
            "second".into(),
        ],
        "server",
    );

    assert_eq!(
        configuration.get("server.port"),
        Some(&ConfigurationValue::Text("4000".into()))
    );
    assert_eq!(
        configuration.get("server.name"),
        Some(&ConfigurationValue::Value(Value::string("project")))
    );
    assert_eq!(
        configuration.get("server.values"),
        Some(&ConfigurationValue::Value(Value::List(
            vec![Value::Int(1), Value::Int(2)].into()
        )))
    );
    assert_eq!(
        configuration.get("feature.enabled"),
        Some(&ConfigurationValue::Text("true".into()))
    );
    assert_eq!(
        configuration.get("server.tag"),
        Some(&ConfigurationValue::TextList(vec![
            "first".into(),
            "second".into()
        ]))
    );
    fs::remove_dir_all(root).expect("remove configuration directory");
}

#[test]
fn ignores_missing_or_malformed_optional_toml_and_remains_immutable() {
    let root = root("immutable");
    fs::create_dir_all(&root).expect("create configuration directory");
    let path = root.join("slug.toml");
    fs::write(&path, "[server]\nport = 3000\n").expect("write project configuration");
    let configuration = Configuration::load(&root, None, [], &[], "server");
    fs::write(&path, "not valid = [toml\n").expect("replace project configuration");

    assert_eq!(
        configuration.get("server.port"),
        Some(&ConfigurationValue::Value(Value::Int(3000)))
    );
    let malformed = Configuration::load(&root, None, [], &[], "server");
    assert_eq!(malformed.get("server.port"), None);
    fs::remove_dir_all(root).expect("remove configuration directory");
}

#[test]
fn exposes_cfg_argv_and_argm_to_program_and_imported_modules() {
    let root = root("builtins");
    fs::create_dir_all(&root).expect("create configuration directory");
    fs::write(
        root.join("library.slug"),
        "export val port = cfg(\"port\", 80)\n",
    )
    .expect("write library module");
    let configuration = Configuration::load(
        &root,
        None,
        [
            ("SLUG__app__port".into(), "3001".into()),
            ("SLUG__feature__enabled".into(), "true".into()),
            ("SLUG__library__port".into(), "4000".into()),
        ],
        &[
            "--port".into(),
            "3002".into(),
            "first".into(),
            "--tag=alpha".into(),
            "--tag".into(),
            "beta".into(),
            "--".into(),
            "tail".into(),
        ],
        "app",
    );
    let loader = ModuleLoader::with_configuration(&root, None, configuration);
    let main_path = root.join("app.slug");
    let mut program = compile(
        &main_path.to_string_lossy(),
        "val library = import(\"library\")\n\
         export val values = [cfg(\"port\", 80), cfg(\"feature.enabled\", false), cfg(\"missing\", [\"fallback\"]), library.port, argv(), argm()]\n",
    )
    .expect("compile configuration program");
    program.set_module_name("app");
    let mut vm = Vm::with_module_loader(loader);

    vm.run_named(&program, "main")
        .expect("execute configuration builtins");

    let Some(Value::List(values)) = vm.global("values") else {
        panic!("configuration values must be exported as a list");
    };
    assert_eq!(
        &values[..5],
        [
            Value::Int(3002),
            Value::Bool(true),
            Value::List(vec![Value::string("fallback")].into()),
            Value::Int(4000),
            Value::List(
                vec![
                    Value::string("--port"),
                    Value::string("3002"),
                    Value::string("first"),
                    Value::string("--tag=alpha"),
                    Value::string("--tag"),
                    Value::string("beta"),
                    Value::string("--"),
                    Value::string("tail"),
                ]
                .into()
            ),
        ]
    );
    let argument_map = values[5].to_string();
    assert!(argument_map.contains("\"app.port\": \"3002\""));
    assert!(argument_map.contains("\"app.tag\": [\"alpha\", \"beta\"]"));
    assert!(argument_map.contains("\"positional\": [\"first\", \"tail\"]"));
    fs::remove_dir_all(root).expect("remove configuration directory");
}
