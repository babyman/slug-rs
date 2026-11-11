use super::*;

#[test]
fn host_driven_execution_stalls_without_waiting_then_observes_a_due_timer() {
    let program = compile("host-driven-timer.slug", "select { after 50 }\n42\n")
        .expect("compile host-driven timer source");
    let mut vm = Vm::new();
    vm.start_named(&program, "main")
        .expect("start host-driven execution");

    let started = std::time::Instant::now();
    assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(20),
        "host-driven progress must not wait for the timer"
    );

    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(matches!(
        vm.run_until_stalled(),
        VmProgress::Completed(Value::Int(42))
    ));
}

#[test]
fn host_driven_nursery_waits_for_its_children_without_blocking_the_host() {
    let program = compile(
        "host-driven-nursery.slug",
        "nursery { spawn { select { after 50 } }; nursery { select { after 50 } } }\n42\n",
    )
    .expect("compile host-driven nursery source");
    let mut vm = Vm::new();
    vm.start_named(&program, "main")
        .expect("start host-driven execution");

    let started = std::time::Instant::now();
    assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(20),
        "a nursery must not wait for its children inside poll"
    );
    assert!(vm.next_timer_in().is_some(), "its children wait on timers");

    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(matches!(
        vm.run_until_stalled(),
        VmProgress::Completed(Value::Int(42))
    ));
}

#[test]
fn host_driven_root_settlement_returns_to_the_host_between_turns() {
    // After the root body returns, its children exchange messages for a
    // while: each poll runs at most one scheduler turn of them.
    let program = compile(
        "host-driven-settlement.slug",
        "val ping = make_channel(0)\n\
         val pong = make_channel(0)\n\
         val serve = fn(n:num) { if (n > 0) { select { send ping, n }; select { recv pong }; recur(n - 1) } }\n\
         val echo = fn(n:num) { if (n > 0) { select { recv ping }; select { send pong, n }; recur(n - 1) } }\n\
         spawn { serve(200) }\n\
         spawn { echo(200) }\n\
         42\n",
    )
    .expect("compile host-driven settlement source");
    let mut vm = vm_with_channel_constructor();
    vm.start_named(&program, "main")
        .expect("start host-driven execution");
    let mut polls = 0;
    let value = loop {
        polls += 1;
        match vm.poll() {
            VmProgress::MadeProgress => {}
            VmProgress::Completed(value) => break value,
            other => panic!("unexpected progress {other:?}"),
        }
    };
    assert_eq!(value, Value::Int(42));
    assert!(polls > 200, "settlement ran the children in {polls} polls");
}

#[test]
fn a_stalled_host_learns_how_long_until_the_next_timer() {
    let program = compile(
        "host-driven-deadline.slug",
        "select { after 80 }\nselect { after 0 }\n7\n",
    )
    .expect("compile host-driven deadline source");
    let mut vm = Vm::new();
    assert_eq!(vm.next_timer_in(), None, "nothing is waiting yet");
    vm.start_named(&program, "main")
        .expect("start host-driven execution");
    assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));
    let wait = vm
        .next_timer_in()
        .expect("the stalled program waits on a timer");
    assert!(
        wait > std::time::Duration::from_millis(40) && wait <= std::time::Duration::from_millis(80),
        "the wait is what is left of 80 ms: {wait:?}"
    );

    // A host that sleeps for that long, as a freestanding one would until
    // its timer interrupt, then finds the program done.
    std::thread::sleep(wait);
    loop {
        match vm.run_until_stalled() {
            VmProgress::Completed(value) => {
                assert_eq!(value, Value::Int(7));
                break;
            }
            VmProgress::Stalled => {
                let wait = vm.next_timer_in().expect("still waiting on a timer");
                assert!(wait < std::time::Duration::from_millis(20), "{wait:?}");
                std::thread::sleep(wait);
            }
            other => panic!("unexpected progress {other:?}"),
        }
    }
    assert_eq!(vm.next_timer_in(), None, "no timer is left");
}

#[test]
fn host_driven_execution_drains_native_ingress_without_producer_reentry() {
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
    let module = NativeModule::new("test.host_pump", state.clone()).unwrap();
    let function = module
        .function("create_channel", NativeArity::Exact(0), create_channel)
        .unwrap();
    let program = compile(
        "host-driven-native.slug",
        "val channel = create_channel()\nselect { recv channel }\n",
    )
    .expect("compile host-driven native source");
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
fn private_select_await_resumes_a_suspended_task_frame_after_a_timer() {
    let mut child = Chunk::new("child", 0);
    let delay = child.constant(Value::Int(1));
    let result = child.constant(Value::Int(42));
    child
        .emit(Op::Constant(delay))
        .emit(Op::Select(vec![SelectCase::After { has_handler: false }]))
        .emit(Op::SelectApply)
        .emit(Op::Pop)
        .emit(Op::Constant(result))
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::MakeClosure {
        chunk: 0,
        captures: vec![],
    })
    .emit(Op::Spawn)
    .emit(Op::Select(vec![SelectCase::Await { has_handler: false }]))
    .emit(Op::SelectApply)
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(child);
    program.add_chunk(main);
    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(42)
    );
}

