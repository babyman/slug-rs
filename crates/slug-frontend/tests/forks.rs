//! What the compiler checks about `fork` and `task var`. The behavior of
//! running programs is specified by `tests/conformance/fork`.

use slug_frontend::{SourceErrorCode, compile};

fn accepts(source: &str) {
    if let Err(error) = compile("forks.slug", source) {
        panic!("{source}\nrejected: {}", error.message);
    }
}

fn rejects(source: &str, code: SourceErrorCode, fragment: &str) {
    let error = compile("forks.slug", source).expect_err(source);
    assert_eq!(error.code, code, "{source}: {}", error.message);
    assert!(
        error.message.contains(fragment),
        "{source}: {}",
        error.message
    );
}

#[test]
fn a_fork_may_write_its_own_bindings_and_task_vars() {
    accepts(
        "val main = fn() {\n    fork {\n        var i = 0\n        i = i + 1\n        val inc = fn() { i = i + 1 }\n        inc()\n        i\n    }\n}\n",
    );
    accepts("task var who = \"main\"\nval main = fn() {\n    fork { who = \"fork\" }\n}\n");
    // A fork's own binding that shadows an outer one is its own.
    accepts(
        "val main = fn() {\n    var n = 0\n    fork {\n        var n = 1\n        n = 2\n    }\n    n = 3\n}\n",
    );
}

#[test]
fn a_fork_may_read_what_it_cannot_write() {
    accepts("var total = 0\nval main = fn() {\n    var n = 1\n    fork { n + total }\n}\n");
}

#[test]
fn spawn_keeps_sharing_live_bindings() {
    accepts(
        "var total = 0\nval main = fn() {\n    var n = 0\n    spawn {\n        n = n + 1\n        total = n\n    }\n}\n",
    );
}

#[test]
fn a_write_inside_a_fork_to_an_outer_binding_is_reported() {
    // Through a closure the fork defines.
    rejects(
        "val main = fn() {\n    var n = 0\n    fork {\n        val bump = fn() { n = n + 1 }\n        bump()\n    }\n}\n",
        SourceErrorCode::ForkWrite,
        "a fork cannot change `n`",
    );
    // From an inner fork to the outer fork's own binding.
    rejects(
        "val main = fn() {\n    fork {\n        var a = 0\n        val inner = fork { a = 1 }\n        a\n    }\n}\n",
        SourceErrorCode::ForkWrite,
        "a fork cannot change `a`",
    );
    // From a task spawned inside the fork.
    rejects(
        "var total = 0\nval main = fn() {\n    fork {\n        val inner = spawn { total = 1 }\n        0\n    }\n}\n",
        SourceErrorCode::ForkWrite,
        "declare it `task var total`",
    );
}

#[test]
fn a_fork_in_module_top_level_code_is_reported() {
    for source in [
        "{ fork { 1 } }\n",
        "spawn { fork { 1 } }\n",
        "val values = [fork { 1 }]\n",
    ] {
        rejects(
            source,
            SourceErrorCode::ForkDuringInitialization,
            "while a module initializes",
        );
    }
    // A function declared at top level forks when it is called.
    accepts("val start = fn() { fork { 1 } }\n");
}

#[test]
fn task_is_a_modifier_only_before_var() {
    accepts("val task = 1\ntask + 1\n");
    accepts("val task = 1\ntask\nvar x = 2\n");
    accepts("/**\n * Who is acting.\n */\nexport task var acting:str|nil = nil\n");
    rejects(
        "task val x = 1\n",
        SourceErrorCode::ExpectedSyntax,
        "`task val`",
    );
    rejects(
        "val main = fn() {\n    task var x = 1\n}\n",
        SourceErrorCode::TopLevelOnly,
        "module top level",
    );
}

#[test]
fn a_task_var_is_recorded_in_module_metadata() {
    let program = compile("forks.slug", "task var who = \"main\"\nvar count = 0\n").unwrap();
    let flags = program
        .declarations()
        .iter()
        .map(|declaration| {
            (
                declaration.bindings[0].as_str(),
                declaration.mutable,
                declaration.task,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(flags, [("who", true, true), ("count", true, false)]);
}

#[test]
fn a_fork_returning_a_task_handle_is_reported() {
    rejects(
        "val main = fn() {\n    fork { spawn { 1 } }\n}\n",
        SourceErrorCode::UnsendableValue,
        "is not sendable",
    );
    // A list of handles may be empty, so it is checked when it runs.
    accepts("val main = fn() {\n    fork { [spawn { 1 }] }\n}\n");
}

#[test]
fn a_fork_typed_at_the_prompt_is_not_forking_during_initialization() {
    let state = slug_frontend::InteractiveCompilerState::default();
    slug_frontend::compile_interactive_forms("repl.slug", "fork { 1 }\n", &state)
        .expect("a fork at the prompt compiles");
    // The write rule still applies.
    let Err(error) = slug_frontend::compile_interactive_forms(
        "repl.slug",
        "{\n    var n = 0\n    fork { n = 1 }\n}\n",
        &state,
    ) else {
        panic!("a write to an outer binding compiled");
    };
    assert_eq!(error.code, SourceErrorCode::ForkWrite, "{}", error.message);
}
