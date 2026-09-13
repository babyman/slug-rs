#[cfg(feature = "concurrency")]
use std::sync::{Arc, Mutex};
use std::{
    cell::RefCell,
    io::Write,
    process::{Command, Stdio},
    rc::Rc,
};

#[cfg(feature = "concurrency")]
use slug_vm::NativeChannelProducer;
use slug_vm::{ModuleLoader, Vm};
use slug_vm::{
    NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus, RuntimeErrorKind,
    compile,
    interactive::{Diagnostic, DiagnosticCategory, EventOrigin, OutputError, OutputStream, Server},
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
fn configured_server_resolves_and_typechecks_builtin_imports() {
    let loader = ModuleLoader::new(".", Some("lib".into()));
    let mut server = Server::new(Vm::with_module_loader(loader));
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
    let session = open_session(&mut server);

    let imported = submit(
        &mut server,
        3,
        &session,
        "val builtin = import(\"slug.builtin\")\nbuiltin.len(\"hello\")",
    );
    assert!(imported.ok, "{imported:?}");
    assert_eq!(imported.result, Some(serde_json::json!({ "value": 5 })));

    let missing = submit(&mut server, 4, &session, "import(\"missing.module\")");
    assert!(!missing.ok);
    assert_eq!(
        missing.error.expect("runtime diagnostic").kind.as_deref(),
        Some("module")
    );
}

#[test]
#[cfg(feature = "concurrency")]
fn imported_channel_calls_suspend_and_resume_in_an_interactive_task() {
    let loader = ModuleLoader::new(".", Some("lib".into()));
    let mut server = Server::new(Vm::with_module_loader(loader));
    assert!(
        server
            .handle_line(
                &request(
                    1,
                    "initialize",
                    None,
                    Some(serde_json::json!({ "protocol": 1 }))
                )
                .to_string()
            )
            .ok
    );
    let session = open_session(&mut server);

    let waiting = submit(
        &mut server,
        3,
        &session,
        "val {*} = import('slug.channel')\nvar msg = chan(8)\nrecv(msg) /> fn(value) { println('received', value) }",
    );
    assert_eq!(
        waiting.result,
        Some(serde_json::json!({ "state": "stalled" }))
    );

    let sent = submit(&mut server, 4, &session, "send(msg, 'hello')");
    assert!(sent.ok, "{sent:?}");
    assert_eq!(
        server.take_events()[0].data,
        serde_json::json!("received hello\n")
    );
}

#[test]
#[cfg(not(feature = "concurrency"))]
fn slim_server_currently_rejects_a_sender_while_a_cell_is_stalled() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    let waiting = submit(
        &mut server,
        3,
        &session,
        "var msg = chan(8)\nselect { recv msg }",
    );
    assert_eq!(
        waiting.result,
        Some(serde_json::json!({ "state": "stalled" }))
    );

    let rejected = submit(&mut server, 4, &session, "select { send msg, 2 }");
    assert_eq!(
        rejected.error.expect("active submission diagnostic").code,
        "submission_active"
    );
}

#[test]
fn launched_program_output_has_a_root_origin() {
    let loader = ModuleLoader::new(".", Some("lib".into()));
    let mut server = Server::new(Vm::with_module_loader(loader.clone()));
    let program = loader
        .compile_source("<root>", "println('worker started')")
        .expect("compile root program");

    server.run_root_program(&program).expect("run root program");
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].origin, EventOrigin::Root);
    assert_eq!(events[0].event, "stdout");
    assert_eq!(events[0].data, serde_json::json!("worker started\n"));
}