#[test]
fn select_checks_ready_cases_before_driving_an_awaited_task() {
    let program = compile(
        "select-ready-snapshot.slug",
        "val inbox = make_channel(1)\n\
         select { send inbox, 7 }\n\
         val task = spawn { select { after 1 }; 9 }\n\
         val selected = select {\n\
           await task /> fn(value) { 1 }\n\
           recv inbox /> fn(value) { 2 }\n\
         }\n\
         select { await task }\n\
         selected\n",
    )
    .expect("compile select ready snapshot source");

    assert_eq!(
        vm_with_channel_constructor()
            .run_named(&program, "main")
            .unwrap(),
        Value::Int(2)
    );
}

#[test]
fn a_losing_select_await_does_not_observe_a_later_task_failure() {
    let program = compile(
        "select-losing-await.slug",
        "val gate = make_channel(0)\n\
         val task = spawn { select { recv gate }; throw \"lost failure\" }\n\
         select { await task; after 1 }\n\
         val sender = spawn { select { send gate, 1 } }\n\
         select { await sender }\n",
    )
    .expect("compile losing select await source");

    let error = vm_with_channel_constructor()
        .run_named(&program, "main")
        .expect_err("the losing await must not consume task failure propagation");
    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(error.message, "uncaught throw: lost failure");
}

#[test]
fn explicit_nursery_bodies_suspend_on_concurrency_operations() {
    let program = compile(
        "nursery-suspension.slug",
        "nursery { select { after 1 }; 42 }\n",
    )
    .expect("compile suspending nursery source");

    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(42)
    );
}

#[test]
fn a_failed_nursery_body_settles_its_owned_tasks() {
    let program = compile(
        "nursery-error-settlement.slug",
        "var held:any|nil = nil\n\
         val attempt = fn() {\n\
           defer onerror(error) { nil }\n\
           nursery {\n\
             held = spawn { 42 }\n\
             throw \"body failure\"\n\
           }\n\
         }\n\
         attempt()\n\
         select { await held }\n",
    )
    .expect("compile nursery error settlement source");

    let error = Vm::new()
        .run_named(&program, "main")
        .expect_err("an escaped child must already be cancelled");
    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(error.message, "sibling cancelled due to fail-fast");
}

#[test]
fn a_failed_root_body_cancels_its_pending_tasks() {
    let program = compile(
        "root-error-settlement.slug",
        "var child_ran = false\n\
         val child = spawn { child_ran = true }\n\
         throw \"root failure\"\n",
    )
    .expect("compile root error settlement source");
    let mut vm = Vm::new();

    let error = vm
        .run_named(&program, "main")
        .expect_err("the root body must retain its failure");
    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(error.message, "uncaught throw: root failure");
    assert_eq!(vm.global("child_ran"), Some(Value::Bool(false)));
}

#[test]
fn a_failed_root_body_does_not_wait_for_a_timer_task() {
    let program = compile(
        "root-error-timer.slug",
        "var fired = false\n\
         val child = spawn { select { after 2000 }; fired = true }\n\
         select { after 1 }\n\
         throw \"root failure\"\n",
    )
    .expect("compile root error timer source");
    let mut vm = Vm::new();

    let started = std::time::Instant::now();
    let error = vm
        .run_named(&program, "main")
        .expect_err("the root body must retain its failure");
    assert_eq!(error.message, "uncaught throw: root failure");
    assert_eq!(vm.global("fired"), Some(Value::Bool(false)));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn select_removes_losing_channel_waiters_before_the_next_send() {
    let program = compile(
        "select-winner.slug",
        "val left = make_channel(0)\n\
         val right = make_channel(0)\n\
         val sender = spawn { select { send left, 11 }; select { send right, 12 } }\n\
         val first = select {\n\
           recv left\n\
           recv right\n\
         }\n\
         val second = select { recv right }\n\
         select { await sender }\n\
         first + second\n",
    )
    .expect("compile select winner source");

    assert_eq!(
        vm_with_channel_constructor()
            .run_named(&program, "main")
            .unwrap(),
        Value::Int(23)
    );
}

#[test]
fn cancellation_removes_select_channel_and_timer_waiters() {
    let program = compile(
        "select-cancellation.slug",
        "val inbox = make_channel(0)\n\
         val attempt = fn() {\n\
           defer onerror(err) { nil }\n\
           nursery {\n\
             spawn { select { recv inbox; after 10 } }\n\
             spawn { throw \"fail\" }\n\
           }\n\
         }\n\
         attempt()\n\
         val sender = spawn { select { send inbox, 42 } }\n\
         select { await sender }\n",
    )
    .expect("compile select cancellation source");

    let error = vm_with_channel_constructor()
        .run_named(&program, "main")
        .expect_err("cancelled select must not receive a later send");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.message, "task remains blocked with no runnable work");
}

