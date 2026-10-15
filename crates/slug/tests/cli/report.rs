use super::*;

const NATIVE_MODULE: &str = "@native(\"demo\")\nexport resource Handle\n\n@stable\nexport foreign open = fn(path:str, mode:str = \"r\"):Handle\nforeign log = fn(...values)\n";

const ENTRY: &str = "val n = import(\"report_native\")\nval which = \"slug.\" + \"std\"\nval other = import(which)\nprintln(cfg(\"port\", 8080), n.open)\n";

fn report_dir(kind: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("slug-cli-report-{kind}-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("create report directory");
    fs::write(directory.join("report_native.slug"), NATIVE_MODULE).expect("write native module");
    fs::write(directory.join("entry.slug"), ENTRY).expect("write entry module");
    directory
}

fn report(path: &std::path::Path) -> serde_json::Value {
    let output = slug().arg("report").arg(path).output().expect("run report");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("report JSON")
}

#[test]
fn reports_source_capabilities_with_dynamic_imports_and_signatures() {
    let directory = report_dir("source");
    let entry = directory.join("entry.slug");
    let report = report(&entry);
    fs::remove_dir_all(&directory).expect("remove report directory");

    assert_eq!(report["version"], 1);
    assert_eq!(report["entry"], entry.to_string_lossy().as_ref());
    assert_eq!(report["closed"], false);
    let modules = report["modules"].as_array().expect("modules");
    assert_eq!(modules.len(), 2);

    let root = &modules[0];
    assert_eq!(root["name"], report["entry"]);
    assert_eq!(root["imports"], serde_json::json!(["report_native"]));
    assert_eq!(
        root["dynamic_imports"],
        serde_json::json!([{ "line": 3, "column": 13 }])
    );
    assert_eq!(root["builtins"], serde_json::json!(["cfg", "println"]));

    let native = &modules[1];
    assert_eq!(native["name"], "report_native");
    assert_eq!(native["dynamic_imports"], serde_json::json!([]));
    assert_eq!(
        native["resources"],
        serde_json::json!([{ "name": "Handle", "exported": true, "tags": ["native"] }])
    );
    assert_eq!(
        native["foreign"],
        serde_json::json!([
            {
                "name": "open",
                "exported": true,
                "arity": { "min": 1, "max": 2 },
                "parameters": [
                    { "name": "path", "type": "str", "variadic": false, "default": false },
                    { "name": "mode", "type": "str", "variadic": false, "default": true },
                ],
                "result": "Handle",
                "tags": ["stable"],
            },
            {
                "name": "log",
                "exported": false,
                "arity": { "min": 0, "max": null },
                "parameters": [
                    { "name": "values", "type": null, "variadic": true, "default": false },
                ],
                "result": null,
                "tags": [],
            },
        ])
    );
}

#[test]
fn reports_compiled_artifacts_as_closed() {
    let directory = report_dir("artifact");
    let artifact = directory.join("entry.cslug");
    let compiled = slug()
        .arg("compile")
        .arg(directory.join("entry.slug"))
        .arg(&artifact)
        .output()
        .expect("compile report artifact");
    assert!(compiled.status.success());
    let report = report(&artifact);
    fs::remove_dir_all(&directory).expect("remove report directory");

    assert_eq!(report["entry"], artifact.to_string_lossy().as_ref());
    assert_eq!(report["closed"], true);
    let native = &report["modules"][1];
    assert!(report["modules"][0].get("dynamic_imports").is_none());
    assert_eq!(
        native["foreign"][0]["arity"],
        serde_json::json!({ "min": 1, "max": 2 })
    );
    assert!(native["foreign"][0].get("parameters").is_none());
    assert_eq!(native["resources"][0]["name"], "Handle");
}

#[test]
fn report_requires_exactly_one_path() {
    for arguments in [&["report"][..], &["report", "a.slug", "b.slug"]] {
        let output = slug().args(arguments).output().expect("run report");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("report requires exactly one .slug or .cslug path")
        );
    }
}
