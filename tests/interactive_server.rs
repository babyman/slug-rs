use std::{
    io::Write,
    process::{Command, Stdio},
};

use slug_vm::{
    NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus, RuntimeErrorKind,
    compile,
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
fn session_persists_compiler_and_runtime_bindings_across_submissions() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    let defined = submit(&mut server, 3, &session, "val x = 10");
    assert!(defined.ok);
    assert_eq!(defined.result, Some(serde_json::json!({ "value": null })));

    let evaluated = submit(&mut server, 4, &session, "x + 5");
    assert!(evaluated.ok);
    assert_eq!(evaluated.result, Some(serde_json::json!({ "value": 15 })));

    let function = submit(
        &mut server,
        5,
        &session,
        "val add = fn(value) { x + value }",
    );
    assert!(function.ok);
    let called = submit(&mut server, 6, &session, "add(7)");
    assert!(called.ok);
    assert_eq!(called.result, Some(serde_json::json!({ "value": 17 })));
}

#[test]
fn compile_failures_do_not_commit_session_state() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    assert!(submit(&mut server, 3, &session, "val x = 10").ok);

    let failed = submit(&mut server, 4, &session, "val broken = 1 + true");
    assert!(!failed.ok);
    assert_eq!(
        failed.error.expect("source diagnostic").category,
        DiagnosticCategory::Source
    );

    let preserved = submit(&mut server, 5, &session, "x");
    assert!(preserved.ok);
    assert_eq!(preserved.result, Some(serde_json::json!({ "value": 10 })));
    let absent = submit(&mut server, 6, &session, "broken");
    assert!(!absent.ok);
    assert_eq!(
        absent.error.expect("runtime diagnostic").kind.as_deref(),
        Some("name")
    );
}

#[test]
fn closures_observe_later_mutations_in_their_session_environment() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    assert!(submit(&mut server, 3, &session, "var count = 1").ok);
    assert!(
        submit(
            &mut server,
            4,
            &session,
            "val increment = fn() { count = count + 1 }"
        )
        .ok
    );
    assert!(submit(&mut server, 5, &session, "increment()").ok);
    let count = submit(&mut server, 6, &session, "count");
    assert_eq!(count.result, Some(serde_json::json!({ "value": 2 })));
}

#[test]
fn output_is_captured_as_a_session_event() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    let response = submit(
        &mut server,
        3,
        &session,
        "print(\"hello\")\nprintln(\" world\")",
    );

    assert!(response.ok);
    let events = server.take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].session, session);
    assert_eq!(events[0].event, "stdout");
    assert_eq!(events[0].data, serde_json::json!("hello"));
    assert_eq!(events[1].data, serde_json::json!(" world\n"));
}

#[test]
fn sessions_observe_host_bindings_registered_after_they_open() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    let module = NativeModule::new("test.interactive_host", ()).expect("native module");
    let answer = module
        .function("host_answer", NativeArity::Exact(0), host_answer)
        .expect("native function");
    server
        .define_host_native(answer)
        .expect("register shared host binding");

    let response = submit(&mut server, 3, &session, "host_answer()");
    assert!(response.ok);
    assert_eq!(response.result, Some(serde_json::json!({ "value": 42 })));
}

#[test]
fn session_bindings_shadow_host_bindings_without_leaking_to_other_sessions() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);
    let module = NativeModule::new("test.interactive_host", ()).expect("native module");
    let answer = module
        .function("shared_name", NativeArity::Exact(0), host_answer)
        .expect("native function");
    server
        .define_host_native(answer)
        .expect("register shared host binding");

    assert!(submit(&mut server, 3, &first, "val shared_name = 10").ok);
    let first_value = submit(&mut server, 4, &first, "shared_name");
    let second_value = submit(&mut server, 5, &second, "shared_name()");

    assert_eq!(first_value.result, Some(serde_json::json!({ "value": 10 })));
    assert_eq!(
        second_value.result,
        Some(serde_json::json!({ "value": 42 }))
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

#[test]
fn server_binary_preserves_a_binding_between_ndjson_submissions() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-server");
    let input = concat!(
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocol\":1}}\n",
        "{\"id\":2,\"method\":\"session.open\"}\n",
        "{\"id\":3,\"session\":\"s1\",\"method\":\"submit\",\"params\":{\"source\":\"val x = 10\"}}\n",
        "{\"id\":4,\"session\":\"s1\",\"method\":\"submit\",\"params\":{\"source\":\"x + 5\"}}\n"
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
    let messages = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON response"))
        .collect::<Vec<_>>();
    assert_eq!(messages[2]["result"], serde_json::json!({ "value": null }));
    assert_eq!(messages[3]["result"], serde_json::json!({ "value": 15 }));
}

#[test]
fn server_binary_emits_program_output_only_as_ndjson_events() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-server");
    let input = concat!(
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocol\":1}}\n",
        "{\"id\":2,\"method\":\"session.open\"}\n",
        "{\"id\":3,\"session\":\"s1\",\"method\":\"submit\",\"params\":{\"source\":\"println('hello')\"}}\n"
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
    let messages = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON response"))
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[2],
        serde_json::json!({ "session": "s1", "event": "stdout", "data": "hello\n" })
    );
    assert_eq!(messages[3]["result"], serde_json::json!({ "value": null }));
}

fn initialized_server() -> Server {
    let mut server = Server::default();
    let response = server.handle_line(
        &request(
            1,
            "initialize",
            None,
            Some(serde_json::json!({ "protocol": 1 })),
        )
        .to_string(),
    );
    assert!(response.ok);
    server
}

fn open_session(server: &mut Server) -> String {
    let response = server.handle_line(&request(2, "session.open", None, None).to_string());
    assert!(response.ok);
    response.result.expect("session result")["session"]
        .as_str()
        .expect("session identifier")
        .into()
}

fn submit(
    server: &mut Server,
    id: u64,
    session: &str,
    source: &str,
) -> slug_vm::interactive::Response {
    server.handle_line(
        &request(
            id,
            "submit",
            Some(session),
            Some(serde_json::json!({ "source": source })),
        )
        .to_string(),
    )
}

fn host_answer(call: &mut NativeCall<'_>) -> NativeStatus {
    call.return_value(NativeOwnedValue::integer(42))
}