#[test]
fn session_accumulates_incomplete_source_without_affecting_other_sessions() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);

    let opening = submit(&mut server, 3, &first, "val add = fn(a, b) {");
    assert_eq!(
        opening.result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    assert!(opening.ok);

    let independent = submit(&mut server, 4, &second, "1 + 2");
    assert_eq!(independent.result, Some(serde_json::json!({ "value": 3 })));

    let body = submit(&mut server, 5, &first, "a + b");
    assert_eq!(
        body.result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    let complete = submit(&mut server, 6, &first, "}");
    assert_eq!(complete.result, Some(serde_json::json!({ "value": null })));

    let called = submit(&mut server, 7, &first, "add(2, 3)");
    assert_eq!(called.result, Some(serde_json::json!({ "value": 5 })));
}

#[test]
fn invalid_and_semantic_submissions_clear_pending_source() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    let invalid = submit(&mut server, 3, &session, "val =");
    assert!(!invalid.ok);
    assert_eq!(
        invalid.error.expect("parse diagnostic").kind.as_deref(),
        Some("parse")
    );

    let reusable = submit(&mut server, 4, &session, "1 + 2");
    assert_eq!(reusable.result, Some(serde_json::json!({ "value": 3 })));

    let semantic = submit(&mut server, 5, &session, "1 + true");
    assert!(!semantic.ok);
    assert_eq!(
        semantic.error.expect("semantic diagnostic").kind.as_deref(),
        Some("semantic")
    );
    let reusable = submit(&mut server, 6, &session, "3 + 4");
    assert_eq!(reusable.result, Some(serde_json::json!({ "value": 7 })));
}

#[test]
fn session_completes_nested_blocks_and_delimited_source_incrementally() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    for (id, source) in [(3, "val nested = fn() {"), (4, "{ { 42 } }")] {
        assert_eq!(
            submit(&mut server, id, &session, source).result,
            Some(serde_json::json!({ "state": "incomplete" }))
        );
    }
    assert_eq!(
        submit(&mut server, 5, &session, "}").result,
        Some(serde_json::json!({ "value": null }))
    );
    assert_eq!(
        submit(&mut server, 6, &session, "nested()").result,
        Some(serde_json::json!({ "value": 42 }))
    );

    assert_eq!(
        submit(&mut server, 7, &session, "val values = [").result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    assert_eq!(
        submit(&mut server, 8, &session, "1]").result,
        Some(serde_json::json!({ "value": null }))
    );
}

#[test]
fn session_retains_incomplete_strings_but_clears_complete_semantic_failures() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    assert_eq!(
        submit(&mut server, 3, &session, "val text = \"hello").result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    assert_eq!(
        submit(&mut server, 4, &session, "world\"").result,
        Some(serde_json::json!({ "value": null }))
    );

    assert_eq!(
        submit(&mut server, 5, &session, "val invalid = fn() {").result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    assert_eq!(
        submit(&mut server, 6, &session, "1 + true").result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );
    let failed = submit(&mut server, 7, &session, "}");
    assert!(!failed.ok);
    assert_eq!(
        failed.error.expect("semantic diagnostic").kind.as_deref(),
        Some("semantic")
    );
    assert_eq!(
        submit(&mut server, 8, &session, "1 + 2").result,
        Some(serde_json::json!({ "value": 3 }))
    );
}

#[test]
fn closing_a_session_discards_its_pending_source() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    assert_eq!(
        submit(&mut server, 3, &session, "val pending = fn() {").result,
        Some(serde_json::json!({ "state": "incomplete" }))
    );

    assert!(
        server
            .handle_line(&request(4, "session.close", Some(&session), None).to_string())
            .ok
    );
    let replacement = open_session(&mut server);
    assert_eq!(
        submit(&mut server, 5, &replacement, "1 + 2").result,
        Some(serde_json::json!({ "value": 3 }))
    );
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
    assert_eq!(
        events[0].origin,
        EventOrigin::Session {
            session: session.clone()
        }
    );
    assert_eq!(events[0].event, "stdout");
    assert_eq!(events[0].data, serde_json::json!("hello"));
    assert_eq!(events[1].data, serde_json::json!(" world\n"));
}

#[test]
fn sessions_emit_output_with_their_own_explicit_attribution() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);

    assert!(submit(&mut server, 3, &first, "println('first')").ok);
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].origin, EventOrigin::Session { session: first });
    assert_eq!(events[0].data, serde_json::json!("first\n"));

    assert!(submit(&mut server, 4, &second, "println('second')").ok);
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].origin, EventOrigin::Session { session: second });
    assert_eq!(events[0].data, serde_json::json!("second\n"));
}