#[test]
fn malformed_private_select_bytecode_returns_checked_errors() {
    let mut missing_operand = Chunk::new("main", 0);
    missing_operand
        .emit(Op::Select(vec![SelectCase::Receive { has_handler: false }]))
        .emit(Op::Return);
    let error = Vm::new()
        .run(&program_with_main(missing_operand), 0)
        .expect_err("select without its channel operand must fail");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);

    let mut malformed_result = Chunk::new("main", 0);
    malformed_result
        .emit(Op::Nil)
        .emit(Op::SelectApply)
        .emit(Op::Return);
    let error = Vm::new()
        .run(&program_with_main(malformed_result), 0)
        .expect_err("select apply without a select result must fail");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
}

#[test]
fn native_channel_pair_delivers_owned_mailbox_values_on_the_vm_thread() {
    fn channel_with_event(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        assert_eq!(
            producer.try_send(slug_vm::NativeSendValue::integer(42)),
            slug_vm::NativeProducerStatus::Sent
        );
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer", ()).unwrap();
    let function = module
        .function(
            "channel_with_event",
            NativeArity::Exact(0),
            channel_with_event,
        )
        .unwrap();
    let program = compile(
        "native-producer.slug",
        "val channel = channel_with_event()\nselect { recv channel }\n",
    )
    .expect("compile native producer source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(42));
}

#[test]
fn closing_a_native_producer_drains_events_then_closes_its_receiver() {
    fn closed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        assert_eq!(
            producer.try_send(slug_vm::NativeSendValue::integer(7)),
            slug_vm::NativeProducerStatus::Sent
        );
        producer.close();
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer_close", ()).unwrap();
    let function = module
        .function("closed_channel", NativeArity::Exact(0), closed_channel)
        .unwrap();
    let program = compile(
        "native-producer-close.slug",
        "val channel = closed_channel()\nselect { recv channel } + if (select { recv channel }) { 1 } else { 0 }\n",
    )
    .expect("compile native producer close source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(7));
}

#[test]
fn a_closed_native_channel_does_not_keep_tasks_from_running() {
    fn closed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        assert_eq!(
            producer.try_send(slug_vm::NativeSendValue::integer(7)),
            slug_vm::NativeProducerStatus::Sent
        );
        producer.close();
        call.return_value(channel)
    }
    fn later_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let _ = producer.try_send(slug_vm::NativeSendValue::integer(35));
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer_closed_progress", ()).unwrap();
    let closed = module
        .function("closed_channel", NativeArity::Exact(0), closed_channel)
        .unwrap();
    let later = module
        .function("later_channel", NativeArity::Exact(0), later_channel)
        .unwrap();
    // Once one channel has closed, a task waiting on another must still run
    // when its value arrives; a closed channel counted as progress on every
    // round would keep the root, waiting inside `main`, spinning instead.
    let program = compile(
        "native-producer-closed-progress.slug",
        "val main = fn() {\n\
             val channel = closed_channel()\n\
             val first = select { recv channel }\n\
             val second = later_channel()\n\
             val task = spawn { select { recv second } }\n\
             first + select { await task }\n\
         }\n",
    )
    .expect("compile closed native channel source");
    let mut vm = Vm::new();
    vm.define_native(closed).unwrap();
    vm.define_native(later).unwrap();
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(42));
}

#[test]
fn native_and_slug_senders_share_a_channel_buffer_bound() {
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

    fn send_native(call: &mut NativeCall<'_>) -> NativeStatus {
        let producer = call
            .state::<Arc<ProducerState>>()
            .expect("producer state")
            .0
            .lock()
            .expect("producer state lock")
            .clone()
            .expect("channel producer");
        let status = producer.try_send(slug_vm::NativeSendValue::integer(2));
        call.return_value(NativeOwnedValue::boolean(matches!(
            status,
            slug_vm::NativeProducerStatus::Sent
        )))
    }

    let module = NativeModule::new(
        "test.producer_capacity",
        Arc::new(ProducerState(Mutex::new(None))),
    )
    .unwrap();
    let create = module
        .function("create_channel", NativeArity::Exact(0), create_channel)
        .unwrap();
    let send = module
        .function("send_native", NativeArity::Exact(0), send_native)
        .unwrap();
    let program = compile(
        "native-producer-capacity.slug",
        "val channel = create_channel()\n\
         select { send channel, 1 }\n\
         if (send_native()) { 99 } else { select { recv channel } }\n",
    )
    .expect("compile native producer capacity source");
    let mut vm = Vm::new();
    vm.define_native(create).unwrap();
    vm.define_native(send).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(1));
}

