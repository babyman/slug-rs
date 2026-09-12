use std::{
    io::Write,
    process::{Command, Stdio},
};

use slug_vm::{
    RuntimeErrorKind, compile,
    interactive::{Diagnostic, DiagnosticCategory, Server},
};

fn request(
    id: u64,
    method: &str,
    session: Option<&str>,
    params: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut request = serde_json::json!({ "id": id, "method": method });
    if let Some(session) = session {
        request["session"] = serde_json::json!(session);
    }
    if let Some(params) = params {
        request["params"] = params;
    }
    request
}

#[test]
fn engine_initializes_opens_and_closes_a_session() {
    let mut server = Server::default();
    assert!(server.vm().global("cfg").is_some());

    let initialize = server.handle_line(
        &request(
            1,
            "initialize",
            None,
            Some(serde_json::json!({ "protocol": 1 })),
        )
        .to_string(),
    );
    assert!(initialize.ok);
    assert_eq!(
        initialize.result,
        Some(serde_json::json!({ "protocol": 1, "capabilities": { "sessions": true } }))
    );

    let opened = server.handle_line(&request(2, "session.open", None, None).to_string());
    assert!(opened.ok);
    assert_eq!(opened.result, Some(serde_json::json!({ "session": "s1" })));

    let closed = server.handle_line(&request(3, "session.close", Some("s1"), None).to_string());
    assert!(closed.ok);
    assert_eq!(closed.session.as_deref(), Some("s1"));
    assert_eq!(closed.result, Some(serde_json::Value::Null));
}

#[test]
fn engine_reports_protocol_failures_and_remains_usable() {
    let mut server = Server::default();
    let malformed = server.handle_line("{");
    assert!(!malformed.ok);
    assert_eq!(malformed.id, None);
    assert_eq!(malformed.error.expect("diagnostic").code, "malformed_json");

    let before_initialize = server.handle_line(&request(1, "session.open", None, None).to_string());
    assert_eq!(
        before_initialize.error.expect("diagnostic").code,
        "not_initialized"
    );

    let unknown_method = server.handle_line(&request(2, "unknown", None, None).to_string());
    assert_eq!(
        unknown_method.error.expect("diagnostic").code,
        "unknown_method"
    );

    let initialized = server.handle_line(
        &request(
            3,
            "initialize",
            None,
            Some(serde_json::json!({ "protocol": 1 })),
        )
        .to_string(),
    );
    assert!(initialized.ok);
    let unknown_session =
        server.handle_line(&request(4, "session.close", Some("s99"), None).to_string());
    assert_eq!(
        unknown_session.error.expect("diagnostic").code,
        "unknown_session"
    );
}

#[test]
fn diagnostic_projection_preserves_source_and_runtime_structure() {
    let source = compile("interactive-source.slug", "val = 1").expect_err("invalid source");
    let source = Diagnostic::from_source(&source);
    assert_eq!(source.category, DiagnosticCategory::Source);
    assert_eq!(source.kind.as_deref(), Some("parse"));
    assert_eq!(
        source.location.expect("location").path,
        "interactive-source.slug"
    );

    let program = compile("interactive-runtime.slug", "throw 42").expect("valid source");
    let runtime = slug_vm::Vm::new()
        .run_named(&program, "main")
        .expect_err("throw fails");
    assert_eq!(runtime.kind, RuntimeErrorKind::Thrown);
    let runtime = Diagnostic::from_runtime(&runtime);
    assert_eq!(runtime.category, DiagnosticCategory::Runtime);
    assert_eq!(runtime.kind.as_deref(), Some("thrown"));
    assert_eq!(runtime.thrown.expect("thrown value").display, "42");

    let host = Diagnostic::host("io_failure", "cannot write output");
    assert_eq!(host.category, DiagnosticCategory::Host);
    assert_eq!(
        serde_json::to_value(host).expect("host diagnostic is serializable")["code"],
        "io_failure"
    );
}

#[test]
fn server_binary_keeps_ndjson_on_stdout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-server");
    let input = concat!(
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocol\":1}}\n",
        "{\"id\":2,\"method\":\"session.open\"}\n",
        "{\"id\":3,\"session\":\"s1\",\"method\":\"session.close\"}\n"
    );
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write input");
    let output = child.wait_with_output().expect("wait for server");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let lines = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let messages = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON response"))
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["result"]["protocol"], 1);
    assert_eq!(messages[1]["result"]["session"], "s1");
    assert_eq!(messages[2]["result"], serde_json::Value::Null);
}