#[test]
fn host_and_background_output_are_explicitly_attributed_to_live_sessions() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    server
        .emit_output(&session, OutputStream::Stderr, "warning\\n")
        .expect("emit stderr event");
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].origin,
        EventOrigin::Session {
            session: session.clone()
        }
    );
    assert_eq!(events[0].event, "stderr");
    assert_eq!(events[0].data, serde_json::json!("warning\\n"));

    assert!(
        server
            .handle_line(&request(3, "session.close", Some(&session), None).to_string())
            .ok
    );
    assert_eq!(
        server
            .emit_output(&session, OutputStream::Stdout, "late")
            .expect_err("closed sessions cannot receive output"),
        OutputError::UnknownSession(session)
    );
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
fn sessions_retain_independent_bindings_and_closures() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);

    assert!(submit(&mut server, 3, &first, "var x = 10").ok);
    assert!(submit(&mut server, 4, &first, "val read = fn() { x }").ok);
    assert!(submit(&mut server, 5, &second, "var x = 20").ok);
    assert!(submit(&mut server, 6, &second, "val read = fn() { x }").ok);
    assert!(submit(&mut server, 7, &first, "x = 11").ok);

    let first_value = submit(&mut server, 8, &first, "read()");
    let second_value = submit(&mut server, 9, &second, "read()");

    assert_eq!(first_value.result, Some(serde_json::json!({ "value": 11 })));
    assert_eq!(
        second_value.result,
        Some(serde_json::json!({ "value": 20 }))
    );
}

#[test]
fn closing_a_session_preserves_the_other_session() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);
    assert!(submit(&mut server, 3, &second, "val x = 20").ok);

    let closed = server.handle_line(&request(4, "session.close", Some(&first), None).to_string());
    assert!(closed.ok);
    let rejected = submit(&mut server, 5, &first, "1");
    assert_eq!(
        rejected.error.expect("closed session diagnostic").code,
        "unknown_session"
    );
    let surviving = submit(&mut server, 6, &second, "x");
    assert_eq!(surviving.result, Some(serde_json::json!({ "value": 20 })));
}

#[test]
fn sessions_communicate_through_an_explicitly_shared_host_channel() {
    let mut server = initialized_server();
    let first = open_session(&mut server);
    let second = open_session(&mut server);
    let module = NativeModule::new(
        "test.interactive_shared_channel",
        Rc::new(RefCell::new(Option::<NativeOwnedValue>::None)),
    )
    .expect("native module");
    let channel = module
        .function("shared_queue", NativeArity::Exact(0), shared_queue)
        .expect("native function");
    server
        .define_host_native(channel)
        .expect("register shared channel factory");

    assert!(submit(&mut server, 3, &first, "val queue = shared_queue()").ok);
    assert!(
        submit(
            &mut server,
            4,
            &first,
            "select { send queue, 99 /> fn(_) { nil } }"
        )
        .ok
    );
    assert!(submit(&mut server, 5, &second, "val queue = shared_queue()").ok);
    let received = submit(&mut server, 6, &second, "select { recv queue }");

    assert_eq!(received.result, Some(serde_json::json!({ "value": 99 })));
}