#[test]
fn closing_a_native_producer_rejects_parked_slug_senders() {
    fn delayed_close(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            producer.close();
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer_sender_close", ()).unwrap();
    let function = module
        .function("delayed_close", NativeArity::Exact(0), delayed_close)
        .unwrap();
    let program = compile(
        "native-producer-sender-close.slug",
        "val channel = delayed_close()\n\
         select { send channel, 1 }\n\
         val sender = spawn { select { send channel, 2 } }\n\
         select { await sender }\n",
    )
    .expect("compile native producer sender-close source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    let error = vm
        .run_named(&program, "main")
        .expect_err("native close must reject the parked sender");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.message, "send on a closed channel");
}

#[test]
fn a_closed_native_producer_rejects_the_next_slug_send() {
    fn closed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        producer.close();
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer_closed_send", ()).unwrap();
    let function = module
        .function("closed_channel", NativeArity::Exact(0), closed_channel)
        .unwrap();
    let program = compile(
        "native-producer-closed-send.slug",
        "val channel = closed_channel()\nselect { send channel, 1 }\n",
    )
    .expect("compile native producer closed-send source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    let error = vm
        .run_named(&program, "main")
        .expect_err("closed native channel must reject sends");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.message, "send on a closed channel");
}

#[test]
fn a_foreign_thread_wakes_a_root_parked_on_a_native_channel() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert_eq!(
                producer.try_send(slug_vm::NativeSendValue::integer(42)),
                slug_vm::NativeProducerStatus::Sent
            );
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.producer_wake", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "native-producer-wake.slug",
        "val channel = delayed_channel()\nselect { recv channel }\n",
    )
    .expect("compile native producer wake source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(42));
}

#[test]
fn a_foreign_thread_wakes_an_explicit_nursery_body_parked_on_a_native_channel() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert_eq!(
                producer.try_send(slug_vm::NativeSendValue::integer(42)),
                slug_vm::NativeProducerStatus::Sent
            );
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.nursery_producer_wake", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "nursery-native-producer-wake.slug",
        "val channel = delayed_channel()\nnursery { select { recv channel } }\n",
    )
    .expect("compile explicit nursery native producer source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(42));
}

#[test]
fn a_foreign_thread_wakes_a_root_settling_a_task_on_a_native_channel() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert_eq!(
                producer.try_send(slug_vm::NativeSendValue::integer(42)),
                slug_vm::NativeProducerStatus::Sent
            );
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.root_settlement_producer_wake", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "root-settlement-native-producer-wake.slug",
        "val channel = delayed_channel()\nspawn { select { recv channel } }\nnil\n",
    )
    .expect("compile root settlement native producer source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Nil);
}

#[test]
fn a_closed_foreign_producer_wakes_a_task_settling_at_the_root() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(5));
            producer.close();
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.root_settlement_producer_close", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "root-settlement-native-producer-close.slug",
        "val channel = delayed_channel()\nspawn { if (select { recv channel }) { 1 } else { 42 } }\nnil\n",
    )
    .expect("compile root settlement native producer closure source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Nil);
}

#[test]
fn a_late_native_event_wakes_without_a_fixed_polling_deadline() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            assert_eq!(
                producer.try_send(slug_vm::NativeSendValue::integer(42)),
                slug_vm::NativeProducerStatus::Sent
            );
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.late_producer_wake", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "late-native-producer-wake.slug",
        "val channel = delayed_channel()\nselect { recv channel }\n",
    )
    .expect("compile late native producer wake source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(42));
}

#[test]
fn a_native_event_can_win_before_a_later_select_timer() {
    fn delayed_channel(call: &mut NativeCall<'_>) -> NativeStatus {
        let (channel, producer) = call.channel(1);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            assert_eq!(
                producer.try_send(slug_vm::NativeSendValue::integer(7)),
                slug_vm::NativeProducerStatus::Sent
            );
        });
        call.return_value(channel)
    }

    let module = NativeModule::new("test.native_select_wake", ()).unwrap();
    let function = module
        .function("delayed_channel", NativeArity::Exact(0), delayed_channel)
        .unwrap();
    let program = compile(
        "native-select-wake.slug",
        "val channel = delayed_channel()\n\
         select {\n\
           recv channel /> fn(value) { value }\n\
           after 1000 /> fn(unused) { 99 }\n\
         }\n",
    )
    .expect("compile native select wake source");
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(7));
}

/// Producers minted from Slug-created channels, as typed native bindings use
/// them: `attach(channel)` mints one and keeps it in module state.
mod minted_producers {
    use super::*;
    use slug_vm::{NativeChannelProducer, NativeProducerStatus, NativeSendValue};

    /// Values the closing-race worker sends before it closes.
    const RACE_SENDS: i64 = 2;

    #[derive(Default)]
    struct Minted {
        producers: Mutex<Vec<NativeChannelProducer>>,
        statuses: Mutex<Vec<String>>,
        threaded: bool,
        close_after_mint: bool,
        /// The worker thread closes the channel this long after its sends.
        close_after: Option<std::time::Duration>,
    }

    fn status_name(status: &NativeProducerStatus) -> String {
        match status {
            NativeProducerStatus::Sent => "sent".into(),
            NativeProducerStatus::Full(_) => "full".into(),
            NativeProducerStatus::Closed(_) => "closed".into(),
        }
    }

