use super::*;

#[cfg(not(feature = "concurrency"))]
#[test]
fn slim_runtime_defers_concurrency_capability_errors_until_execution() {
    let program = compile(
        "slim-capability.slug",
        "if (false) { spawn { 42 } }\nselect { after 1 }\n",
    )
    .expect("concurrency syntax remains valid in a slim build");
    let error = Vm::new()
        .run_named(&program, "main")
        .expect_err("executed scheduler operation must report its unavailable capability");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(
        error.message,
        "runtime capability `select timer or task-await` is unavailable"
    );
}

#[cfg(not(feature = "concurrency"))]
#[test]
fn slim_runtime_pumps_native_channel_select_without_a_scheduler() {
    struct ProducerState(Mutex<Option<slug_vm::NativeChannelProducer>>);

    fn create_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        call.state::<Arc<ProducerState>>()
            .expect("producer state")
            .0
            .lock()
            .expect("producer state lock")
            .replace(producer);
        call.return_value(channel)
    }

    let state = Arc::new(ProducerState(Mutex::new(None)));
    let module = NativeModule::new("test.slim_host_pump", state.clone()).unwrap();
    let function = module
        .function("create_channel", NativeArity::Exact(0), create_channel)
        .unwrap();
    let program = compile(
        "slim-host-pump.slug",
        "val channel = create_channel()\nselect { recv channel }\n",
    )
    .expect("compile a channel-only select");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    vm.start_named(&program, "main")
        .expect("start host-driven execution");
    assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));

    let producer = state
        .0
        .lock()
        .expect("producer state lock")
        .clone()
        .expect("channel producer");
    assert_eq!(
        producer.try_send(slug_vm::NativeSendValue::integer(42)),
        slug_vm::NativeProducerStatus::Sent
    );
    assert!(matches!(
        vm.run_until_stalled(),
        VmProgress::Completed(Value::Int(42))
    ));
}

#[test]
fn full_and_slim_compile_the_same_scheduler_source() {
    let program = compile(
        "compiler-feature-equivalence.slug",
        "val channel = chan(0)\n\
         val task = spawn { select { recv channel; after 1 } }\n\
         nursery { select { await task; after 1 } }\n",
    );
    assert!(
        program.is_ok(),
        "runtime feature selection must not change source acceptance: {program:?}"
    );
}

fn plain_channel(call: &mut NativeCall<'_>) -> NativeStatus {
    let channel = call.plain_channel(0);
    call.return_value(channel)
}

#[test]
fn a_blocked_host_execution_leaves_the_vm_reusable() {
    let module = NativeModule::new("test.blocked_host_cleanup", ()).unwrap();
    let function = module
        .function("plain_channel", NativeArity::Exact(0), plain_channel)
        .unwrap();
    let blocked = compile(
        "blocked-host-execution.slug",
        "val channel = plain_channel()\nselect { recv channel }\n",
    )
    .expect("compile permanently blocked channel wait");
    let complete =
        compile("reused-host-execution.slug", "42\n").expect("compile follow-up execution");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();

    let error = vm
        .run_named(&blocked, "main")
        .expect_err("an ordinary channel with no sender must block");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.message, "task remains blocked with no runnable work");
    assert_eq!(vm.run_named(&complete, "main").unwrap(), Value::Int(42));
}

#[cfg(feature = "metrics")]
#[test]
fn abandoning_a_blocked_host_execution_removes_its_channel_waiter() {
    let module = NativeModule::new("test.blocked_waiter_cleanup", ()).unwrap();
    let function = module
        .function("plain_channel", NativeArity::Exact(0), plain_channel)
        .unwrap();
    let program = compile(
        "blocked-waiter-cleanup.slug",
        "val channel = plain_channel()\nselect { recv channel }\n",
    )
    .expect("compile permanently blocked channel wait");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();

    vm.run_named(&program, "main")
        .expect_err("an ordinary channel with no sender must block");
    assert!(
        vm.metrics().wait_registration_removals >= 1,
        "discarding the root execution must remove its channel waiter"
    );
}

#[test]
fn shutdown_cancels_a_host_execution_before_its_first_poll() {
    let program = compile("shutdown-before-poll.slug", "42\n").expect("compile program");
    let mut vm = Vm::new();
    vm.start_named(&program, "main").expect("start execution");
    vm.shutdown();

    assert_shutdown_progress(&mut vm);
}

#[test]
fn shutdown_cancels_a_host_execution_suspended_on_a_channel() {
    let module = NativeModule::new("test.shutdown_suspension", ()).unwrap();
    let function = module
        .function("plain_channel", NativeArity::Exact(0), plain_channel)
        .unwrap();
    let program = compile(
        "shutdown-suspended-execution.slug",
        "val channel = plain_channel()\nselect { recv channel }\n",
    )
    .expect("compile blocked channel wait");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    vm.start_named(&program, "main").expect("start execution");
    assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));
    vm.shutdown();

    assert_shutdown_progress(&mut vm);
}

fn assert_shutdown_progress(vm: &mut Vm) {
    for progress in [vm.poll(), vm.run_until_stalled()] {
        let VmProgress::Failed(error) = progress else {
            panic!("shutdown must reject all later progress calls");
        };
        assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
        assert_eq!(error.message, "VM has shut down");
    }
    let error = vm
        .blocking_run()
        .expect_err("blocking progress after shutdown must fail");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.message, "VM has shut down");
}
