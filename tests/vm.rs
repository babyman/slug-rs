use std::{cell::Cell, rc::Rc};

use slug_vm::{
    CallArgumentKind, Capture, Chunk, MatchMapKey, MatchRest, NativeArity, NativeCall, NativeError,
    NativeModule, NativeOwnedValue, NativeResourceType, NativeStatus, Op, Program,
    RuntimeErrorKind, SchemaField, SourceSpan, Value, Vm, compile,
};

fn program_with_main(main: Chunk) -> Program {
    let mut program = Program::new();
    program.add_chunk(main);
    program
}

mod native_resource_fixture {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    struct Payload {
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
    }

    struct State {
        first: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        second: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
    }

    pub fn module(closed: Rc<Cell<usize>>, destroyed: Rc<Cell<usize>>) -> NativeModule {
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let module = NativeModule::new(
            "test.shared_resources",
            State {
                first: first.clone(),
                second: second.clone(),
                closed,
                destroyed,
            },
        )
        .unwrap();
        *first.borrow_mut() = Some(module.resource_type("first", close, destroy).unwrap());
        *second.borrow_mut() = Some(module.resource_type("second", close, destroy).unwrap());
        module
    }

    pub fn install(vm: &mut Vm, module: &NativeModule) {
        for (name, arity, callback) in [
            (
                "make_resource",
                NativeArity::Exact(0),
                make_resource as for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
            ),
            ("wrong_resource", NativeArity::Exact(1), wrong_resource),
            ("close_resource", NativeArity::Exact(1), close_resource),
        ] {
            vm.define_native(module.function(name, arity, callback).unwrap())
                .unwrap();
        }
    }

    fn close(payload: &mut Payload) {
        payload.closed.set(payload.closed.get() + 1);
    }

    fn destroy(payload: Payload) {
        let Payload { destroyed, .. } = payload;
        destroyed.set(destroyed.get() + 1);
    }

    fn make_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.return_value(value),
            Err(error) => call.raise(error),
        }
    }

    fn wrong_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .second
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        match call.with_resource(0, &resource_type, |_| ()) {
            Ok(()) => call.return_value(NativeOwnedValue::nil()),
            Err(error) => call.raise(error),
        }
    }

    fn close_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }
}

#[test]
fn retains_top_level_export_names_as_module_metadata() {
    let program = compile(
        "exports.slug",
        "export val answer = 42\nexport var {left, right} = {left: 1, right: 2}\nval hidden = 0\n",
    )
    .expect("compile exported module");
    assert_eq!(program.exports(), ["answer", "left", "right"]);
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
fn repeats_strings_through_private_multiply_bytecode() {
    let mut main = Chunk::new("main", 0);
    let dash = main.constant(Value::string("-"));
    let two = main.constant(Value::Int(2));
    main.emit(Op::Constant(dash))
        .emit(Op::Constant(two))
        .emit(Op::Multiply)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("--")
    );
}

#[test]
fn pipes_values_through_private_call_bytecode() {
    fn add(call: &mut NativeCall<'_>) -> NativeStatus {
        let left = match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
            Ok(value) => value,
            Err(error) => return call.raise(error),
        };
        let right = match call.argument(1).and_then(slug_vm::NativeValueRef::as_i64) {
            Ok(value) => value,
            Err(error) => return call.raise(error),
        };
        call.return_value(NativeOwnedValue::integer(left + right))
    }

    let mut main = Chunk::new("main", 0);
    let two = main.constant(Value::Int(2));
    let three = main.constant(Value::Int(3));
    main.emit(Op::Constant(two))
        .emit(Op::GetGlobal("add".into()))
        .emit(Op::Constant(three))
        .emit(Op::PipelineCall(vec![CallArgumentKind::Positional]))
        .emit(Op::Return);
    let mut vm = Vm::new();
    let module = NativeModule::new("test.math", ()).unwrap();
    vm.define_native(module.function("add", NativeArity::Exact(2), add).unwrap())
        .unwrap();

    assert_eq!(vm.run(&program_with_main(main), 0).unwrap(), Value::Int(5));
}

#[test]
fn matches_structs_through_private_pattern_bytecode() {
    let mut main = Chunk::new("main", 0);
    let name = main.constant(Value::string("Slug"));
    main.emit(Op::StructSchema(vec![SchemaField {
        name: "name".into(),
        has_default: false,
    }]))
    .emit(Op::DefineGlobal("User".into()))
    .emit(Op::GetGlobal("User".into()))
    .emit(Op::Constant(name))
    .emit(Op::Struct(vec!["name".into()]))
    .emit(Op::GetGlobal("User".into()))
    .emit(Op::TryMatch {
        pattern: slug_vm::MatchPattern::Struct {
            schema: 0,
            fields: vec![("name".into(), slug_vm::MatchPattern::Binding)],
        },
        bindings: 1,
        operands: 1,
    })
    .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn executes_checked_bitwise_and_shift_bytecode() {
    let mut main = Chunk::new("main", 0);
    let left = main.constant(Value::Int(6));
    let right = main.constant(Value::Int(3));
    main.emit(Op::Constant(left))
        .emit(Op::Constant(right))
        .emit(Op::BitAnd)
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(2)
    );

    let mut invalid = Chunk::new("invalid", 0);
    let one = invalid.constant(Value::Int(1));
    let count = invalid.constant(Value::Int(64));
    invalid
        .emit(Op::Constant(one))
        .emit(Op::Constant(count))
        .emit(Op::ShiftLeft)
        .emit(Op::Return);
    let error = Vm::new()
        .run(&program_with_main(invalid), 0)
        .expect_err("out-of-range shifts must remain checked");
    assert_eq!(error.kind, RuntimeErrorKind::Type);
    assert_eq!(error.message, "shift count is out of range");
}

