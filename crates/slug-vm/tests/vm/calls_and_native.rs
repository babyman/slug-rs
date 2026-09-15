use super::*;

#[test]
fn branches_without_evaluating_the_other_arm() {
    let mut main = Chunk::new("main", 0);
    let then_value = main.constant(Value::Int(1));
    let else_value = main.constant(Value::Int(2));
    main.emit(Op::False)
        .emit(Op::JumpIfFalse(5))
        .emit(Op::Pop)
        .emit(Op::Constant(then_value))
        .emit(Op::Jump(7))
        .emit(Op::Pop)
        .emit(Op::Constant(else_value))
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(2)
    );
}

#[test]
fn calls_closures_and_preserves_captured_values() {
    let mut inner = Chunk::new("inner", 0);
    let two = inner.constant(Value::Int(2));
    inner
        .emit(Op::GetCapture(0))
        .emit(Op::Constant(two))
        .emit(Op::Add)
        .emit(Op::Return);

    let mut factory = Chunk::new("factory", 1);
    factory
        .emit(Op::MakeClosure {
            chunk: 0,
            captures: vec![Capture::Local(0)],
        })
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    let forty = main.constant(Value::Int(40));
    main.emit(Op::MakeClosure {
        chunk: 1,
        captures: vec![],
    })
    .emit(Op::Constant(forty))
    .emit(Op::Call(1))
    .emit(Op::Call(0))
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(inner);
    program.add_chunk(factory);
    program.add_chunk(main);
    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(42)
    );
}

#[test]
fn closures_share_mutable_captures() {
    let mut counter = Chunk::new("counter", 0);
    let one = counter.constant(Value::Int(1));
    counter
        .emit(Op::GetCapture(0))
        .emit(Op::Constant(one))
        .emit(Op::Add)
        .emit(Op::SetCapture(0))
        .emit(Op::GetCapture(0))
        .emit(Op::Return);

    let mut factory = Chunk::new("factory", 0);
    factory.locals = 1;
    let zero = factory.constant(Value::Int(0));
    factory
        .emit(Op::Constant(zero))
        .emit(Op::SetLocal(0))
        .emit(Op::MakeClosure {
            chunk: 0,
            captures: vec![Capture::Local(0)],
        })
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::MakeClosure {
        chunk: 1,
        captures: vec![],
    })
    .emit(Op::Call(0))
    .emit(Op::DefineGlobal("counter".into()))
    .emit(Op::GetGlobal("counter".into()))
    .emit(Op::Call(0))
    .emit(Op::Pop)
    .emit(Op::GetGlobal("counter".into()))
    .emit(Op::Call(0))
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(counter);
    program.add_chunk(factory);
    program.add_chunk(main);
    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(2)
    );
}

#[test]
fn keeps_globals_across_runs_and_calls_explicit_native_functions() {
    fn double(call: &mut NativeCall<'_>) -> NativeStatus {
        let value = match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
            Ok(value) => value,
            Err(error) => return call.raise(error),
        };
        call.return_value(NativeOwnedValue::integer(value * 2))
    }

    let mut main = Chunk::new("main", 0);
    let twenty_one = main.constant(Value::Int(21));
    main.emit(Op::GetGlobal("double".into()))
        .emit(Op::Constant(twenty_one))
        .emit(Op::Call(1))
        .emit(Op::DefineGlobal("answer".into()))
        .emit(Op::GetGlobal("answer".into()))
        .emit(Op::Return);
    let program = program_with_main(main);
    let mut vm = Vm::new();
    let module = NativeModule::new("test.math", ()).unwrap();
    vm.define_native(
        module
            .function("double", NativeArity::Exact(1), double)
            .unwrap(),
    )
    .unwrap();

    assert_eq!(vm.run(&program, 0).unwrap(), Value::Int(42));
    assert_eq!(vm.global("answer"), Some(Value::Int(42)));
}

#[test]
fn reports_checked_native_conversion_and_structured_errors() {
    fn require_integer(call: &mut NativeCall<'_>) -> NativeStatus {
        let value = match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
            Ok(value) => value,
            Err(error) => return call.raise(error),
        };
        call.return_value(NativeOwnedValue::integer(value))
    }

    fn fail(call: &mut NativeCall<'_>) -> NativeStatus {
        call.raise(
            NativeError::new("test.failure", "deliberate failure")
                .with_data(NativeOwnedValue::integer(42)),
        )
    }

    let module = NativeModule::new("test.errors", ()).unwrap();
    let mut vm = Vm::new();
    vm.define_native(
        module
            .function("require_integer", NativeArity::Exact(1), require_integer)
            .unwrap(),
    )
    .unwrap();
    vm.define_native(
        module
            .function("fail", NativeArity::Exact(0), fail)
            .unwrap(),
    )
    .unwrap();

    let mut wrong_type = Chunk::new("wrong_type", 0);
    let text = wrong_type.constant(Value::string("not an integer"));
    wrong_type
        .emit(Op::GetGlobal("require_integer".into()))
        .emit(Op::Constant(text))
        .emit(Op::Call(1))
        .emit(Op::Return);
    let error = vm
        .run(&program_with_main(wrong_type), 0)
        .expect_err("native conversion should fail");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("native.type")
    );

    let mut structured = Chunk::new("structured", 0);
    structured
        .emit(Op::GetGlobal("fail".into()))
        .emit(Op::Call(0))
        .emit(Op::Return);
    let error = vm
        .run(&program_with_main(structured), 0)
        .expect_err("native error should fail");
    let native = error.native.as_deref().expect("structured native error");
    assert_eq!(native.code, "test.failure");
    assert_eq!(native.data, Some(Value::Int(42)));
}