#[test]
#[cfg(feature = "concurrency")]
fn stalled_form_allows_a_later_binding_free_submission_to_resume_it() {
    let mut server = initialized_server();
    let stalled = open_session(&mut server);
    let runnable = open_session(&mut server);
    let state = Arc::new(ProducerState(Mutex::new(None)));
    let module = NativeModule::new("test.interactive_ingress", state.clone()).expect("module");
    let input = module
        .function("session_input", NativeArity::Exact(0), session_input)
        .expect("native function");
    server
        .define_host_native(input)
        .expect("register input factory");

    let pending = submit(
        &mut server,
        3,
        &stalled,
        "val inbox = session_input()\nselect { recv inbox /> fn(value) { println('resumed', value) } }",
    );
    assert_eq!(
        pending.result,
        Some(serde_json::json!({ "state": "stalled" }))
    );
    let rejected = submit(&mut server, 4, &stalled, "val later = 1");
    assert_eq!(
        rejected.error.expect("background binding diagnostic").code,
        "background_bindings"
    );
    let sent = submit(&mut server, 5, &stalled, "select { send inbox, 99 }");
    assert_eq!(sent.result, Some(serde_json::json!({ "value": null })));
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].origin,
        EventOrigin::Session {
            session: stalled.clone()
        }
    );
    assert_eq!(events[0].data, serde_json::json!("resumed 99\n"));

    let completed = submit(&mut server, 6, &runnable, "6 * 7");
    assert_eq!(completed.result, Some(serde_json::json!({ "value": 42 })));

    let resumed = server.handle_line(&request(7, "session.poll", Some(&stalled), None).to_string());
    assert_eq!(resumed.result, Some(serde_json::json!({ "state": "idle" })));
}

#[test]
#[cfg(feature = "concurrency")]
fn submitted_tasks_supervise_spawned_children() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    let response = submit(&mut server, 3, &session, "spawn { println('child ran') }");

    assert!(response.ok, "{response:?}");
    let events = server.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].origin, EventOrigin::Session { session });
    assert_eq!(events[0].event, "stdout");
    assert_eq!(events[0].data, serde_json::json!("child ran\n"));
}

#[test]
#[cfg(feature = "concurrency")]
fn pending_cell_bindings_are_not_visible_before_settlement() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    let state = Arc::new(ProducerState(Mutex::new(None)));
    let module = NativeModule::new("test.interactive_overlay", state.clone()).expect("module");
    let input = module
        .function("session_input", NativeArity::Exact(0), session_input)
        .expect("native function");
    server
        .define_host_native(input)
        .expect("register input factory");

    let pending = submit(
        &mut server,
        3,
        &session,
        "val inbox = session_input()\nvar value = select { recv inbox }",
    );
    assert_eq!(
        pending.result,
        Some(serde_json::json!({ "state": "stalled" }))
    );

    let unavailable = submit(&mut server, 4, &session, "value");
    assert_eq!(
        unavailable
            .error
            .expect("uncommitted binding diagnostic")
            .kind
            .as_deref(),
        Some("name")
    );

    let producer = state
        .0
        .lock()
        .expect("producer state")
        .clone()
        .expect("session input producer");
    assert_eq!(
        producer.try_send(slug_vm::NativeSendValue::integer(42)),
        slug_vm::NativeProducerStatus::Sent
    );
    let settled = server.handle_line(&request(5, "session.poll", Some(&session), None).to_string());
    assert_eq!(settled.result, Some(serde_json::json!({ "state": "idle" })));

    let value = submit(&mut server, 6, &session, "value");
    assert_eq!(value.result, Some(serde_json::json!({ "value": 42 })));
}

