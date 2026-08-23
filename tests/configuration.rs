use std::fs;

use slug_vm::{Configuration, ConfigurationValue, Value};

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