#[test]
fn contains_native_panics_and_callback_contract_violations() {
    fn panics(_: &mut NativeCall<'_>) -> NativeStatus {
        panic!("must not cross the native boundary")
    }

    fn omits_result(_: &mut NativeCall<'_>) -> NativeStatus {
        NativeStatus::Ok
    }

    fn sets_two_results(call: &mut NativeCall<'_>) -> NativeStatus {
        call.set_result(NativeOwnedValue::nil());
        call.set_result(NativeOwnedValue::integer(1));
        NativeStatus::Ok
    }

    fn calls(name: &str) -> Program {
        let mut main = Chunk::new("main", 0);
        main.emit(Op::GetGlobal(name.into()))
            .emit(Op::Call(0))
            .emit(Op::Return);
        program_with_main(main)
    }

    let module = NativeModule::new("test.contract", ()).unwrap();
    let mut vm = Vm::new();
    vm.define_native(
        module
            .function("panics", NativeArity::Exact(0), panics)
            .unwrap(),
    )
    .unwrap();
    vm.define_native(
        module
            .function("omits_result", NativeArity::Exact(0), omits_result)
            .unwrap(),
    )
    .unwrap();
    vm.define_native(
        module
            .function("sets_two_results", NativeArity::Exact(0), sets_two_results)
            .unwrap(),
    )
    .unwrap();

    let error = vm
        .run(&calls("panics"), 0)
        .expect_err("panic should become a checked error");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(error.message.contains("panicked"));

    let error = vm
        .run(&calls("omits_result"), 0)
        .expect_err("missing result should become a checked error");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(error.message.contains("without a result"));

    let error = vm
        .run(&calls("sets_two_results"), 0)
        .expect_err("multiple outcomes should become a checked error");
    assert_eq!(error.kind, RuntimeErrorKind::NativeContract);
    assert!(error.message.contains("more than one outcome"));
}

#[test]
fn validates_native_resource_types_and_closes_resources_on_teardown() {
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed.clone(), destroyed.clone());
    let mut vm = Vm::new();
    native_resource_fixture::install(&mut vm, &module);

    let mut wrong = Chunk::new("wrong", 0);
    wrong
        .emit(Op::GetGlobal("wrong_resource".into()))
        .emit(Op::GetGlobal("make_resource".into()))
        .emit(Op::Call(0))
        .emit(Op::Call(1))
        .emit(Op::Return);
    let error = vm
        .run(&program_with_main(wrong), 0)
        .expect_err("resource type mismatch should fail");
    assert_eq!(
        error.native.as_ref().map(|error| error.code.as_str()),
        Some("native.resource_type")
    );
    assert_eq!(closed.get(), 0);

    let mut explicit_close = Chunk::new("explicit_close", 0);
    explicit_close
        .emit(Op::GetGlobal("close_resource".into()))
        .emit(Op::GetGlobal("make_resource".into()))
        .emit(Op::Call(0))
        .emit(Op::Call(1))
        .emit(Op::Return);
    assert_eq!(
        vm.run(&program_with_main(explicit_close), 0).unwrap(),
        Value::Nil
    );
    assert_eq!(closed.get(), 1);

    let mut make = Chunk::new("make", 0);
    make.emit(Op::GetGlobal("make_resource".into()))
        .emit(Op::Call(0))
        .emit(Op::Return);
    let resource = vm.run(&program_with_main(make), 0).unwrap();
    drop(vm);
    assert_eq!(closed.get(), 2);
    drop(resource);
    assert_eq!(destroyed.get(), 3);
}

#[test]
fn retries_native_resource_close_after_busy_and_panicking_attempts() {
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed.clone(), destroyed);
    let mut vm = Vm::new();
    native_resource_fixture::install(&mut vm, &module);

    for (factory, closer) in [
        ("make_resource", "busy_then_close"),
        ("make_panicking_resource", "retry_close"),
    ] {
        let mut main = Chunk::new("main", 0);
        main.emit(Op::GetGlobal(closer.into()))
            .emit(Op::GetGlobal(factory.into()))
            .emit(Op::Call(0))
            .emit(Op::Call(1))
            .emit(Op::Return);
        assert_eq!(vm.run(&program_with_main(main), 0).unwrap(), Value::Nil);
    }

    assert_eq!(closed.get(), 2);
}