#[test]
#[cfg(feature = "concurrency")]
fn poll_delivers_a_background_task_failure() {
    let mut server = initialized_server();
    let session = open_session(&mut server);
    let state = Arc::new(ProducerState(Mutex::new(None)));
    let module = NativeModule::new("test.interactive_failure", state.clone()).expect("module");
    let input = module
        .function("session_input", NativeArity::Exact(0), session_input)
        .expect("native function");
    server
        .define_host_native(input)
        .expect("register input factory");

    let pending = submit(
        &mut server,
        3,
        &session,
        "val inbox = session_input()\nselect { recv inbox /> fn(_) { throw 42 } }",
    );
    assert_eq!(
        pending.result,
        Some(serde_json::json!({ "state": "stalled" }))
    );
    let producer = state
        .0
        .lock()
        .expect("producer state")
        .clone()
        .expect("session input producer");
    assert_eq!(
        producer.try_send(slug_vm::NativeSendValue::integer(1)),
        slug_vm::NativeProducerStatus::Sent
    );

    let failure = server.handle_line(&request(4, "session.poll", Some(&session), None).to_string());
    assert!(!failure.ok);
    assert_eq!(
        failure.error.expect("background failure").kind.as_deref(),
        Some("thrown")
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
fn submitted_diagnostics_retain_source_kinds_and_locations() {
    let mut server = initialized_server();
    let session = open_session(&mut server);

    let parse = submit(&mut server, 3, &session, "val =");
    let parse = parse.error.expect("parse diagnostic");
    assert_eq!(parse.category, DiagnosticCategory::Source);
    assert_eq!(parse.kind.as_deref(), Some("parse"));
    assert_eq!(
        parse.location.expect("parse location").path,
        "<interactive:s1>"
    );

    let semantic = submit(&mut server, 4, &session, "1 + true");
    let semantic = semantic.error.expect("semantic diagnostic");
    assert_eq!(semantic.category, DiagnosticCategory::Source);
    assert_eq!(semantic.kind.as_deref(), Some("semantic"));
    assert_eq!(
        semantic.location.expect("semantic location").path,
        "<interactive:s1>"
    );

    let runtime = submit(&mut server, 5, &session, "1 / 0");
    let runtime = runtime.error.expect("runtime diagnostic");
    assert_eq!(runtime.category, DiagnosticCategory::Runtime);
    assert_eq!(runtime.kind.as_deref(), Some("divide_by_zero"));
    assert_eq!(
        runtime.location.expect("runtime location").path,
        "<interactive:s1>"
    );
    assert!(!runtime.frames.is_empty());
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
fn server_binary_returns_structured_slug_diagnostics_on_protocol_stdout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-server");
    let input = concat!(
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocol\":1}}\n",
        "{\"id\":2,\"method\":\"session.open\"}\n",
        "{\"id\":3,\"session\":\"s1\",\"method\":\"submit\",\"params\":{\"source\":\"1 + true\"}}\n",
        "{\"id\":4,\"session\":\"s1\",\"method\":\"session.close\"}\n"
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
    assert_eq!(messages[2]["id"], 3);
    assert_eq!(messages[2]["ok"], false);
    assert_eq!(messages[2]["error"]["category"], "source");
    assert_eq!(messages[2]["error"]["kind"], "semantic");
    assert_eq!(messages[3]["result"], serde_json::Value::Null);
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
        serde_json::json!({ "source": "session", "session": "s1", "event": "stdout", "data": "hello\n" })
    );
    assert_eq!(messages[3]["result"], serde_json::json!({ "value": null }));
}

#[test]
fn server_binary_orders_output_before_a_failing_submission_response() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_slug-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start slug-server");
    let input = concat!(
        "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocol\":1}}\n",
        "{\"id\":2,\"method\":\"session.open\"}\n",
        "{\"id\":3,\"session\":\"s1\",\"method\":\"submit\",\"params\":{\"source\":\"println('before failure')\\nthrow 1\"}}\n"
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
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON message"))
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[2],
        serde_json::json!({ "source": "session", "session": "s1", "event": "stdout", "data": "before failure\n" })
    );
    assert_eq!(messages[3]["id"], 3);
    assert_eq!(messages[3]["ok"], false);
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

fn shared_queue(call: &mut NativeCall<'_>) -> NativeStatus {
    let Some(state) = call
        .state::<Rc<RefCell<Option<NativeOwnedValue>>>>()
        .cloned()
    else {
        return call.report_contract_violation("shared queue state is unavailable");
    };
    let mut queue = state.borrow_mut();
    if let Some(existing) = queue.as_ref() {
        return call.return_value(existing.clone());
    }
    let channel = call.plain_channel(1);
    queue.replace(channel.clone());
    call.return_value(channel)
}

#[cfg(feature = "concurrency")]
struct ProducerState(Mutex<Option<NativeChannelProducer>>);

#[cfg(feature = "concurrency")]
fn session_input(call: &mut NativeCall<'_>) -> NativeStatus {
    let (channel, producer) = call.channel(1);
    let Some(state) = call.state::<Arc<ProducerState>>() else {
        return call.report_contract_violation("input producer state is unavailable");
    };
    state
        .0
        .lock()
        .expect("producer state lock")
        .replace(producer);
    call.return_value(channel)
}
