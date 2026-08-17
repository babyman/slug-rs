use slug_vm::{Capture, Chunk, Op, Program, RuntimeErrorKind, SourceSpan, Value, Vm};

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