#[test]
fn appends_values_through_private_list_bytecode() {
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let two = main.constant(Value::Int(2));
    main.emit(Op::Constant(one))
        .emit(Op::List(1))
        .emit(Op::Constant(two))
        .emit(Op::ListAppend)
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![Value::Int(1), Value::Int(2)]))
    );

    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let two = main.constant(Value::Int(2));
    main.emit(Op::Constant(one))
        .emit(Op::Constant(two))
        .emit(Op::List(1))
        .emit(Op::ListPrepend)
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![Value::Int(1), Value::Int(2)]))
    );
}

#[test]
fn concatenates_lists_through_private_add_bytecode() {
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let two = main.constant(Value::Int(2));
    main.emit(Op::Constant(one))
        .emit(Op::List(1))
        .emit(Op::Constant(two))
        .emit(Op::List(1))
        .emit(Op::Add)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![Value::Int(1), Value::Int(2)]))
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
        error.thrown.as_deref(),
        Some(&Value::List(std::rc::Rc::new(vec![Value::Int(42)])))
    );
    assert_eq!(error.span, Some(SourceSpan::new("throw.slug", 3, 1)));
    assert_eq!(error.message, "uncaught throw: [42]");
}

#[test]
fn deferred_handler_rethrows_replace_the_error_and_preserve_its_cause() {
    let program = compile(
        "defer-rethrow.slug",
        "val fail = fn() {\n\
           defer onerror(err) { throw \"replacement\" }\n\
           throw \"original\"\n\
         }\n\
         fail()\n",
    )
    .expect("compile deferred rethrow source");

    let error = Vm::new()
        .run_named(&program, "main")
        .expect_err("replacement error must remain checked");

    assert_eq!(error.thrown.as_deref(), Some(&Value::string("replacement")));
    let cause = error
        .cause
        .expect("replacement error retains the active error");
    assert_eq!(cause.thrown.as_deref(), Some(&Value::string("original")));
    assert!(error.frames.iter().all(|frame| frame.function != "<fn>"));
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
        .emit(Op::Recur(vec![CallArgumentKind::Positional]));

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
                rest: MatchRest::Binding,
            },
            bindings: 2,
            operands: 0,
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
fn at_patterns_bind_whole_values_before_nested_bindings() {
    let whole = Value::List(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]));
    let mut main = Chunk::new("main", 0);
    let values = main.constant(whole.clone());
    main.emit(Op::Constant(values))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::At(Box::new(slug_vm::MatchPattern::List {
                items: vec![slug_vm::MatchPattern::Binding],
                rest: MatchRest::Binding,
            })),
            bindings: 3,
            operands: 0,
        })
        .emit(Op::JumpIfFalse(14))
        .emit(Op::Pop)
        .emit(Op::DefineGlobal("tail".into()))
        .emit(Op::DefineGlobal("head".into()))
        .emit(Op::DefineGlobal("whole".into()))
        .emit(Op::Pop)
        .emit(Op::GetGlobal("whole".into()))
        .emit(Op::GetGlobal("head".into()))
        .emit(Op::GetGlobal("tail".into()))
        .emit(Op::List(3))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![
            whole,
            Value::Int(1),
            Value::List(std::rc::Rc::new(vec![Value::Int(2), Value::Int(3)])),
        ]))
    );
}

#[test]
fn match_alternatives_rollback_before_retrying() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::List(std::rc::Rc::new(vec![Value::Int(1)])));
    main.emit(Op::Constant(value))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Alternatives(vec![
                slug_vm::MatchPattern::At(Box::new(slug_vm::MatchPattern::List {
                    items: vec![slug_vm::MatchPattern::Literal(Value::Int(2))],
                    rest: MatchRest::None,
                })),
                slug_vm::MatchPattern::List {
                    items: vec![slug_vm::MatchPattern::Binding],
                    rest: MatchRest::None,
                },
            ]),
            bindings: 1,
            operands: 0,
        })
        .emit(Op::JumpIfFalse(10))
        .emit(Op::Pop)
        .emit(Op::DefineGlobal("value".into()))
        .emit(Op::Pop)
        .emit(Op::GetGlobal("value".into()))
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(1)
    );
}

