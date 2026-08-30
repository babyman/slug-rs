use super::*;

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
        "var held = nil\n\
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
fn a_failed_root_body_joins_runnable_owned_tasks() {
    let program = compile(
        "root-error-settlement.slug",
        "var child_ran = false\n\
         spawn { child_ran = true }\n\
         throw \"root failure\"\n",
    )
    .expect("compile root error settlement source");
    let mut vm = Vm::new();

    let error = vm
        .run_named(&program, "main")
        .expect_err("the root body must retain its failure");
    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(error.message, "uncaught throw: root failure");
    assert_eq!(vm.global("child_ran"), Some(Value::Bool(true)));
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