    fn attach(call: &mut NativeCall<'_>) -> NativeStatus {
        let producer = match call.producer(0) {
            Ok(producer) => producer,
            Err(error) => return call.raise(error),
        };
        let state = call.state::<Arc<Minted>>().expect("minted state").clone();
        if state.threaded {
            let worker = producer.clone();
            let state = state.clone();
            std::thread::spawn(move || {
                let values = if state.close_after.is_some() {
                    (0..RACE_SENDS).map(NativeSendValue::integer).collect()
                } else {
                    vec![
                        NativeSendValue::string("hello"),
                        NativeSendValue::integer(7),
                    ]
                };
                for value in values {
                    let mut value = value;
                    loop {
                        match worker.try_send(value) {
                            NativeProducerStatus::Sent => break,
                            NativeProducerStatus::Full(rejected) => {
                                value = rejected;
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                            NativeProducerStatus::Closed(_) => return,
                        }
                    }
                }
                if let Some(delay) = state.close_after {
                    let started = std::time::Instant::now();
                    while started.elapsed() < delay {
                        std::hint::spin_loop();
                    }
                    worker.close();
                }
            });
        } else {
            let mut statuses = state.statuses.lock().unwrap();
            statuses.push(status_name(&producer.try_send(NativeSendValue::nil())));
            statuses.push(status_name(
                &producer.try_send(NativeSendValue::boolean(true)),
            ));
        }
        if state.close_after_mint {
            producer.close();
        }
        state.producers.lock().unwrap().push(producer);
        call.return_value(slug_vm::NativeOwnedValue::nil())
    }

    fn vm_with_attach(state: Arc<Minted>) -> Vm {
        let mut vm = vm_with_channel_constructor();
        let module = NativeModule::new("test.minted", state).unwrap();
        vm.define_native(
            module
                .function("attach", NativeArity::Exact(1), attach)
                .unwrap(),
        )
        .unwrap();
        vm
    }

    #[test]
    fn a_producer_feeds_a_slug_channel_from_another_thread() {
        let state = Arc::new(Minted {
            threaded: true,
            ..Minted::default()
        });
        let program = compile(
            "minted-threaded.slug",
            "val channel = make_channel(1)\n\
             attach(channel)\n\
             val first = select { recv channel }\n\
             val second = select { recv channel }\n\
             [first, second]\n",
        )
        .expect("compile threaded producer source");
        let value = vm_with_attach(state)
            .run_named(&program, "main")
            .expect("receive from the worker thread");
        assert_eq!(value.to_string(), "[\"hello\", 7]");
    }

    #[test]
    fn a_receiver_sees_sends_that_race_with_a_closing_producer() {
        // The worker sends and closes while the VM may be between draining
        // the channel and checking for a live producer; the VM must still
        // deliver every value and the close rather than report a blocked task.
        let program = compile(
            "minted-closing-race.slug",
            "val channel = make_channel(1)\n\
             attach(channel)\n\
             val count = fn(total) {\n\
               match select { recv channel } {\n\
                 nil => total\n\
                 _ => recur(total + 1)\n\
               }\n\
             }\n\
             count(0)\n",
        )
        .expect("compile closing race source");
        // Sweep the close across the VM's wake-up and drain.
        for micros in 0..400 {
            let state = Arc::new(Minted {
                threaded: true,
                close_after: Some(std::time::Duration::from_micros(micros)),
                ..Minted::default()
            });
            let value = vm_with_attach(state)
                .run_named(&program, "main")
                .expect("receive every value before the close");
            assert_eq!(value.to_string(), RACE_SENDS.to_string());
        }
    }

    #[test]
    fn a_producer_shares_the_channel_capacity_and_closes_with_the_vm() {
        let state = Arc::new(Minted::default());
        let program = compile(
            "minted-capacity.slug",
            "val channel = make_channel(2)\n\
             select { send channel, 1 }\n\
             attach(channel)\n\
             val a = select { recv channel }\n\
             val b = select { recv channel }\n\
             val c = select { recv channel }\n\
             [a, b, c]\n",
        )
        .expect("compile capacity source");
        let mut vm = vm_with_attach(state.clone());
        vm.start_named(&program, "main")
            .expect("start capacity program");
        assert!(matches!(vm.run_until_stalled(), VmProgress::Stalled));
        // The buffered Slug value occupied one of two slots when the producer
        // was minted, so the first native send fit and the second did not.
        assert_eq!(*state.statuses.lock().unwrap(), ["sent", "full"]);

        let producer = state.producers.lock().unwrap()[0].clone();
        assert_eq!(
            producer.try_send(NativeSendValue::float(1.5)),
            NativeProducerStatus::Sent
        );
        assert!(matches!(
            vm.run_until_stalled(),
            VmProgress::Completed(value) if value.to_string() == "[1, nil, 1.5]"
        ));
        drop(vm);
        assert!(matches!(
            producer.try_send(NativeSendValue::integer(9)),
            NativeProducerStatus::Closed(_)
        ));
    }

    #[test]
    fn closing_a_producer_closes_the_slug_channel() {
        let state = Arc::new(Minted {
            close_after_mint: true,
            ..Minted::default()
        });
        let program = compile(
            "minted-close.slug",
            "val channel = make_channel(2)\n\
             attach(channel)\n\
             val first = select { recv channel }\n\
             val second = select { recv channel }\n\
             val after = select { recv channel }\n\
             [first, second, after]\n",
        )
        .expect("compile close source");
        let value = vm_with_attach(state)
            .run_named(&program, "main")
            .expect("receive until the producer closes");
        // Both values sent before close are delivered, then the closed
        // channel yields nil.
        assert_eq!(value.to_string(), "[nil, true, nil]");
    }

    #[test]
    fn an_unbuffered_channel_cannot_mint_a_producer() {
        let program = compile("minted-unbuffered.slug", "attach(make_channel(0))\n")
            .expect("compile unbuffered source");
        let error = vm_with_attach(Arc::new(Minted::default()))
            .run_named(&program, "main")
            .expect_err("an unbuffered channel is rejected");
        assert_eq!(
            error.native.as_ref().map(|native| native.code.as_str()),
            Some("native.type")
        );
        assert!(error.message.contains("buffered channel"), "{error}");
    }
}

// Fork behavior that needs native functions; the source-level behavior is in
// `tests/conformance/fork`. Both wait for phase 2 of
// `docs/planning/fork-tasks.md`.

#[test]
fn only_a_module_with_thread_safe_state_declares_functions_thread_safe() {
    fn answer(call: &mut NativeCall<'_>) -> NativeStatus {
        call.return_value(NativeOwnedValue::integer(42))
    }
    // State a fork on another thread must not read.
    let local = NativeModule::new("test.local_state", std::rc::Rc::new(Cell::new(0))).unwrap();
    let function = local
        .function("answer", NativeArity::Exact(0), answer)
        .unwrap();
    let error = function
        .declare_thread_safe()
        .expect_err("local state cannot be shared");
    assert!(error.to_string().contains("new_thread_safe"), "{error}");
    assert!(!function.is_thread_safe());

    let shared = NativeModule::new_thread_safe("test.shared_state", ()).unwrap();
    let function = shared
        .function("answer", NativeArity::Exact(0), answer)
        .unwrap();
    function.declare_thread_safe().unwrap();
    let mut vm = Vm::new();
    vm.define_native(function).unwrap();
    let program = compile(
        "fork-thread-safe-native.slug",
        "val main = fn() {\n    val t = fork { answer() }\n    select { await t }\n}\n",
    )
    .expect("compile thread-safe native source");
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(42));
}

