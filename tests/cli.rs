use std::{fs, process::Command};

fn slug() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slug"));
    command.env("SLUG_HOME", env!("CARGO_MANIFEST_DIR"));
    command
}

fn channel_source(source: &str) -> String {
    format!(
        "val {{ await, chan, close, recv, send }} = import(\"slug.channel\")\n{}",
        source.replace("channel(", "chan(")
    )
}

fn fixture_path(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-cli-{kind}-{}.slug", std::process::id()))
}

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

#[test]
fn executes_spawned_tasks_and_explicit_nurseries() {
    let path = fixture_path("tasks-and-nurseries");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nval task = spawn { 20 + 22 }\nprintln(await(task))\nprintln(nursery limit 1 { 7 })\n",
    )
    .expect("write task source");
    let output = slug().arg(&path).output().expect("run task source");
    fs::remove_file(path).expect("remove task source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n7\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn repeated_awaits_return_one_cached_task_completion() {
    let path = fixture_path("repeated-await");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nval task = spawn { println(\"ran\"); 42 }\nprintln(await(task))\nprintln(await(task))\n",
    )
    .expect("write repeated await source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run repeated await source");
    fs::remove_file(path).expect("remove repeated await source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "ran\n42\n42\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn channels_rendezvous_between_cooperatively_scheduled_tasks() {
    let path = fixture_path("channel-rendezvous");
    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nval sender = spawn { send(inbox, 42) }\nval receiver = spawn { recv(inbox) }\nprintln(await(receiver))\nawait(sender)\n"),
    )
    .expect("write channel source");
    let output = slug().arg(&path).output().expect("run channel source");
    fs::remove_file(path).expect("remove channel source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn root_evaluation_suspends_for_channel_messages_and_task_completion() {
    let path = fixture_path("channel-root-suspension");
    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nspawn { send(inbox, 42) }\nprintln(recv(inbox))\nval child = spawn { 6 * 7 }\nprintln(await(child))\n"),
    )
    .expect("write root-suspension source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run root-suspension source");
    fs::remove_file(path).expect("remove root-suspension source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n42\n");
}

#[test]
fn select_handles_ready_blocked_and_timer_cases_without_stale_waiters() {
    let path = fixture_path("select-cases");
    let source = channel_source(
        "val inbox = channel(1)\n\
         send(inbox, 7)\n\
         println(select {\n\
           recv inbox /> fn(value) { value }\n\
           _ /> fn(unused) { 0 }\n\
         })\n\
         println(select {\n\
           send inbox, 8 /> fn(unused) { 1 }\n\
         })\n\
         println(recv(inbox))\n\
         val task = spawn { 9 }\n\
         println(select { await task /> fn(value) { value } })\n\
         println(select { after 1 /> fn(unused) { 10 } })\n\
         val left = channel(0)\n\
         val right = channel(0)\n\
         val sender = spawn { select { send left, 11 }; select { send right, 12 } }\n\
         println(select {\n\
           recv left /> fn(value) { value }\n\
           recv right /> fn(value) { value }\n\
         })\n\
         println(select { recv right })\n\
         await(sender)\n",
    );
    fs::write(&path, source).expect("write select source");
    let output = slug().arg(&path).output().expect("run select source");
    fs::remove_file(path).expect("remove select source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\n1\n8\n9\n10\n11\n12\n"
    );
}

#[test]
fn selected_task_failures_unwind_cleanup_and_support_onerror_recovery() {
    let path = fixture_path("select-task-failure");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer { println(\"cleanup\") }\n\
           val task = spawn { select { after 1 }; throw \"child failure\" }\n\
           select { await task /> fn(value) { println(\"handler\"); value } }\n\
         }\n\
         fail()\n",
    )
    .expect("write selected failing task source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run selected failing task source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "cleanup\n");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("uncaught throw: child failure"));
    assert!(!stderr.contains("panicked"));

    fs::write(
        &path,
        "val recover = fn() {\n\
           defer onerror(err) { println(\"recovered\", err); 42 }\n\
           val task = spawn { select { after 1 }; throw \"child failure\" }\n\
           select { await task /> fn(value) { println(\"handler\"); value } }\n\
         }\n\
         println(recover())\n",
    )
    .expect("write selected recovered task source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run selected recovered task source");
    fs::remove_file(path).expect("remove select task failure source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "recovered child failure\n42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn channels_preserve_buffered_fifo_messages_and_resume_blocked_senders() {
    let path = fixture_path("channel-buffer");
    fs::write(
        &path,
        channel_source("val inbox = channel(1)\nval sender = spawn { send(inbox, 1); send(inbox, 2) }\nval receiver = spawn { println(recv(inbox)); println(recv(inbox)) }\nawait(receiver)\nawait(sender)\n"),
    )
    .expect("write channel source");
    let output = slug().arg(&path).output().expect("run channel source");
    fs::remove_file(path).expect("remove channel source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n");
}

#[test]
fn closing_a_channel_wakes_receivers_and_rejects_sends() {
    let path = fixture_path("channel-close");
    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nval receiver = spawn { recv(inbox) }\nval closer = spawn { close(inbox) }\nprintln(await(receiver))\nawait(closer)\n"),
    )
    .expect("write channel source");
    let output = slug().arg(&path).output().expect("run channel source");
    fs::remove_file(&path).expect("remove channel source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "nil\n");

    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nclose(inbox)\nsend(inbox, 1)\n"),
    )
    .expect("write closed-send source");
    let output = slug().arg(&path).output().expect("run closed-send source");
    fs::remove_file(&path).expect("remove closed-send source");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("send on a closed channel"));

    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nval sender = spawn { defer { println(\"cleaned\") }; send(inbox, 1) }\nval closer = spawn { close(inbox) }\nawait(closer)\nawait(sender)\n"),
    )
    .expect("write blocked-send source");
    let output = slug().arg(&path).output().expect("run blocked-send source");
    fs::remove_file(path).expect("remove blocked-send source");

    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "cleaned\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("send on a closed channel"));
}

#[test]
fn reports_a_checked_error_for_a_channel_task_with_no_possible_progress() {
    let path = fixture_path("channel-blocked");
    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nspawn { recv(inbox) }\n"),
    )
    .expect("write blocked channel source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run blocked channel source");
    fs::remove_file(path).expect("remove blocked channel source");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("task remains blocked with no runnable work")
    );
}

#[test]
fn fail_fast_cancellation_removes_parked_channel_waiters() {
    let path = fixture_path("channel-cancelled-waiter");
    fs::write(
        &path,
        channel_source("val inbox = channel(0)\nval attempt = fn() {\n  defer onerror(err) { nil }\n  nursery {\n    spawn { recv(inbox) }\n    spawn { throw \"fail\" }\n  }\n}\nattempt()\nval sender = spawn { send(inbox, 42) }\nawait(sender)\n"),
    )
    .expect("write cancellation source");
    let output = slug().arg(&path).output().expect("run cancellation source");
    fs::remove_file(path).expect("remove cancellation source");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("task remains blocked with no runnable work")
    );
}

#[test]
fn spawned_tasks_share_root_globals() {
    let path = fixture_path("spawn-shared-globals");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nvar answer = 0\nval task = spawn { answer = 42 }\nawait(task)\nprintln(answer)\n",
    )
    .expect("write shared-global task source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run shared-global task source");
    fs::remove_file(path).expect("remove shared-global task source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "42\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn spawned_tasks_snapshot_immediate_captures_but_keep_outer_captures_live() {
    let path = fixture_path("spawn-capture-boundary");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\n\
         val direct = fn() { var value = 1; val task = spawn { value }; value = 2; await(task) }\n\
         val outer = fn() { var value = 1; val middle = fn() { val task = spawn { value }; value = 2; await(task) }; middle() }\n\
         println(direct())\nprintln(outer())\n",
    )
    .expect("write capture-boundary task source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run capture-boundary task source");
    fs::remove_file(path).expect("remove capture-boundary task source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n2\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn propagates_unawaited_task_failures_when_the_root_settles() {
    let path = fixture_path("unawaited-task-failure");
    fs::write(
        &path,
        "spawn { throw \"child failure\" }\nprintln(\"parent\")\n",
    )
    .expect("write failing task source");
    let output = slug().arg(&path).output().expect("run failing task source");
    fs::remove_file(path).expect("remove failing task source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "parent\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("uncaught throw: child failure")
    );
}

#[test]
fn propagates_unawaited_task_failures_when_a_nursery_settles() {
    let path = fixture_path("unawaited-nursery-task-failure");
    fs::write(
        &path,
        "println(nursery { spawn { throw \"child failure\" }; \"unreachable\" })\n",
    )
    .expect("write failing nursery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run failing nursery source");
    fs::remove_file(path).expect("remove failing nursery source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("uncaught throw: child failure")
    );
}

#[test]
fn rejects_a_zero_nursery_limit() {
    let path = fixture_path("nursery-zero-limit");
    fs::write(&path, "nursery limit 0 { 42 }\n").expect("write zero-limit nursery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run zero-limit nursery source");
    fs::remove_file(path).expect("remove zero-limit nursery source");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("nursery limit must be positive")
    );
}

#[test]
fn nursery_limit_releases_a_permit_after_a_task_settles() {
    let path = fixture_path("nursery-permit-release");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nprintln(nursery limit 1 { val first = spawn { 1 }; await(first); val second = spawn { 2 }; await(second) })\n",
    )
    .expect("write permit-release nursery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run permit-release nursery source");
    fs::remove_file(path).expect("remove permit-release nursery source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "2\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn nursery_limit_queues_direct_tasks_beyond_active_capacity() {
    let path = fixture_path("nursery-pending-limit");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nprintln(nursery limit 1 { val first = spawn { 1 }; val second = spawn { 2 }; await(first) + await(second) })\n",
    )
    .expect("write queued-task nursery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run queued-task nursery source");
    fs::remove_file(path).expect("remove pending-limit nursery source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn awaiting_a_queued_task_preserves_nursery_admission_order() {
    let path = fixture_path("nursery-admission-order");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nnursery limit 1 { val first = spawn { println(\"first\"); 1 }; val second = spawn { println(\"second\"); 2 }; await(second) }\n",
    )
    .expect("write admission-order source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run admission-order source");
    fs::remove_file(path).expect("remove admission-order source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "first\nsecond\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn awaiting_a_later_task_drives_the_ready_queue_in_spawn_order() {
    let path = fixture_path("task-ready-order");
    fs::write(
        &path,
        "val { await } = import(\"slug.channel\")\nval first = spawn { println(\"first\"); 1 }\nval second = spawn { println(\"second\"); 2 }\nawait(second)\n",
    )
    .expect("write ready-order source");
    let output = slug().arg(&path).output().expect("run ready-order source");
    fs::remove_file(path).expect("remove ready-order source");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "first\nsecond\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn explicit_nursery_cancels_pending_siblings_after_a_child_failure() {
    let path = fixture_path("nursery-fail-fast");
    fs::write(
        &path,
        "nursery { spawn { throw \"first failure\" }; spawn { println(\"sibling ran\") } }\n",
    )
    .expect("write fail-fast nursery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run fail-fast nursery source");
    fs::remove_file(path).expect("remove fail-fast nursery source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("uncaught throw: first failure")
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
    fs::remove_file(path).expect("remove failing entrypoint source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: not implemented at ")
    );
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
#[allow(clippy::too_many_lines)]
fn accepts_annotations_and_checks_provable_mismatches_on_request() {
    let path = fixture_path("type-annotations");
    fs::write(
        &path,
        "val label:str|nil = \"ready\"\nval User = struct { name:str = \"Slug\" }\nval double = fn<T>(value:num):num { value * 2 }\nprintln(label, double(2), User {}.name)\n",
    )
    .expect("write annotated source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run annotated source");
    fs::remove_file(&path).expect("remove annotated source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "ready 4 Slug\n");

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nprintln(first<str>(\"left\", \"right\"))\n",
    )
    .expect("write generic call source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run generic call source");
    fs::remove_file(&path).expect("remove generic call source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "left\n");

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nfirst(1, \"wrong\")\n",
    )
    .expect("write inconsistent generic call");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run inconsistent generic call");
    fs::remove_file(&path).expect("remove inconsistent generic call");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );

    fs::write(
        &path,
        "val first = fn<T>(left:T, right:T):T { left }\nfirst(1, \"wrong\") /> println\n",
    )
    .expect("write piped inconsistent generic call");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run piped inconsistent generic call");
    fs::remove_file(&path).expect("remove piped inconsistent generic call");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected num, got str")
    );

    fs::write(&path, "val label:str = 1\n").expect("write mismatched declaration");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched declaration");
    fs::remove_file(&path).expect("remove mismatched declaration");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );

    fs::write(&path, "val label = fn():str { 1 }\n").expect("write mismatched return");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched return");
    fs::remove_file(&path).expect("remove mismatched return");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );

    fs::write(&path, "val User = struct { name:str = 1 }\n")
        .expect("write mismatched struct default");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run mismatched struct default");
    fs::remove_file(path).expect("remove mismatched struct default");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: expected str, got num")
    );
}

#[test]
fn enforces_any_nil_and_canonical_type_rules() {
    let path = fixture_path("semantic-types");
    fs::write(
        &path,
        "val nonNil:any = \"ready\"\n\
         val nullable:any|nil = nil\n\
         val source:str|nil = \"value\"\n\
         val duplicate:str|nil = source\n\
         val values:list<str|nil> = [\"value\", nil]\n\
         val same:list<str|nil> = values\n\
         val safe = fn():any { nonNil }\n\
         val maybe = fn():any|nil { nullable }\n\
         println(duplicate, same, safe(), maybe())\n",
    )
    .expect("write canonical semantic type source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run canonical semantic type source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "value [\"value\", nil] ready nil\n"
    );

    fs::write(&path, "val invalid:any = nil\n").expect("write nil-to-any source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run nil-to-any source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: expected any, got nil")
    );

    fs::write(
        &path,
        "val identity = fn<T>(value:T):T { value }\nidentity(nil)\n",
    )
    .expect("write nil generic inference source");
    let output = slug()
        .arg("-type-check")
        .arg(&path)
        .output()
        .expect("run nil generic inference source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: generic type argument cannot include nil")
    );

    fs::write(&path, "val invalid:nmu = 1\n").expect("write unknown annotation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run unknown annotation source");
    fs::remove_file(path).expect("remove semantic type source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: unknown type `nmu`")
    );
}

#[test]
fn resolves_statically_known_calls_through_lexical_callable_scopes() {
    let path = fixture_path("scoped-callables");
    fs::write(
        &path,
        "val render = fn(value:str):str { \"outer:\" + value }\n\
         val alias = render\n\
         val invoke = fn(render) { render(2) }\n\
         val inner = {\n\
           val render = fn(value:num):num { value + 1 }\n\
           render(2)\n\
         }\n\
         println(inner, render(\"ok\"), alias(\"alias\"), invoke(fn(value) { value + 3 }))\n",
    )
    .expect("write scoped callable source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run scoped callable source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3 outer:ok outer:alias 5\n"
    );

    fs::write(
        &path,
        "val render = fn(value:str):str { value }\nrender(1)\n",
    )
    .expect("write statically invalid call source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run statically invalid call source");
    fs::remove_file(path).expect("remove scoped callable source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: expected str, got num")
    );
}

#[test]
fn accepts_tags_and_evaluates_their_arguments_before_declarations() {
    let path = fixture_path("tags");
    fs::write(
        &path,
        "var observed = 0\n@audit(observed = observed + 1)\nval increment = fn(@unit value) { value + 1 }\nprintln(observed, increment(2))\n",
    )
    .expect("write tagged source");
    let output = slug().arg(&path).output().expect("run tagged source");
    fs::remove_file(&path).expect("remove tagged source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1 3\n");

    fs::write(&path, "@export val value = 1\nprintln(value)\n")
        .expect("write legacy export-tag source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run legacy export-tag source");
    fs::remove_file(&path).expect("remove legacy export-tag source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");

    let cases = [(
        "tagged-expression",
        "@audit println(1)\n",
        "slug: parse error: documentation blocks and tags must prefix a val, var, or foreign declaration",
    )];
    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid tagged source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid tagged source");
        fs::remove_file(path).expect("remove invalid tagged source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn attaches_strict_documentation_blocks_to_top_level_declarations() {
    let path = fixture_path("documentation-blocks");
    fs::write(
        &path,
        "/**\n * Adds one to a value.\n */\n// A comment may intervene.\n@deprecated\nval increment = fn(value) { value + 1 }\nprintln(increment(2))\n",
    )
    .expect("write documented source");
    let output = slug().arg(&path).output().expect("run documented source");
    fs::remove_file(&path).expect("remove documented source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n");

    fs::write(
        &path,
        "/**\n * Module documentation.\n */\n\n/**\n * Fibonacci documentation.\n */\nvar fib = fn(n) match {\n x if x < 2 => x\n x => fib(x - 2) + fib(x - 1)\n}\nprintln(fib(6))\n",
    )
    .expect("write module-documented source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run module-documented source");
    fs::remove_file(&path).expect("remove module-documented source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "8\n");

    let cases = [
        (
            "malformed-documentation-block",
            "/**\n not a documentation line\n */\nval value = 1\n",
            "slug: parse error: every non-empty documentation line must begin with *",
        ),
        (
            "misplaced-documentation-block",
            "/**\n * Documentation\n */\nprintln(1)\n",
            "slug: parse error: documentation blocks and tags must prefix a val, var, or foreign declaration",
        ),
        (
            "nested-documentation-block",
            "val value = fn() {\n /**\n  * Documentation\n  */\n val inner = 1\n inner\n}\n",
            "slug: parse error: documentation blocks are only valid at top level",
        ),
    ];
    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid documented source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid documented source");
        fs::remove_file(path).expect("remove invalid documented source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn reports_unregistered_documented_foreign_declarations() {
    let path = fixture_path("documented-foreign-declaration");
    fs::write(
        &path,
        "/**\n * creates a new channel with an optional buffer capacity.\n *\n * An unbuffered channel (capacity 0) blocks the sender until a receiver\n * is ready. A buffered channel allows up to `capacity` messages to be\n * queued before blocking.\n */\nexport foreign chan = fn(capacity:num = 0):chan<any|nil>\n",
    )
    .expect("write documented foreign source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run documented foreign source");
    fs::remove_file(&path).expect("remove documented foreign source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("foreign function `")
            && String::from_utf8_lossy(&output.stderr).contains(".chan` is not registered")
    );
}

#[test]
fn imports_slug_channel_with_its_registered_foreign_bindings() {
    let path = fixture_path("slug-channel-library");
    fs::write(
        &path,
        "val channel = import(\"slug.channel\")\n\
         val inbox = channel.chan(2)\n\
         val returned = inbox /> channel.send(7) /> channel.send(42)\n\
         println(returned == inbox)\n\
         println(channel.recv(inbox))\n\
         println(channel.recv(inbox))\n\
         channel.close(inbox)\n\
         println(channel.recv(inbox))\n",
    )
    .expect("write slug.channel source");
    let output = slug()
        .arg(&path)
        .env("SLUG_HOME", env!("CARGO_MANIFEST_DIR"))
        .env_remove("SLUG_FIXTURE_LIBRARY_ROOT")
        .output()
        .expect("run slug.channel source");
    fs::remove_file(path).expect("remove slug.channel source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\n7\n42\nnil\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn discards_function_parameters_without_introducing_bindings() {
    let path = fixture_path("discard-parameters");
    fs::write(
        &path,
        channel_source("val channel = channel(1)\nprintln(0 /> fn(_) { channel })\nprintln(fn(_, _) { 7 }(1, 2))\n"),
    )
    .expect("write discard parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run discard parameter source");
    fs::remove_file(&path).expect("remove discard parameter source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "<chan>\n7\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn pipes_values_into_calls_and_subjectless_matches() {
    let path = fixture_path("pipeline");
    fs::write(
        &path,
        "val add = fn(first, second) { first + second }\nval double = fn(value) { value * 2 }\nval total = 2 /> add(3) /> double\nval first = [1, 2, 3] /> match {\n  [head, ...] => head\n}\nprintln(total, first)\n",
    )
    .expect("write pipeline source");
    let output = slug().arg(&path).output().expect("run pipeline source");
    fs::remove_file(&path).expect("remove pipeline source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "10 1\n");

    fs::write(&path, "1 /> match 2 { _ => 3 }\n").expect("write invalid pipeline match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid pipeline match source");
    fs::remove_file(path).expect("remove invalid pipeline match source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: pipeline match must omit its subject")
    );
}

#[test]
fn matches_and_destructures_structs_by_schema_identity() {
    let path = fixture_path("struct-patterns");
    fs::write(
        &path,
        "val User = struct { name, active = true }\nval OtherUser = struct { name, active = true }\nval user = User { name: \"Slug\" }\nval describe = fn(value) match {\n  User {name, active: true} => name\n  _ => \"other\"\n}\nval missing = fn(value) match {\n  User {missing} => \"matched\"\n  _ => \"other\"\n}\nval User {name: extracted} = user\nprintln(describe(user), describe(OtherUser { name: \"Slug\" }), missing(user), extracted)\n",
    )
    .expect("write struct pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run struct pattern source");
    fs::remove_file(&path).expect("remove struct pattern source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug other other Slug\n"
    );

    fs::write(
        &path,
        "val User = struct { name }\nmatch User { name: \"Slug\" } { User {name, name} => name }\n",
    )
    .expect("write duplicate struct pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate struct pattern source");
    fs::remove_file(&path).expect("remove duplicate struct pattern source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: parse error: duplicate struct pattern field `name`")
    );

    fs::write(
        &path,
        "val NotSchema = 1\nmatch nil { NotSchema {} => true }\n",
    )
    .expect("write invalid struct pattern schema source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid struct pattern schema source");
    fs::remove_file(path).expect("remove invalid struct pattern schema source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: struct pattern schema must be a struct schema")
    );
}

#[test]
fn evaluates_checked_bitwise_and_shift_operators() {
    let path = fixture_path("bitwise-and-shifts");
    fs::write(&path, "println(6 & 3, 4 | 1, 6 ^ 3, ~0, 1 << 4, -8 >> 2)\n")
        .expect("write bitwise source");
    let output = slug().arg(&path).output().expect("run bitwise source");
    fs::remove_file(&path).expect("remove bitwise source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "2 5 5 -1 16 -2\n"
    );

    for source in ["1 << -1\n", "1 << 64\n", "1.5 & 1\n", "~true\n"] {
        fs::write(&path, source).expect("write invalid bitwise source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid bitwise source");
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with("slug: runtime error:")
        );
    }
    fs::remove_file(path).expect("remove invalid bitwise source");
}

#[test]
fn appends_and_prepends_list_values_with_checked_operands() {
    let path = fixture_path("list-concatenation");
    fs::write(
        &path,
        "val original = [1, 2]\nval appended = original :+ 3\nval combined = original + [3, 4]\nprintln(original, appended, 0 +: original, combined)\n",
    )
        .expect("write list concatenation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run list concatenation source");
    fs::remove_file(&path).expect("remove list concatenation source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, 2] [1, 2, 3] [0, 1, 2] [1, 2, 3, 4]\n"
    );

    fs::write(&path, "1 :+ 2\n").expect("write invalid list concatenation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid list concatenation source");
    fs::remove_file(path).expect("remove invalid list concatenation source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: left operand of :+ must be a list")
    );
}

#[test]
fn parses_decimal_hexadecimal_and_byte_literals() {
    let path = fixture_path("numeric-and-byte-literals");
    fs::write(
        &path,
        "println(1_000, 1.5, 2e3, 1.25e-2, 0x10, 0x_ff, 0x\"414243\")\n",
    )
    .expect("write numeric and byte literals");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run numeric and byte literals");
    fs::remove_file(path).expect("remove numeric and byte literals");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1000 1.5 2000 0.0125 16 255 0x\"414243\"\n"
    );
}

#[test]
fn rejects_malformed_hexadecimal_and_byte_literals_with_locations() {
    for (kind, source, message) in [
        ("empty-hex", "0x\n", "expected hexadecimal digit"),
        (
            "odd-byte-literal",
            "0x\"f\"\n",
            "byte literal must contain one or more complete hexadecimal byte pairs",
        ),
        (
            "empty-byte-literal",
            "0x\"\"\n",
            "byte literal must contain one or more complete hexadecimal byte pairs",
        ),
        (
            "invalid-byte-literal",
            "0x\"gg\"\n",
            "invalid hexadecimal digit in byte literal",
        ),
        (
            "double-decimal-separator",
            "1__000\n",
            "invalid number separator",
        ),
        (
            "trailing-hexadecimal-separator",
            "0xff_\n",
            "invalid hexadecimal number separator",
        ),
        ("missing-exponent", "1e\n", "expected exponent digit"),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write malformed literal source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run malformed literal source");
        fs::remove_file(path).expect("remove malformed literal source");

        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(stderr.starts_with("slug: parse error:"), "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
        assert!(stderr.ends_with(":1:1\n"), "{stderr}");
    }
}

#[test]
fn executes_source_through_the_public_cli() {
    let path = fixture_path("success");
    fs::write(&path, "val total = 6 * 7\nprintln(total)\n").expect("write Slug source");
    let output = slug().arg(&path).output().expect("run Slug source");
    fs::remove_file(path).expect("remove Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_bindings_assignments_comments_and_strings() {
    let path = fixture_path("state");
    fs::write(
        &path,
        "# track mutable state\nvar label = \"Slug\"\nlabel = label + \" VM\"\nprintln(label)\n",
    )
    .expect("write stateful Slug source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run stateful Slug source");
    fs::remove_file(path).expect("remove stateful Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug VM\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn parses_raw_triple_quoted_and_extended_escaped_strings() {
    let path = fixture_path("string-forms");
    fs::write(
        &path,
        "val name = \"Slug\"\nprintln('C:\\Program Files\\Slug', \"escaped \\$ and \\{\", \"\"\"\nfirst\n  second\n\"\"\", '''\nliteral $name\n''')\n",
    )
    .expect("write string forms");
    let output = slug().arg(&path).output().expect("run string forms");
    fs::remove_file(path).expect("remove string forms");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "C:\\Program Files\\Slug escaped $ and \\{ first\n  second literal $name\n"
    );
}

#[test]
fn elides_newlines_adjacent_to_triple_string_delimiters() {
    let path = fixture_path("triple-string-final-newline");
    fs::write(&path, "println(\"\"\"\nfirst\nsecond\n\"\"\")\n")
        .expect("write triple string source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run triple string source");
    fs::remove_file(path).expect("remove triple string source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "first\nsecond\n");
}

#[test]
fn parses_one_to_three_digit_octal_string_escapes() {
    let path = fixture_path("octal-string-escapes");
    fs::write(&path, "println(\"\\101\\40\\141\")\n").expect("write octal escapes");
    let output = slug().arg(&path).output().expect("run octal escapes");
    fs::remove_file(path).expect("remove octal escapes");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "A a\n");
}

#[test]
fn interpolates_identifier_values_in_non_raw_strings() {
    let path = fixture_path("identifier-interpolation");
    fs::write(
        &path,
        "val name = \"Slug\"\nval total = 42\nprintln(\"Hello $name\", \"Total: $total\", '$name')\n",
    )
    .expect("write interpolation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run interpolation source");
    fs::remove_file(&path).expect("remove interpolation source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Hello Slug Total: 42 $name\n"
    );

    fs::write(&path, "\"$missing\"\n").expect("write unknown interpolation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run unknown interpolation source");
    fs::remove_file(path).expect("remove unknown interpolation source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error:")
    );
}

#[test]
fn executes_core_functions_blocks_conditionals_and_collections() {
    let path = fixture_path("core-language");
    fs::write(
        &path,
        "val choose = fn(a, b) { if (a > b) { a } else { b } }\n\
         val make = fn(x) { fn(y) { x + y } }\n\
         val total = { val first = 40\n first + 2 }\n\
         val values = [10, 20, 30]\n\
         val key = \"label\"\n\
         val user = {name: \"Slug\", [key]: 7}\n\
         println(choose(2, 9), make(40)(2), total, values[-1], user.name, user[key])\n",
    )
    .expect("write core Slug source");
    let output = slug().arg(&path).output().expect("run core Slug source");
    fs::remove_file(path).expect("remove core Slug source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "9 42 42 30 Slug 7\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn slices_lists_with_an_omitted_start() {
    let path = fixture_path("list-slices");
    fs::write(
        &path,
        "val values = [10, 20, 30, 40, 50]\n\
         println(values[:2], values[0:2], values[1:4:2], values[-3:])\n",
    )
    .expect("write slice source");
    let output = slug().arg(&path).output().expect("run slice source");
    fs::remove_file(path).expect("remove slice source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[10, 20] [10, 20] [20, 40] [30, 40, 50]\n"
    );
}

#[test]
fn expands_list_and_call_spreads_in_source_order() {
    let path = fixture_path("spreads");
    fs::write(
        &path,
        "var order = \"\"\n\
         val mark = fn(value) { order = order + value; value }\n\
         val values = [mark(\"a\"), ...[mark(\"b\")], ...[mark(\"c\")], mark(\"d\")]\n\
         val collect = fn(first, second, third, fourth) { first + second + third + fourth }\n\
         println(values, collect(...[mark(\"e\")], ...[mark(\"f\")], mark(\"g\"), mark(\"h\")), order)\n",
    )
    .expect("write spread source");
    let output = slug().arg(&path).output().expect("run spread source");
    fs::remove_file(path).expect("remove spread source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "[\"a\", \"b\", \"c\", \"d\"] efgh abcdefgh\n"
    );
}

#[test]
fn binds_named_source_arguments_and_reports_binding_errors() {
    let path = fixture_path("named-arguments");
    fs::write(
        &path,
        "val format = fn(first, second, third) { first + second + third }\n\
         println(format(first = \"a\", second = \"b\", third = \"c\"), format(\"a\", third = \"c\", second = \"b\"))\n",
    )
    .expect("write named argument source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run named argument source");
    fs::remove_file(path).expect("remove named argument source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "abc abc\n");

    for (kind, source) in [
        (
            "unknown-named-argument",
            "val f = fn(value) { value }\nf(other = 1)\n",
        ),
        (
            "duplicate-named-argument",
            "val f = fn(value) { value }\nf(value = 1, value = 2)\n",
        ),
        (
            "missing-required-argument",
            "val f = fn(value) { value }\nf()\n",
        ),
        (
            "excess-positional-arguments",
            "val f = fn(value) { value }\nf(1, 2)\n",
        ),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid named argument source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid named argument source");
        fs::remove_file(path).expect("remove invalid named argument source");
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.starts_with("slug: semantic error: no matching overload for `f`"),
            "{stderr}"
        );
    }
}

#[test]
fn rejects_duplicate_function_parameter_names() {
    let path = fixture_path("duplicate-parameter");
    fs::write(&path, "val duplicate = fn(value, value) { value }\n")
        .expect("write duplicate parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run duplicate parameter source");
    fs::remove_file(path).expect("remove duplicate parameter source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: semantic error: duplicate parameter 'value'")
    );
}

#[test]
fn binds_final_variadic_parameters() {
    let path = fixture_path("variadic-parameters");
    fs::write(
        &path,
        "val collect = fn(first, ...rest) { [first, rest] }\n\
         println(collect(1, 2, 3), collect(1), collect(first = 1, rest = [2, 3]))\n",
    )
    .expect("write variadic source");
    let output = slug().arg(&path).output().expect("run variadic source");
    fs::remove_file(path).expect("remove variadic source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[1, [2, 3]] [1, []] [1, [2, 3]]\n"
    );

    let path = fixture_path("non-list-named-variadic");
    fs::write(
        &path,
        "val collect = fn(...rest) { rest }\ncollect(rest = 1)\n",
    )
    .expect("write invalid variadic source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid variadic source");
    fs::remove_file(path).expect("remove invalid variadic source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: variadic parameter `rest` expects a list")
    );
}

#[test]
fn evaluates_omitted_parameter_defaults_in_the_callee() {
    let path = fixture_path("default-parameters");
    fs::write(
        &path,
        "val suffix = \"!\"\nval greet = fn(name = \"Slug\", ending = suffix) { name + ending }\nprintln(greet(), greet(name = \"Ada\"), greet(\"Rust\", \"?\"))\n",
    )
    .expect("write default parameter source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run default parameter source");
    fs::remove_file(path).expect("remove default parameter source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug! Ada! Rust?\n"
    );
}

#[test]
fn default_expressions_capture_the_function_defining_environment() {
    let path = fixture_path("default-closure-environment");
    fs::write(
        &path,
        "val make = fn(prefix) { fn(suffix = prefix) { suffix } }\n\
         val fromMaker = make(\"captured\")\n\
         val caller = fn(prefix) { fromMaker() }\n\
         println(caller(\"caller\"), fromMaker(suffix = \"explicit\"))\n",
    )
    .expect("write default closure source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run default closure source");
    fs::remove_file(path).expect("remove default closure source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "captured explicit\n"
    );
}

#[test]
fn function_match_bodies_observe_bound_defaults_and_variadics() {
    let path = fixture_path("function-match-call-binding");
    fs::write(
        &path,
        "val classify = fn(first = 1, ...rest) match {\n\
           [1, []] => \"default\"\n\
           [1, [2, 3]] => \"spread\"\n\
           _ => \"other\"\n\
         }\n\
         println(classify(), classify(1, 2, 3), classify(9))\n",
    )
    .expect("write function match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run function match source");
    fs::remove_file(path).expect("remove function match source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "default spread other\n"
    );
}

#[test]
fn rejects_non_list_source_spreads() {
    for (kind, source, expected) in [
        (
            "non-list-call-spread",
            "println(...1)\n",
            "slug: runtime error: call spread expects a list",
        ),
        (
            "non-list-literal-spread",
            "[...1]\n",
            "slug: runtime error: list spread expects a list",
        ),
    ] {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid spread source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid spread source");
        fs::remove_file(path).expect("remove invalid spread source");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn constructs_and_compares_struct_values_with_stored_defaults() {
    let path = fixture_path("struct-foundation");
    fs::write(
        &path,
        "var evaluations = 0\n\
         val User = struct {\n\
           name,\n\
           sequence = { evaluations = evaluations + 1; evaluations },\n\
         }\n\
         val first = User {name: \"Slug\"}\n\
         val second = User {name: \"Slug\"}\n\
         val Other = struct {name, sequence = 1}\n\
         val other = Other {name: \"Slug\"}\n\
         println(first.name, first[\"sequence\"], evaluations, User == User, first == second, first == other, match first { _ => \"matched\" })\n",
    )
    .expect("write struct foundation source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run struct foundation source");
    fs::remove_file(path).expect("remove struct foundation source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug 1 1 true true false matched\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn copies_structs_with_checked_replacement_fields() {
    let path = fixture_path("struct-copy");
    fs::write(
        &path,
        "val User = struct { name, active = true }\n\
         val first = User { name: \"Slug\" }\n\
         val second = first copy { active: false }\n\
         println(first.name, first.active, second.name, second.active, first == second)\n",
    )
    .expect("write struct copy source");
    let output = slug().arg(&path).output().expect("run struct copy source");
    fs::remove_file(&path).expect("remove struct copy source");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slug true Slug false false\n"
    );

    fs::write(&path, "val value = 1\nvalue copy { field: 2 }\n")
        .expect("write non-struct copy source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-struct copy source");
    fs::remove_file(path).expect("remove non-struct copy source");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("slug: runtime error: cannot copy non-struct value")
    );
}

#[test]
fn reports_checked_struct_schema_construction_and_access_errors() {
    let cases = [
        (
            "missing-struct-field",
            "val User = struct {name}\nUser {}\n",
            "slug: runtime error: missing required struct field 'name'",
        ),
        (
            "unknown-struct-field",
            "val User = struct {name}\nUser {other: 1}\n",
            "slug: runtime error: struct schema has no field 'other'",
        ),
        (
            "duplicate-struct-construction-field",
            "val User = struct {name}\nUser {name: \"a\", name: \"b\"}\n",
            "slug: runtime error: duplicate struct field 'name'",
        ),
        (
            "non-schema-construction",
            "1 {name: \"a\"}\n",
            "slug: runtime error: cannot construct struct from num",
        ),
        (
            "unknown-struct-access",
            "val User = struct {name}\nval user = User {name: \"a\"}\nuser.other\n",
            "slug: runtime error: struct has no field 'other'",
        ),
        (
            "duplicate-schema-field",
            "struct {name, name}\n",
            "slug: semantic error: duplicate struct field 'name'",
        ),
    ];

    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid struct source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run invalid struct source");
        fs::remove_file(path).expect("remove invalid struct source");
        let std::process::Output {
            status,
            stdout,
            stderr,
        } = output;
        let status = status.code();
        let stderr = String::from_utf8(stderr).expect("stderr is UTF-8");

        assert_eq!(status, Some(1));
        assert!(stdout.is_empty());
        assert!(stderr.starts_with(expected));
    }
}

#[test]
fn returns_early_from_nested_function_control_flow() {
    let path = fixture_path("explicit-return");
    fs::write(
        &path,
        "val firstPositive = fn(a, b) {\n\
           if (a > 0) { return a }\n\
           if (b > 0) { return b }\n\
           0 - 1\n\
         }\n\
         println(firstPositive(5, 9), firstPositive(-1, 7), firstPositive(-1, -2))\n",
    )
    .expect("write explicit return source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run explicit return source");
    fs::remove_file(path).expect("remove explicit return source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "5 7 -1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn reuses_function_frames_for_tail_recursion() {
    let path = fixture_path("recur");
    fs::write(
        &path,
        "val countTo = fn(n, total) {\n\
           if (n == 0) { total } else { recur(n - 1, total + 1) }\n\
         }\n\
         println(countTo(100_000, 0))\n",
    )
    .expect("write recur source");
    let output = slug().arg(&path).output().expect("run recur source");
    fs::remove_file(path).expect("remove recur source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "100000\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn recur_preserves_values_captured_by_earlier_iterations() {
    let path = fixture_path("recur-capture");
    fs::write(
        &path,
        "val retain = fn(n, saved) {\n\
           val current = n\n\
           if (n == 0) { saved() } else { recur(n - 1, fn() { current }) }\n\
         }\n\
         println(retain(1, fn() { nil }))\n",
    )
    .expect("write recur capture source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur capture source");
    fs::remove_file(path).expect("remove recur capture source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_literals_and_lists_with_case_local_bindings() {
    let path = fixture_path("match");
    fs::write(
        &path,
        "val describe = fn(value) {\n\
           match value {\n\
             0 => \"zero\"\n\
             [head, ...tail] => head + tail[0]\n\
             _ => \"other\"\n\
           }\n\
         }\n\
         val sum = fn(xs, total) {\n\
           match xs {\n\
             [] => total\n\
             [head, ...tail] => recur(tail, total + head)\n\
           }\n\
         }\n\
         println(describe(0), describe([4, 5]), describe(true), sum([1, 2, 3], 0), match 1 { 0 => \"no\" })\n",
    )
    .expect("write match source");
    let output = slug().arg(&path).output().expect("run match source");
    fs::remove_file(path).expect("remove match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "zero 9 other 6 nil\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn binds_whole_values_with_nested_at_patterns() {
    let path = fixture_path("at-patterns");
    fs::write(
        &path,
        "val describe = fn(value) match {\n\
           whole @ [head, ...tail] => whole[0] + head + tail[0]\n\
           _ => nil\n\
         }\n\
         val whole @ [first, ...rest] = [4, 5]\n\
         println(describe([1, 2]), whole[0], first, rest[0])\n",
    )
    .expect("write at pattern source");
    let output = slug().arg(&path).output().expect("run at pattern source");
    fs::remove_file(path).expect("remove at pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "4 4 4 5\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn failed_nested_at_patterns_continue_to_later_cases() {
    let path = fixture_path("failed-at-pattern");
    fs::write(
        &path,
        "val inspect = fn(value) match {\n\
           whole @ [1, 3] => whole[0]\n\
           [left, right] => left + right\n\
           _ => nil\n\
         }\n\
         println(inspect([1, 2]))\n",
    )
    .expect("write failing at pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run failing at pattern source");
    fs::remove_file(path).expect("remove failing at pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_non_binding_case_alternatives_with_guards() {
    let path = fixture_path("match-alternatives");
    fs::write(
        &path,
        "val classify = fn(value) match {\n\
           0, 1 if value == 1 => \"one\"\n\
           0, 1 => \"small\"\n\
           [0], [1] => \"list\"\n\
           _ => \"other\"\n\
         }\n\
         println(classify(1), classify(0), classify([1]), classify(3))\n",
    )
    .expect("write match alternative source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run match alternative source");
    fs::remove_file(path).expect("remove match alternative source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "one small list other\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn pinned_patterns_observe_global_local_and_captured_bindings() {
    let path = fixture_path("pinned-bindings");
    fs::write(
        &path,
        "var expected = 1\n\
         val global_match = fn(value) match { ^expected => true; _ => false }\n\
         val make_matcher = fn(expected) { fn(value) match { ^expected => true; _ => false } }\n\
         val captured_match = make_matcher(2)\n\
         val local_match = fn(expected, value) { match value { ^expected => true; _ => false } }\n\
         println(global_match(1), captured_match(2), local_match(3, 3))\n\
         expected = 2\n\
         println(global_match(1), global_match(2))\n",
    )
    .expect("write pinned binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run pinned binding source");
    fs::remove_file(path).expect("remove pinned binding source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "true true true\nfalse true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn pinned_patterns_work_in_collections_destructuring_and_alternatives() {
    let path = fixture_path("nested-pinned-patterns");
    fs::write(
        &path,
        "val expected = 2\n\
         val [^expected, tail] = [2, 3]\n\
         val {status: ^expected, value} = {status: 2, value: \"ok\"}\n\
         val fallback = match [1, 3] {\n\
           [head, ^expected] => head\n\
           [left, right] => left + right\n\
         }\n\
         val alternative = match 2 { ^expected, 0 => true; _ => false }\n\
         println(tail, value, fallback, alternative)\n",
    )
    .expect("write nested pinned pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested pinned pattern source");
    fs::remove_file(path).expect("remove nested pinned pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "3 ok 4 true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_unknown_and_malformed_pinned_patterns() {
    let unknown = fixture_path("unknown-pinned-pattern");
    fs::write(&unknown, "match 1 { ^missing => true }\n")
        .expect("write unknown pinned pattern source");
    let output = slug()
        .arg(&unknown)
        .output()
        .expect("run unknown pinned pattern source");
    fs::remove_file(unknown).expect("remove unknown pinned pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: unknown pinned binding `missing`")
    );

    let malformed = fixture_path("malformed-pinned-pattern");
    fs::write(&malformed, "match 1 { ^ => true }\n")
        .expect("write malformed pinned pattern source");
    let output = slug()
        .arg(&malformed)
        .output()
        .expect("run malformed pinned pattern source");
    fs::remove_file(malformed).expect("remove malformed pinned pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected pinned binding name")
    );
}

#[test]
fn rejects_bindings_in_match_alternatives() {
    for (label, source) in [
        ("direct", "match 1 { value, 0 => value }\n"),
        ("nested", "match [1] { [value], [] => value }\n"),
        ("at", "match 1 { whole @ 1, 0 => whole }\n"),
    ] {
        let path = fixture_path(&format!("binding-match-alternative-{label}"));
        fs::write(&path, source).expect("write binding match alternative source");
        let output = slug()
            .arg(&path)
            .output()
            .expect("run binding match alternative source");
        fs::remove_file(path).expect("remove binding match alternative source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with("slug: semantic error: match alternatives cannot introduce bindings")
        );
    }
}

#[test]
fn rejects_trailing_match_alternatives() {
    let path = fixture_path("trailing-match-alternative");
    fs::write(&path, "match 1 { 0, => \"no\" }\n")
        .expect("write trailing match alternative source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run trailing match alternative source");
    fs::remove_file(path).expect("remove trailing match alternative source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected match pattern")
    );
}

#[test]
fn rejects_duplicate_and_malformed_at_patterns() {
    let duplicate = fixture_path("duplicate-at-pattern");
    fs::write(&duplicate, "match [1] { value @ [value] => value }\n")
        .expect("write duplicate at pattern source");
    let output = slug()
        .arg(&duplicate)
        .output()
        .expect("run duplicate at pattern source");
    fs::remove_file(duplicate).expect("remove duplicate at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: duplicate match binding `value`")
    );

    let malformed = fixture_path("malformed-at-pattern");
    fs::write(&malformed, "match 1 { value @ => value }\n")
        .expect("write malformed at pattern source");
    let output = slug()
        .arg(&malformed)
        .output()
        .expect("run malformed at pattern source");
    fs::remove_file(malformed).expect("remove malformed at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected match pattern")
    );
}

#[test]
fn match_guards_use_case_bindings_and_continue_after_false() {
    let path = fixture_path("match-guards");
    fs::write(
        &path,
        "val classify = fn(value) {\n\
           match value {\n\
             n if n > 0 => \"positive\"\n\
             0 => \"zero\"\n\
             _ => \"negative\"\n\
           }\n\
         }\n\
         val firstLong = fn(value) {\n\
           match value {\n\
             [head, ...tail] if tail[0] > 10 => head\n\
             [head, ...tail] => tail[0]\n\
             _ => nil\n\
           }\n\
         }\n\
         println(classify(3), classify(0), classify(0 - 4), firstLong([1, 5]))\n",
    )
    .expect("write match guard source");
    let output = slug().arg(&path).output().expect("run match guard source");
    fs::remove_file(path).expect("remove match guard source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "positive zero negative 5\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn matches_string_keyed_maps_with_nested_patterns_and_extra_entries() {
    let path = fixture_path("map-patterns");
    fs::write(
        &path,
        "val describe = fn(user) {\n\
           match user {\n\
             {name: \"Slug\"} => \"known\"\n\
             {name, age: years} if years > 17 => name\n\
             _ => \"other\"\n\
           }\n\
         }\n\
         println(describe({name: \"Slug\", extra: true}), describe({name: \"Eve\", age: 20}), describe({name: \"Kid\", age: 5}), describe([]))\n",
    )
    .expect("write map pattern source");
    let output = slug().arg(&path).output().expect("run map pattern source");
    fs::remove_file(path).expect("remove map pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "known Eve other other\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn computed_map_pattern_keys_support_expressions_and_lexical_bindings() {
    let path = fixture_path("computed-map-patterns");
    fs::write(
        &path,
        "val globalKey = \"status\"\n\
         val read = fn(prefix) {\n\
           val suffix = \"tus\"\n\
           fn(value) match {\n\
             {[prefix + suffix]: result, ...rest} => result + rest.extra\n\
             _ => \"missing\"\n\
           }\n\
         }\n\
         val destructure = fn(key, value) { val {[key]: found} = value; found }\n\
         val exact = match ({[1]: \"one\"}) { {|[1]: result|} => result; _ => \"missing\" }\n\
         val alternative = match ({status: \"ready\"}) {\n\
           {[globalKey]: \"ok\"}, {[globalKey]: \"ready\"} => \"alternative\"\n\
           _ => \"missing\"\n\
         }\n\
         var evaluations = 0\n\
         val key = fn() { evaluations = evaluations + 1; \"status\" }\n\
         val evaluated = match ({status: \"ready\"}) { {[key()]: \"ready\"} => \"once\" }\n\
         println(destructure(globalKey, {status: \"Slug\"}), read(\"sta\")({status: \"ok\", extra: \"!\"}), exact, alternative, evaluated, evaluations)\n",
    )
    .expect("write computed map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run computed map pattern source");
    fs::remove_file(path).expect("remove computed map pattern source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug ok! one alternative once 1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_unhashable_computed_map_pattern_keys_from_source() {
    let path = fixture_path("invalid-computed-map-pattern-key");
    fs::write(&path, "match ({status: \"ok\"}) { {[[]]: _} => true }\n")
        .expect("write invalid computed map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid computed map pattern source");
    fs::remove_file(path).expect("remove invalid computed map pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: list cannot be used as a map key")
    );
}

#[test]
fn requires_a_value_pattern_after_a_computed_map_key() {
    let path = fixture_path("computed-map-pattern-shorthand");
    fs::write(
        &path,
        "match ({status: \"ok\"}) { {[\"status\"]} => true }\n",
    )
    .expect("write computed map pattern shorthand");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run computed map pattern shorthand");
    fs::remove_file(path).expect("remove computed map pattern shorthand");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected : after computed map pattern key")
    );
}

#[test]
fn function_match_bodies_follow_parameter_subject_rules() {
    let path = fixture_path("function-match");
    fs::write(
        &path,
        "val classify = fn(value) match {\n\
           0 => \"zero\"\n\
           n if n > 0 => \"positive\"\n\
           _ => \"negative\"\n\
         }\n\
         val pair = fn(left, right) match {\n\
           [1, 2] => \"one-two\"\n\
           _ => \"other\"\n\
         }\n\
         val empty = fn() match { [] => \"empty\" }\n\
         val sum = fn(xs, total) match {\n\
           [[], total] => total\n\
           [[head, ...tail], total] => recur(tail, total + head)\n\
         }\n\
         println(classify(3), classify(0), classify(0 - 1), pair(1, 2), pair(2, 1), empty(), sum([1, 2, 3], 0))\n",
    )
    .expect("write function match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run function match source");
    fs::remove_file(path).expect("remove function match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "positive zero negative one-two other empty 6\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn destructures_list_and_map_bindings_with_declared_mutability() {
    let path = fixture_path("destructuring");
    fs::write(
        &path,
        "var [first, ...tail] = [1, 2, 3]\n\
         first = 10\n\
         tail = [7]\n\
         val {name, age: years} = {name: \"Slug\", age: 3, extra: true}\n\
         println(first, tail[0], name, years)\n",
    )
    .expect("write destructuring source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run destructuring source");
    fs::remove_file(path).expect("remove destructuring source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "10 7 Slug 3\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn captures_remaining_map_entries_in_match_and_destructuring() {
    let path = fixture_path("map-rest-patterns");
    fs::write(
        &path,
        "val {name, ...rest} = {name: \"Slug\", status: \"ok\", active: true}\n\
         val describe = fn(user) match {\n\
           {name, ...remaining} => name + \":\" + remaining.status\n\
           _ => \"missing\"\n\
         }\n\
         println(name, rest.status, rest.active, describe({name: \"Eve\", status: \"ready\", age: 3}))\n",
    )
    .expect("write map rest source");
    let output = slug().arg(&path).output().expect("run map rest source");
    fs::remove_file(path).expect("remove map rest source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug ok true Eve:ready\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn discards_anonymous_list_and_map_pattern_remainders() {
    let path = fixture_path("anonymous-rest-patterns");
    fs::write(
        &path,
        "val [first, ...] = [1, 2, 3]\n\
         val list_head = fn(values) match {\n\
           [head, ...] => head\n\
           _ => nil\n\
         }\n\
         val map_name = fn(value) match {\n\
           {name, ...} => name\n\
           _ => nil\n\
         }\n\
         println(first, list_head([4, 5]), map_name({name: \"Slug\", extra: true}))\n",
    )
    .expect("write anonymous rest pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run anonymous rest pattern source");
    fs::remove_file(path).expect("remove anonymous rest pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1 4 Slug\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_non_final_anonymous_list_rest_patterns() {
    let path = fixture_path("non-final-anonymous-list-rest");
    fs::write(&path, "val [head, ..., tail] = [1, 2, 3]\n")
        .expect("write non-final anonymous list rest source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-final anonymous list rest source");
    fs::remove_file(path).expect("remove non-final anonymous list rest source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: list spread pattern must be final")
    );
}

#[test]
fn rejects_anonymous_rest_in_exact_map_patterns() {
    let path = fixture_path("anonymous-exact-map-rest");
    fs::write(&path, "val {|name, ...|} = {name: \"Slug\"}\n")
        .expect("write exact map anonymous rest source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run exact map anonymous rest source");
    fs::remove_file(path).expect("remove exact map anonymous rest source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: exact map patterns cannot contain a spread pattern")
    );
}

#[test]
fn exact_map_patterns_reject_extra_entries() {
    let path = fixture_path("exact-map-patterns");
    fs::write(
        &path,
        "val describe = fn(user) match {\n\
           {|name: \"Slug\", active: true|} => \"exact\"\n\
           {name} => name\n\
           _ => \"other\"\n\
         }\n\
         val {|name|} = {name: \"Slug\"}\n\
         println(describe({name: \"Slug\", active: true}), describe({name: \"Slug\", active: true, extra: 1}), name)\n",
    )
    .expect("write exact map pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run exact map pattern source");
    fs::remove_file(path).expect("remove exact map pattern source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "exact Slug Slug\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn reports_source_location_for_non_matching_destructuring() {
    let path = fixture_path("destructuring-failure");
    fs::write(&path, "val [head] = []\n").expect("write invalid destructuring source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid destructuring source");
    fs::remove_file(path).expect("remove invalid destructuring source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: destructuring pattern did not match at "));
    assert!(stderr.ends_with(":1:14\n  in main\n"));
}

#[test]
fn rejects_recur_outside_a_function_or_tail_position() {
    let top_level = fixture_path("top-level-recur");
    fs::write(&top_level, "recur()\n").expect("write invalid recur source");
    let output = slug()
        .arg(&top_level)
        .output()
        .expect("run invalid recur source");
    fs::remove_file(top_level).expect("remove invalid recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: recur is only valid inside a function")
    );

    let non_tail = fixture_path("non-tail-recur");
    fs::write(&non_tail, "val invalid = fn(n) { recur(n) + 1 }\n")
        .expect("write non-tail recur source");
    let output = slug()
        .arg(&non_tail)
        .output()
        .expect("run non-tail recur source");
    fs::remove_file(non_tail).expect("remove non-tail recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: semantic error: recur is only valid in tail position")
    );

    let missing_parameter = fixture_path("missing-parameter-recur");
    fs::write(
        &missing_parameter,
        "val invalid = fn(n) { recur() }\ninvalid(1)\n",
    )
    .expect("write missing-parameter recur source");
    let output = slug()
        .arg(&missing_parameter)
        .output()
        .expect("run missing-parameter recur source");
    fs::remove_file(missing_parameter).expect("remove missing-parameter recur source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: missing required parameter `n`")
    );
}

#[test]
fn recur_uses_the_ordinary_call_binding_rules() {
    let path = fixture_path("recur-call-binding");
    fs::write(
        &path,
        "val defaulted = fn(value = 7) { if (value == 7) { value } else { recur() } }\n\
         val variadic = fn(first = 1, ...rest) {\n\
           if (first == 0) { rest } else { recur(first = 0, rest = rest) }\n\
         }\n\
         val matched = fn(value = 2, ...rest) match {\n\
           [0, rest] => rest\n\
           [value, rest] => recur(value - 1, ...rest)\n\
         }\n\
         println(defaulted(4), variadic(3, 4, 5), matched(2, 8, 9))\n",
    )
    .expect("write recur binding source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur binding source");
    fs::remove_file(path).expect("remove recur binding source");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "7 [4, 5] [8, 9]\n"
    );
}

#[test]
fn rejects_top_level_return_with_a_location() {
    let path = fixture_path("top-level-return");
    fs::write(&path, "{\nreturn 1\n}\n").expect("write invalid return source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run invalid return source");
    fs::remove_file(path).expect("remove invalid return source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: semantic error: return is only valid inside a function at "));
    assert!(stderr.ends_with(":2:1\n"));
}

#[test]
fn reports_uncaught_throws_with_their_source_location_and_call_frames() {
    let path = fixture_path("throw");
    fs::write(
        &path,
        "val fail = fn() {\n\
           throw [\"bad\", 7]\n\
         }\n\
         fail()\n",
    )
    .expect("write throwing source");
    let output = slug().arg(&path).output().expect("run throwing source");
    fs::remove_file(path).expect("remove throwing source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: uncaught throw: [\"bad\", 7] at "));
    assert!(stderr.ends_with(":2:1\n  in <fn #0>\n  in main\n"));
}

#[test]
fn runs_deferred_actions_in_lifo_order_on_normal_return() {
    let path = fixture_path("defer");
    fs::write(
        &path,
        "val finish = fn(shouldThrow) {\n\
           defer println(\"outer\")\n\
           {\n\
             defer println(\"inner\")\n\
             if (shouldThrow) { throw \"stop\" }\n\
             42\n\
           }\n\
         }\n\
         println(finish(false))\n",
    )
    .expect("write deferred source");
    let output = slug().arg(&path).output().expect("run deferred source");
    fs::remove_file(path).expect("remove deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "inner\nouter\n42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn runs_deferred_actions_before_an_uncaught_throw() {
    let path = fixture_path("defer-throw");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"first\")\n\
           defer println(\"second\")\n\
           throw \"stop\"\n\
         }\n\
         fail()\n",
    )
    .expect("write throwing deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run throwing deferred source");
    fs::remove_file(path).expect("remove throwing deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "second\nfirst\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("uncaught throw: stop")
    );
}

#[test]
fn runs_deferred_actions_before_a_runtime_fault() {
    let path = fixture_path("defer-runtime-fault");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"cleanup\")\n\
           1 / 0\n\
         }\n\
         fail()\n",
    )
    .expect("write faulting deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run faulting deferred source");
    fs::remove_file(path).expect("remove faulting deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "cleanup\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("division by zero")
    );
}

#[test]
fn runs_onsuccess_actions_only_after_normal_completion() {
    let path = fixture_path("defer-onsuccess");
    fs::write(
        &path,
        "val complete = fn() { defer println(\"always\")\n defer onsuccess println(\"success\")\n 1 }\nprintln(complete())\n",
    ).expect("write onsuccess source");
    let output = slug().arg(&path).output().expect("run onsuccess source");
    fs::remove_file(path).expect("remove onsuccess source");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "success\nalways\n1\n"
    );
}

#[test]
fn recovers_errors_with_defer_onerror_and_resumes_the_caller() {
    let path = fixture_path("defer-onerror");
    fs::write(
        &path,
        "val fail = fn(pass) {\n\
           defer println(\"always\")\n\
           defer onerror(err) { println(\"caught\", err)\n 10 }\n\
           defer onsuccess println(\"success\")\n\
           if (pass) { \"ok\" } else { throw \"bad\" }\n\
         }\n\
         println(fail(false))\n",
    )
    .expect("write recovering deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recovering deferred source");
    fs::remove_file(path).expect("remove recovering deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "caught bad\nalways\n10\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn exposes_checked_faults_to_defer_onerror_as_structured_values() {
    let path = fixture_path("defer-onerror-fault");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer onerror(err) { println(err.type, err.msg, err.data)\n 42 }\n\
           1 / 0\n\
         }\n\
         println(\"after\", fail())\n",
    )
    .expect("write fault-recovering deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run fault-recovering deferred source");
    fs::remove_file(path).expect("remove fault-recovering deferred source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "divide_by_zero division by zero nil\nafter 42\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn recovery_preserves_the_callers_active_scopes() {
    let path = fixture_path("defer-onerror-caller-scope");
    fs::write(
        &path,
        "val callee = fn() { defer onerror(err) { 1 }\n throw \"bad\" }\n\
         val caller = fn() {\n\
           { callee() }\n\
           7\n\
         }\n\
         println(caller())\n",
    )
    .expect("write caller-scope recovery source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run caller-scope recovery source");
    fs::remove_file(path).expect("remove caller-scope recovery source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "7\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rethrowing_deferred_handlers_run_older_pending_cleanup() {
    let path = fixture_path("defer-onerror-rethrow-cleanup");
    fs::write(
        &path,
        "val fail = fn() {\n\
           defer println(\"first\")\n\
           defer println(\"second\")\n\
           defer onerror(err) { println(\"handler\")\n throw \"replacement\" }\n\
           throw \"original\"\n\
         }\n\
         fail()\n",
    )
    .expect("write rethrowing deferred source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run rethrowing deferred source");
    fs::remove_file(path).expect("remove rethrowing deferred source");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "handler\nsecond\nfirst\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("uncaught throw: replacement")
    );
}

#[test]
fn recur_exits_nested_scopes_before_starting_the_next_iteration() {
    let path = fixture_path("recur-nested-defer");
    fs::write(
        &path,
        "val count = fn(n) {\n\
           {\n\
             defer println(n)\n\
             if (n == 0) { 0 } else { recur(n - 1) }\n\
           }\n\
         }\n\
         println(count(2))\n",
    )
    .expect("write recur cleanup source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run recur cleanup source");
    fs::remove_file(path).expect("remove recur cleanup source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "2\n1\n0\n0\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn deferred_actions_run_their_own_pending_cleanup_before_returning() {
    let path = fixture_path("deferred-action-cleanup");
    fs::write(
        &path,
        "val complete = fn() {\n\
           defer {\n\
             defer println(\"inner\")\n\
             println(\"outer\")\n\
             return nil\n\
           }\n\
           1\n\
         }\n\
         println(complete())\n",
    )
    .expect("write nested deferred cleanup source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run nested deferred cleanup source");
    fs::remove_file(path).expect("remove nested deferred cleanup source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "outer\ninner\n1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn non_tail_match_discards_its_subject_before_producing_a_result() {
    let path = fixture_path("non-tail-match");
    fs::write(&path, "println(match 1 { 1 => \"yes\" })\n").expect("write non-tail match source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run non-tail match source");
    fs::remove_file(path).expect("remove non-tail match source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "yes\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn bare_map_keys_and_dot_access_use_strings() {
    let path = fixture_path("string-map-keys");
    fs::write(
        &path,
        "val key = \"name\"\nval user = {name: \"Slug\"}\nprintln(user.name, user[key], user[\"name\"])\n",
    )
    .expect("write string-keyed map source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run string-keyed map source");
    fs::remove_file(path).expect("remove string-keyed map source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "Slug Slug Slug\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_removed_symbol_literals() {
    let path = fixture_path("removed-symbol-literal");
    fs::write(&path, "println(:name)\n").expect("write removed symbol literal source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run removed symbol literal source");
    fs::remove_file(path).expect("remove symbol literal source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: parse error: expected expression")
    );
}

#[test]
fn closures_share_mutable_lexical_bindings() {
    let path = fixture_path("mutable-capture");
    fs::write(
        &path,
        "val makeCounter = fn() {\n\
           var value = 0\n\
           fn() {\n\
             value = value + 1\n\
             value\n\
           }\n\
         }\n\
         val counter = makeCounter()\n\
         val makePair = fn() {\n\
           var value = 0\n\
           val increment = fn() { value = value + 1 }\n\
           val current = fn() { value }\n\
           [increment, current]\n\
         }\n\
         val pair = makePair()\n\
         pair[0]()\n\
         pair[0]()\n\
         println(counter(), counter(), pair[1]())\n",
    )
    .expect("write mutable capture source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run mutable capture source");
    fs::remove_file(path).expect("remove mutable capture source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1 2 2\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn short_circuits_logical_operators_and_continues_across_newlines() {
    let path = fixture_path("logical-operators");
    fs::write(
        &path,
        "var calls = 0\n\
         val bump = fn() {\n\
           calls = calls + 1\n\
           true\n\
         }\n\
         false &&\n\
           bump()\n\
         true\n\
           || bump()\n\
         val both = true &&\n\
           true\n\
         val either = false\n\
           || true\n\
         println(calls, both, either)\n",
    )
    .expect("write logical operator source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run logical operator source");
    fs::remove_file(path).expect("remove logical operator source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "0 true true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn handles_comments_and_multiline_delimited_expressions() {
    let path = fixture_path("newlines");
    fs::write(
        &path,
        "println(1) # comment\n\
         println(2)\n\
         println(1\n\
         - 2)\n\
         println(\n\
           3\n\
         )\n\
         println([\n\
           1,\n\
           2\n\
         ][-1])\n\
         println({ [1, 2] })\n",
    )
    .expect("write multiline source");
    let output = slug().arg(&path).output().expect("run multiline source");
    fs::remove_file(path).expect("remove multiline source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n2\n-1\n3\n2\n[1, 2]\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn supports_all_documented_comment_forms_and_dot_string_lookup() {
    let path = fixture_path("comments-and-dot-access");
    fs::write(
        &path,
        "/**\n\
         * Documentation comment\n\
         */\n\
         val user = {[\"name\"]: 1}\n\
         // line comment\n\
         println(user.name) /* block comment */\n",
    )
    .expect("write comment source");
    let output = slug().arg(&path).output().expect("run comment source");
    fs::remove_file(path).expect("remove comment source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "1\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn parses_long_prefix_sequences_without_recursion() {
    let path = fixture_path("prefix-depth");
    let source = format!("println({}true)\n", "!".repeat(100_000));
    fs::write(&path, source).expect("write deeply prefixed source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply prefixed source");
    fs::remove_file(path).expect("remove deeply prefixed source");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "true\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn rejects_deeply_nested_source_without_aborting() {
    let path = fixture_path("nesting-depth");
    let source = format!("println({}true{})\n", "(".repeat(600), ")".repeat(600));
    fs::write(&path, source).expect("write deeply nested source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply nested source");
    fs::remove_file(path).expect("remove deeply nested source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("slug: parse error: source nesting limit exceeded")
    );

    let path = fixture_path("at-pattern-nesting-depth");
    let source = format!("match 1 {{ {}_ => 1 }}\n", "value @ ".repeat(600));
    fs::write(&path, source).expect("write deeply nested at pattern source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run deeply nested at pattern source");
    fs::remove_file(path).expect("remove deeply nested at pattern source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .contains("slug: parse error: source nesting limit exceeded")
    );
}

#[test]
fn rejects_assignment_to_an_immutable_binding_with_a_location() {
    let path = fixture_path("immutable-binding");
    fs::write(&path, "val answer = 1\nanswer = 2\n").expect("write invalid assignment");
    let output = slug().arg(&path).output().expect("run invalid assignment");
    fs::remove_file(path).expect("remove invalid assignment");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.starts_with("slug: semantic error: cannot assign to immutable binding `answer` at ")
    );
    assert!(stderr.ends_with(":2:1\n"));
}

#[test]
fn retains_source_locations_for_runtime_faults_from_source() {
    let path = fixture_path("runtime-location");
    fs::write(&path, "val denominator = 0\nprintln(1 / denominator)\n")
        .expect("write runtime fault source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run runtime fault source");
    fs::remove_file(path).expect("remove runtime fault source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: runtime error: division by zero at "));
    assert!(stderr.ends_with(":2:11\n  in main\n"));
}

#[test]
fn reports_source_parse_errors_without_a_host_crash() {
    let path = fixture_path("invalid");
    fs::write(&path, "val = 1\n").expect("write invalid Slug source");
    let output = slug().arg(&path).output().expect("run invalid Slug source");
    fs::remove_file(path).expect("remove invalid Slug source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.starts_with("slug: parse error: expected binding name at "));
    assert!(stderr.ends_with(":1:5\n"));
}

#[test]
fn rejects_malformed_call_and_variadic_parameter_lists_with_locations() {
    let cases = [
        (
            "positional-after-named",
            "println(label = \"Slug\", 1)\n",
            "slug: parse error: positional argument cannot appear after a named argument",
        ),
        (
            "spread-after-named",
            "println(label = \"Slug\", ...[1])\n",
            "slug: parse error: spread argument cannot appear after a named argument",
        ),
        (
            "variadic-not-final",
            "val collect = fn(...rest, value) { value }\n",
            "slug: parse error: variadic parameter must be final",
        ),
        (
            "variadic-default",
            "val collect = fn(...rest = []) { rest }\n",
            "slug: parse error: variadic parameters cannot have defaults",
        ),
    ];

    for (kind, source, expected) in cases {
        let path = fixture_path(kind);
        fs::write(&path, source).expect("write invalid call source");
        let output = slug().arg(&path).output().expect("run invalid call source");
        fs::remove_file(path).expect("remove invalid call source");

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("stderr is UTF-8")
                .starts_with(expected)
        );
    }
}

#[test]
fn reports_runtime_faults_without_a_host_crash() {
    let path = fixture_path("runtime");
    fs::write(&path, "println(1 / 0)\n").expect("write faulting Slug source");
    let output = slug()
        .arg(&path)
        .output()
        .expect("run faulting Slug source");
    fs::remove_file(path).expect("remove faulting Slug source");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr is UTF-8")
            .starts_with("slug: runtime error: division by zero")
    );
}
