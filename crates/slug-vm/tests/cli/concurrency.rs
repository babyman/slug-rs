use super::*;

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