#[test]
fn a_fork_cannot_call_a_native_function_not_declared_thread_safe() {
    fn answer(call: &mut NativeCall<'_>) -> NativeStatus {
        call.return_value(NativeOwnedValue::integer(42))
    }
    let module = NativeModule::new("test.fork_natives", ()).unwrap();
    let mut vm = Vm::new();
    vm.define_native(
        module
            .function("answer", NativeArity::Exact(0), answer)
            .unwrap(),
    )
    .unwrap();
    let program = compile(
        "fork-native.slug",
        "val main = fn() {\n    val direct = answer()\n    val t = fork { answer() }\n    direct + select { await t }\n}\n",
    )
    .expect("compile fork native source");
    let error = vm
        .run_program(&program)
        .expect_err("a fork calling an undeclared native fails");
    assert!(
        error.message.contains("`answer`") && error.message.contains("thread-safe"),
        "{}",
        error.message
    );
}

#[test]
fn a_fork_can_hold_its_spawners_resource_but_cannot_return_it() {
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed, destroyed);
    let mut vm = Vm::new();
    native_resource_fixture::install(&mut vm, &module);
    let holds = compile(
        "fork-holds-resource.slug",
        "val main = fn() {\n    val r = make_resource()\n    val t = fork { val kept = r; 1 }\n    select { await t }\n}\n",
    )
    .expect("compile holding source");
    assert_eq!(vm.run_program(&holds).unwrap(), Value::Int(1));
    let returns = compile(
        "fork-returns-resource.slug",
        "val main = fn() {\n    val r = make_resource()\n    val t = fork { r }\n    select { await t }\n}\n",
    )
    .expect("compile returning source");
    let error = vm
        .run_program(&returns)
        .expect_err("a resource is not sendable");
    assert!(error.message.contains("sendable"), "{}", error.message);
}

/// A host that answers one module import, `served`, and records that it did.
#[cfg(feature = "threads")]
struct ServingHost {
    configuration: slug_vm::EmptyVmConfiguration,
    native_resources: slug_vm::NativeResourceRegistry,
}

#[cfg(feature = "threads")]
static IMPORT_SERVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "threads")]
impl slug_vm::VmHost for ServingHost {
    fn import_module(
        &self,
        _importer: Option<&str>,
        name: &str,
    ) -> Result<slug_vm::VmModuleExports, slug_vm::VmHostError> {
        if name != "served" {
            return Err(slug_vm::VmHostError::not_found(format!("missing {name}")));
        }
        IMPORT_SERVED.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(slug_vm::VmModuleExports {
            exports: Value::Map(vec![(Value::string("answer"), Value::Int(1))].into()),
        })
    }