#[test]
fn closes_a_native_resource_retained_by_structured_error_data() {
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed.clone(), destroyed.clone());
    let mut vm = Vm::new();
    native_resource_fixture::install(&mut vm, &module);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::GetGlobal("fail_with_resource".into()))
        .emit(Op::Call(0))
        .emit(Op::Return);
    let error = vm
        .run(&program_with_main(main), 0)
        .expect_err("native resource error should fail");
    assert!(matches!(
        error
            .native
            .as_deref()
            .and_then(|native| native.data.as_ref()),
        Some(Value::NativeResource(_))
    ));

    drop(vm);
    assert_eq!(closed.get(), 1);
    drop(error);
    assert_eq!(destroyed.get(), 1);
}

#[test]
fn shared_loader_closes_native_resources_only_after_its_last_runtime_owner() {
    let closed = Rc::new(Cell::new(0));
    let destroyed = Rc::new(Cell::new(0));
    let module = native_resource_fixture::module(closed.clone(), destroyed.clone());
    let loader = ModuleLoader::new(".", None);
    let mut first = Vm::with_module_loader(loader.clone());
    native_resource_fixture::install(&mut first, &module);

    let mut make = Chunk::new("make", 0);
    make.emit(Op::GetGlobal("make_resource".into()))
        .emit(Op::Call(0))
        .emit(Op::Return);
    let resource = first.run(&program_with_main(make), 0).unwrap();
    let second = Vm::with_module_loader(loader.clone());

    drop(second);
    drop(first);
    assert_eq!(closed.get(), 0);
    drop(loader);
    assert_eq!(closed.get(), 1);
    drop(resource);
    assert_eq!(destroyed.get(), 1);
}

#[test]
fn native_descriptors_use_module_qualified_signature_identity() {
    fn returns_nil(call: &mut NativeCall<'_>) -> NativeStatus {
        call.return_value(NativeOwnedValue::nil())
    }

    let first = NativeModule::new("test.first", ()).unwrap();
    let function = first
        .function("read", NativeArity::Exact(0), returns_nil)
        .unwrap();
    assert_eq!(function.qualified_name(), "test.first.read");
    assert!(
        first
            .function("read", NativeArity::Exact(0), returns_nil)
            .is_err()
    );
    assert!(
        first
            .function("read", NativeArity::Exact(1), returns_nil)
            .is_ok()
    );

    let second = NativeModule::new("test.second", ()).unwrap();
    assert!(
        second
            .function("read", NativeArity::Exact(0), returns_nil)
            .is_ok()
    );
}

#[test]
fn turns_runtime_faults_into_slug_errors_with_source_locations() {
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let zero = main.constant(Value::Int(0));
    main.emit(Op::Constant(one))
        .emit(Op::Constant(zero))
        .emit_at(Op::Divide, SourceSpan::new("example.slug", 2, 8))
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("division should fail");
    assert_eq!(error.kind, RuntimeErrorKind::DivideByZero);
    assert_eq!(error.span, Some(SourceSpan::new("example.slug", 2, 8)));
    assert!(!error.frames.is_empty());
}

#[test]
fn reports_function_names_and_call_sites_in_frames() {
    let mut inner = Chunk::new("inner", 0);
    let one = inner.constant(Value::Int(1));
    let zero = inner.constant(Value::Int(0));
    inner
        .emit(Op::Constant(one))
        .emit(Op::Constant(zero))
        .emit_at(Op::Divide, SourceSpan::new("inner.slug", 3, 5))
        .emit(Op::Return);

    let mut outer = Chunk::new("outer", 0);
    outer
        .emit(Op::MakeClosure {
            chunk: 0,
            captures: vec![],
        })
        .emit_at(Op::Call(0), SourceSpan::new("outer.slug", 7, 3))
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::MakeClosure {
        chunk: 1,
        captures: vec![],
    })
    .emit_at(Op::Call(0), SourceSpan::new("main.slug", 2, 1))
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(inner);
    program.add_chunk(outer);
    program.add_chunk(main);
    let error = Vm::new()
        .run_named(&program, "main")
        .expect_err("division should fail");
    assert_eq!(error.frames[0].function, "inner");
    assert_eq!(
        error.frames[0].span,
        Some(SourceSpan::new("outer.slug", 7, 3))
    );
    assert_eq!(error.frames[1].function, "outer");
    assert_eq!(
        error.frames[1].span,
        Some(SourceSpan::new("main.slug", 2, 1))
    );
    assert_eq!(error.frames[2].function, "main");
    assert_eq!(error.frames[2].span, None);
}