#[test]
fn pinned_patterns_compare_dynamic_operands() {
    let mut main = Chunk::new("main", 0);
    let subject = main.constant(Value::Int(3));
    let expected = main.constant(Value::Int(3));
    main.emit(Op::Constant(subject))
        .emit(Op::Duplicate)
        .emit(Op::Constant(expected))
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Pinned(0),
            bindings: 0,
            operands: 1,
        })
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn computed_map_pattern_keys_use_dynamic_operands() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::Int(7), Value::string("seven")),
        (Value::string("extra"), Value::Bool(true)),
    ])));
    let key = main.constant(Value::Int(7));
    main.emit(Op::Constant(value))
        .emit(Op::Duplicate)
        .emit(Op::Constant(key))
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Map {
                entries: vec![(MatchMapKey::Operand(0), slug_vm::MatchPattern::Binding)],
                rest: MatchRest::None,
                exact: false,
            },
            bindings: 1,
            operands: 1,
        })
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn rejects_unhashable_computed_map_pattern_keys() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Map(std::rc::Rc::new(Vec::new())));
    let key = main.constant(Value::List(std::rc::Rc::new(Vec::new())));
    main.emit(Op::Constant(value))
        .emit(Op::Duplicate)
        .emit(Op::Constant(key))
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Map {
                entries: vec![(MatchMapKey::Operand(0), slug_vm::MatchPattern::Wildcard)],
                rest: MatchRest::None,
                exact: false,
            },
            bindings: 0,
            operands: 1,
        })
        .emit(Op::Return);

    let error = Vm::new().run(&program_with_main(main), 0).unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::Type);
    assert_eq!(error.message, "list cannot be used as a map key");
}

#[test]
fn rejects_missing_dynamic_pattern_operands() {
    let mut main = Chunk::new("main", 0);
    let subject = main.constant(Value::Int(3));
    main.emit(Op::Constant(subject))
        .emit(Op::Duplicate)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Pinned(0),
            bindings: 0,
            operands: 0,
        })
        .emit(Op::Return);

    let error = Vm::new().run(&program_with_main(main), 0).unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(error.message, "match pattern operand 0 does not exist");
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
                entries: vec![(
                    MatchMapKey::String("name".into()),
                    slug_vm::MatchPattern::Binding,
                )],
                rest: MatchRest::None,
                exact: false,
            },
            bindings: 1,
            operands: 0,
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
                entries: vec![(
                    MatchMapKey::String("name".into()),
                    slug_vm::MatchPattern::Binding,
                )],
                rest: MatchRest::Binding,
                exact: false,
            },
            bindings: 2,
            operands: 0,
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
                entries: vec![(
                    MatchMapKey::String("name".into()),
                    slug_vm::MatchPattern::Binding,
                )],
                rest: MatchRest::None,
                exact: true,
            },
            bindings: 1,
            operands: 0,
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
fn rejects_non_list_spread_arguments_in_private_bytecode() {
    let mut main = Chunk::new("main", 0);
    let value = main.constant(Value::Int(1));
    main.emit(Op::Nil)
        .emit(Op::Constant(value))
        .emit(Op::CallSpread(vec![CallArgumentKind::Spread]))
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("non-list call spread must be checked");
    assert_eq!(error.kind, RuntimeErrorKind::Type);
    assert_eq!(error.message, "call spread expects a list");
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
fn slices_lists_with_omitted_bounds_in_private_bytecode() {
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let two = main.constant(Value::Int(2));
    let three = main.constant(Value::Int(3));
    let end = main.constant(Value::Int(2));
    main.emit(Op::Constant(one))
        .emit(Op::Constant(two))
        .emit(Op::Constant(three))
        .emit(Op::List(3))
        .emit(Op::Constant(end))
        .emit(Op::GetSlice {
            has_start: false,
            has_end: true,
            has_step: false,
        })
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::List(std::rc::Rc::new(vec![Value::Int(1), Value::Int(2)]))
    );
}

#[test]
fn constructs_structs_with_stored_defaults_and_field_access() {
    let mut main = Chunk::new("main", 0);
    let name = main.constant(Value::string("Slug"));
    let field = main.constant(Value::string("name"));
    main.emit(Op::True)
        .emit(Op::StructSchema(vec![
            SchemaField {
                name: "name".into(),
                has_default: false,
            },
            SchemaField {
                name: "active".into(),
                has_default: true,
            },
        ]))
        .emit(Op::DefineGlobal("User".into()))
        .emit(Op::GetGlobal("User".into()))
        .emit(Op::Constant(name))
        .emit(Op::Struct(vec!["name".into()]))
        .emit(Op::Constant(field))
        .emit(Op::GetIndex)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("Slug")
    );
}

#[test]
fn rejects_duplicate_fields_in_struct_schema_bytecode() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::StructSchema(vec![
        SchemaField {
            name: "name".into(),
            has_default: false,
        },
        SchemaField {
            name: "name".into(),
            has_default: false,
        },
    ]))
    .emit(Op::Return);

    let error = Vm::new().run(&program_with_main(main), 0).unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(error.message, "duplicate struct schema field 'name'");
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
    assert_eq!(vm.global("answer"), Some(&Value::Int(42)));
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