    fn module_initialized(&self, _importer: Option<&str>, _name: &str) -> Option<bool> {
        Some(true)
    }

    fn builtin_globals(&self) -> std::collections::HashMap<String, Value> {
        std::collections::HashMap::new()
    }

    fn native_globals(&self) -> std::collections::HashMap<String, Value> {
        std::collections::HashMap::new()
    }

    fn foreign_function(&self, _module: &str, _name: &str) -> Option<slug_vm::NativeFunction> {
        None
    }

    fn define_foreign_batch(
        &self,
        _functions: Vec<slug_vm::NativeFunction>,
    ) -> Result<(), slug_vm::NativeDescriptorError> {
        Ok(())
    }

    fn define_native_global(&self, _name: String, _value: Value) {}

    fn native_resources(&self) -> slug_vm::NativeResourceRegistry {
        self.native_resources.clone()
    }

    fn module_variables(&self) -> slug_vm::ModuleVariables {
        slug_vm::module_variables()
    }

    fn configuration(&self) -> &dyn slug_vm::VmConfiguration {
        &self.configuration
    }

    fn warn(&self, _message: String) {}

    fn shutdown(&self) {}
}

/// A fork's host call runs on its spawner's thread. A spawner busy computing,
/// which never waits, still serves it at its next call or `recur`: here the
/// spawner spins until it sees the fork's import served, and gives up after
/// far more iterations than that takes.
#[cfg(feature = "threads")]
#[test]
fn a_busy_spawner_serves_its_forks_host_calls() {
    fn served(call: &mut NativeCall<'_>) -> NativeStatus {
        call.return_value(NativeOwnedValue::boolean(
            IMPORT_SERVED.load(std::sync::atomic::Ordering::SeqCst),
        ))
    }
    let module = NativeModule::new("test.serving", ()).unwrap();
    let host: slug_vm::Shared<dyn slug_vm::VmHost> = slug_vm::Shared::new(ServingHost {
        configuration: slug_vm::EmptyVmConfiguration,
        native_resources: slug_vm::NativeResourceRegistry::new(),
    });
    let mut vm = Vm::with_host(host);
    vm.define_native(
        module
            .function("served", NativeArity::Exact(0), served)
            .unwrap(),
    )
    .unwrap();
    let program = compile(
        "busy-spawner.slug",
        "val main = fn() {\n    val t = fork { import(\"served\").answer }\n    val spin = fn(i:num):num { if (served()) { i } else { if (i > 50000000) { i } else { recur(i + 1) } } }\n    val spun = spin(0)\n    if (spun > 50000000) { -1 } else { select { await t } }\n}\n",
    )
    .expect("compile busy spawner source");
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(1));
}

/// A fork's island counts and samples on its own, and its counts and
/// samples join its spawner's when it settles: a fork's work shows up in
/// `metrics` and the profile as the same work in a spawned task does.
#[cfg(all(feature = "threads", feature = "metrics"))]
#[test]
fn a_forks_metrics_and_samples_join_its_spawners() {
    let source = |start: &str| {
        format!(
            "val spin = fn(i:num, total:num):num {{ if (i >= 20000) {{ total }} else {{ recur(i + 1, total + i) }} }}\nval main = fn() {{\n    val t = {start} {{ spin(0, 0) }}\n    select {{ await t }}\n}}\n"
        )
    };
    let run = |start: &str| {
        let program = compile("counted.slug", &source(start)).expect("compile counted source");
        let mut vm = Vm::new();
        vm.enable_profile();
        assert_eq!(vm.run_program(&program).unwrap(), Value::Int(199_990_000));
        let samples: usize = vm.take_profile().expect("profiling").values().sum();
        (vm.metrics(), samples)
    };
    let (spawned, spawned_samples) = run("spawn");
    let (forked, forked_samples) = run("fork");
    // The loop is nearly all of either run.
    assert!(spawned.instructions_executed > 100_000, "{spawned:?}");
    let within = |a: usize, b: usize| a.abs_diff(b) * 20 < b;
    assert!(
        within(forked.instructions_executed, spawned.instructions_executed),
        "fork {} against spawn {}",
        forked.instructions_executed,
        spawned.instructions_executed
    );
    assert_eq!(
        forked.exact_positional_recur_restarts,
        spawned.exact_positional_recur_restarts
    );
    assert!(
        within(forked_samples, spawned_samples),
        "fork {forked_samples} samples against spawn {spawned_samples}"
    );
}

#[test]
#[cfg(all(feature = "metrics", feature = "threads"))]
fn an_admitted_fork_leaves_for_a_worker_without_a_ready_queue_entry() {
    let program = compile(
        "fork-dispatch.slug",
        "val main = fn() {\n    nursery {\n        fork { 1 }\n        fork { 2 }\n        fork { 3 }\n        fork { 4 }\n        0\n    }\n}\n",
    )
    .expect("compile fork dispatch source");
    let mut vm = Vm::new();
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(0));
    // Only the nursery's body waits in the queue: each fork went straight
    // to a worker.
    assert_eq!(vm.metrics().peak_ready_queue, 1);
}

