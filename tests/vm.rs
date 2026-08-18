use slug_vm::{Capture, Chunk, Op, Program, RuntimeErrorKind, SourceSpan, Value, Vm, compile};

fn program_with_main(main: Chunk) -> Program {
    let mut program = Program::new();
    program.add_chunk(main);
    program
}

#[test]
fn executes_integer_arithmetic() {
    let mut main = Chunk::new("main", 0);
    let seven = main.constant(Value::Int(7));
    let six = main.constant(Value::Int(6));
    main.emit(Op::Constant(seven))
        .emit(Op::Constant(six))
        .emit(Op::Multiply)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(42)
    );
}

#[test]
fn turns_destructuring_match_failure_into_a_source_located_runtime_error() {
    let mut main = Chunk::new("main", 0);
    main.emit_at(Op::MatchFailure, SourceSpan::new("destructure.slug", 4, 7));
    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("match failure must be checked");

    assert_eq!(error.kind, RuntimeErrorKind::Match);
    assert_eq!(error.message, "destructuring pattern did not match");
    assert_eq!(error.span, Some(SourceSpan::new("destructure.slug", 4, 7)));
}

#[test]
fn preserves_the_value_and_location_of_a_thrown_error() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::List(std::rc::Rc::new(vec![Value::Int(42)])));
    main.emit_at(Op::Constant(value), SourceSpan::new("throw.slug", 3, 7))
        .emit_at(Op::Throw, SourceSpan::new("throw.slug", 3, 1));

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("throw must be a checked runtime error");

    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(
        error.thrown,
        Some(Value::List(std::rc::Rc::new(vec![Value::Int(42)])))
    );
    assert_eq!(error.span, Some(SourceSpan::new("throw.slug", 3, 1)));
    assert_eq!(error.message, "uncaught throw: [42]");
}

#[test]
fn reuses_a_frame_for_tail_recursion() {
    let mut countdown = Chunk::new("countdown", 1);
    let zero = countdown.constant(Value::Int(0));
    let one = countdown.constant(Value::Int(1));
    countdown
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(zero))
        .emit(Op::Equal)
        .emit(Op::JumpIfFalse(7))
        .emit(Op::Pop)
        .emit(Op::GetLocal(0))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(one))
        .emit(Op::Subtract)
        .emit(Op::Recur(1));

    let mut main = Chunk::new("main", 0);
    let iterations = main.constant(Value::Int(100_000));
    main.emit(Op::MakeClosure {
        chunk: 0,
        captures: vec![],
    })
    .emit(Op::Constant(iterations))
    .emit(Op::Call(1))
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(countdown);
    program.add_chunk(main);
    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(0)
    );
}

#[test]
fn recur_preserves_cells_captured_by_an_earlier_iteration() {
    let program = compile(
        "recur-capture.slug",
        "val retain = fn(n, saved) {\n\
           val current = n\n\
           if (n == 0) { saved() } else { recur(n - 1, fn() { current }) }\n\
         }\n\
         retain(1, fn() { nil })\n",
    )
    .expect("compile recur capture source");

    assert_eq!(
        Vm::new().run_named(&program, "main").unwrap(),
        Value::Int(1)
    );
}

#[test]
fn matches_list_patterns_and_exposes_bindings() {
    let mut main = Chunk::new("main", 0);
    let values = main.constant(Value::List(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ])));
    main.emit(Op::Constant(values))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::List {
                items: vec![slug_vm::MatchPattern::Binding],
                rest: true,
            },
            bindings: 2,
        })
        .emit(Op::JumpIfFalse(12))
        .emit(Op::Pop)
        .emit(Op::DefineGlobal("tail".into()))
        .emit(Op::DefineGlobal("head".into()))
        .emit(Op::Pop)
        .emit(Op::GetGlobal("head".into()))
        .emit(Op::GetGlobal("tail".into()))
        .emit(Op::List(2))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![
            Value::Int(1),
            Value::List(std::rc::Rc::new(vec![Value::Int(2), Value::Int(3)])),
        ]))
    );
}

#[test]
fn matches_map_patterns_and_exposes_string_key_bindings() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("name"), Value::string("Slug")),
        (Value::string("extra"), Value::Bool(true)),
    ])));
    main.emit(Op::Constant(value))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Map {
                entries: vec![("name".into(), slug_vm::MatchPattern::Binding)],
                rest: false,
                exact: false,
            },
            bindings: 1,
        })
        .emit(Op::JumpIfFalse(9))
        .emit(Op::Pop)
        .emit(Op::DefineGlobal("name".into()))
        .emit(Op::Pop)
        .emit(Op::GetGlobal("name".into()))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("Slug")
    );
}

#[test]
fn captures_unmatched_map_entries_in_a_rest_binding() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("name"), Value::string("Slug")),
        (Value::string("active"), Value::Bool(true)),
    ])));
    main.emit(Op::Constant(value))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Map {
                entries: vec![("name".into(), slug_vm::MatchPattern::Binding)],
                rest: true,
                exact: false,
            },
            bindings: 2,
        })
        .emit(Op::JumpIfFalse(10))
        .emit(Op::Pop)
        .emit(Op::DefineGlobal("rest".into()))
        .emit(Op::DefineGlobal("name".into()))
        .emit(Op::Pop)
        .emit(Op::GetGlobal("rest".into()))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Map(std::rc::Rc::new(vec![(
            Value::string("active"),
            Value::Bool(true),
        )]))
    );
}

#[test]
fn exact_map_patterns_reject_extra_entries() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("name"), Value::string("Slug")),
        (Value::string("active"), Value::Bool(true)),
    ])));
    main.emit(Op::Constant(value))
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Map {
                entries: vec![("name".into(), slug_vm::MatchPattern::Binding)],
                rest: false,
                exact: true,
            },
            bindings: 1,
        })
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn preserves_integer_precision_and_rejects_oversized_calls() {
    let mut main = Chunk::new("main", 0);
    let lower = main.constant(Value::Int(9_007_199_254_740_992));
    let higher = main.constant(Value::Int(9_007_199_254_740_993));
    main.emit(Op::Constant(lower))
        .emit(Op::Constant(higher))
        .emit(Op::Less)
        .emit(Op::Constant(higher))
        .emit(Op::Constant(lower))
        .emit(Op::Subtract)
        .emit(Op::List(2))
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![Value::Bool(true), Value::Int(1)]))
    );

    let mut invalid = Chunk::new("main", 0);
    invalid.emit(Op::Call(usize::MAX)).emit(Op::Return);
    let error = Vm::new()
        .run(&program_with_main(invalid), 0)
        .expect_err("oversized call must be rejected");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(error.message, "call argument count is too large");
}

#[test]
fn constructs_and_indexes_collections() {
    let mut main = Chunk::new("main", 0);
    let ten = main.constant(Value::Int(10));
    let twenty = main.constant(Value::Int(20));
    let minus_one = main.constant(Value::Int(-1));
    main.emit(Op::Constant(ten))
        .emit(Op::Constant(twenty))
        .emit(Op::List(2))
        .emit(Op::Constant(minus_one))
        .emit(Op::GetIndex)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(20)
    );
}

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
    fn double(args: &[Value]) -> Result<Value, String> {
        match args {
            [Value::Int(value)] => Ok(Value::Int(value * 2)),
            _ => Err("expected one integer".into()),
        }
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
    vm.define_native("double", double);

    assert_eq!(vm.run(&program, 0).unwrap(), Value::Int(42));
    assert_eq!(vm.global("answer"), Some(&Value::Int(42)));
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