#[test]
fn a_nursery_lets_go_of_tasks_that_have_settled() {
    // A server starts a task per connection: once one settles with nothing
    // to report, its nursery must not keep it, or its result, alive.
    fn destroyed_so_far(call: &mut NativeCall<'_>) -> NativeStatus {
        let count = call.state::<Rc<Cell<usize>>>().unwrap().get();
        call.return_value(NativeOwnedValue::integer(i64::try_from(count).unwrap()))
    }
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed, destroyed.clone());
    let mut vm = Vm::new();
    native_resource_fixture::install(&mut vm, &module);
    let counter = NativeModule::new("test.counter", destroyed).unwrap();
    vm.define_native(
        counter
            .function("destroyed_so_far", NativeArity::Exact(0), destroyed_so_far)
            .unwrap(),
    )
    .unwrap();
    let program = compile(
        "settled-tasks.slug",
        "val run = fn(i:num) { if (i < 300) { val t = spawn { make_resource() }; select { await t }; recur(i + 1) } }\n\
         run(0)\n\
         destroyed_so_far()\n",
    )
    .expect("compile settled tasks source");
    let Value::Int(destroyed) = vm.run_program(&program).unwrap() else {
        panic!("expected a count");
    };
    assert!(
        destroyed >= 200,
        "only {destroyed} of 300 results were released"
    );
}

/// A thread-safe module with a resource type, as a typed clutch declaring
/// `thread_safe = true` on a function that takes a handle would make.
#[cfg(feature = "threads")]
mod owned_resources {
    use std::sync::{Arc, Mutex, OnceLock};

    use super::*;

    pub struct Payload;

    type Kind = Arc<OnceLock<NativeResourceType<Payload>>>;
    type Callback = for<'call> fn(&mut NativeCall<'call>) -> NativeStatus;

    /// Where each payload was destroyed.
    pub static DESTROYED_ON: Mutex<Vec<std::thread::ThreadId>> = Mutex::new(Vec::new());

    fn destroy(_: Payload) {
        DESTROYED_ON
            .lock()
            .unwrap()
            .push(std::thread::current().id());
    }

    fn kind(call: &NativeCall<'_>) -> NativeResourceType<Payload> {
        call.state::<Kind>().unwrap().get().unwrap().clone()
    }

    fn make_it(call: &mut NativeCall<'_>) -> NativeStatus {
        let kind = kind(call);
        let resource = call.resource(&kind, Payload).unwrap();
        call.return_value(resource)
    }

    fn use_it(call: &mut NativeCall<'_>) -> NativeStatus {
        let kind = kind(call);
        match call.with_resource(0, &kind, |_| 7) {
            Ok(value) => call.return_value(NativeOwnedValue::integer(value)),
            Err(error) => call.raise(error),
        }
    }

    pub fn vm() -> Vm {
        let kind: Kind = Arc::default();
        let module = NativeModule::new_thread_safe("test.owned", kind.clone()).unwrap();
        kind.set(module.resource_type("thing", |_| {}, destroy).unwrap())
            .ok()
            .unwrap();
        let mut vm = Vm::new();
        let callbacks: [(&str, NativeArity, Callback); 2] = [
            ("make_it", NativeArity::Exact(0), make_it),
            ("use_it", NativeArity::Exact(1), use_it),
        ];
        for (name, arity, callback) in callbacks {
            let function = module.function(name, arity, callback).unwrap();
            function.declare_thread_safe().unwrap();
            vm.define_native(function).unwrap();
        }
        vm
    }
}

// One test, as both programs record where payloads are destroyed.
#[test]
#[cfg(feature = "threads")]
fn a_fork_holds_but_cannot_use_its_spawners_resource_whose_thread_destroys_it() {
    let mut vm = owned_resources::vm();
    let uses = compile(
        "fork-uses-resource.slug",
        "val main = fn() {\n    val r = make_it()\n    val direct = use_it(r)\n    val own = select { await fork { use_it(make_it()) } }\n    val t = fork { use_it(r) }\n    [direct, own, select { await t }]\n}\n",
    )
    .expect("compile resource owner source");
    let error = vm
        .run_program(&uses)
        .expect_err("a fork using its spawner's resource fails");
    assert!(
        error
            .message
            .contains("made outside this fork, which may hold it but not use it"),
        "{}",
        error.message
    );

    // `start` returns before its fork ends, so the fork's copy of the
    // resource is the last; the spawner's thread still destroys it.
    let drops = compile(
        "fork-drops-resource.slug",
        "val start = fn() {\n    val r = make_it()\n    fork { val kept = r; select { after 50 }; 1 }\n}\nval main = fn() {\n    val t = start()\n    select { await t }\n    select { after 50 }\n}\n",
    )
    .expect("compile resource drop source");
    owned_resources::DESTROYED_ON.lock().unwrap().clear();
    vm.run_program(&drops).unwrap();
    drop(vm);
    let destroyed = owned_resources::DESTROYED_ON.lock().unwrap().clone();
    assert_eq!(destroyed, vec![std::thread::current().id()]);
}
