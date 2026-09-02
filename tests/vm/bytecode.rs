use super::*;

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
fn concatenates_strings_with_values_through_private_add_bytecode() {
    let mut main = Chunk::new("main", 0);
    let prefix = main.constant(Value::string("list of two + "));
    let data = main.constant(Value::Map(std::rc::Rc::new(vec![(
        Value::string("k"),
        Value::List(std::rc::Rc::new(vec![Value::Int(1)])),
    )])));
    main.emit(Op::Constant(prefix))
        .emit(Op::Constant(data))
        .emit(Op::Add)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("list of two + {\"k\": [1]}")
    );
}

#[test]
fn persistently_merges_and_removes_maps_through_private_bytecode() {
    let mut main = Chunk::new("main", 0);
    let left = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("first"), Value::Int(1)),
        (Value::string("second"), Value::Int(2)),
    ])));
    let right = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("second"), Value::Int(20)),
        (Value::string("third"), Value::Int(3)),
    ])));
    let removed = main.constant(Value::string("second"));
    main.emit(Op::Constant(left))
        .emit(Op::Constant(right))
        .emit(Op::Add)
        .emit(Op::Constant(removed))
        .emit(Op::Subtract)
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Map(std::rc::Rc::new(vec![
            (Value::string("first"), Value::Int(1)),
            (Value::string("third"), Value::Int(3)),
        ]))
    );
}

#[test]
fn persistently_copies_maps_through_private_bytecode() {
    let mut main = Chunk::new("main", 0);
    let original = main.constant(Value::Map(std::rc::Rc::new(vec![
        (Value::string("timeout"), Value::Int(1_000)),
        (Value::string("retries"), Value::Int(2)),
    ])));
    let timeout = main.constant(Value::Int(5_000));
    let mode = main.constant(Value::string("fast"));
    main.emit(Op::Constant(original))
        .emit(Op::Constant(timeout))
        .emit(Op::Constant(mode))
        .emit(Op::StructCopy(vec!["timeout".into(), "mode".into()]))
        .emit(Op::Return);

    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Map(std::rc::Rc::new(vec![
            (Value::string("timeout"), Value::Int(5_000)),
            (Value::string("retries"), Value::Int(2)),
            (Value::string("mode"), Value::string("fast")),
        ]))
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
        pattern: slug_vm::MatchPattern::Constrained {
            pattern: Box::new(slug_vm::MatchPattern::Map {
                entries: vec![(
                    (slug_vm::MatchMapKey::String("name".into())),
                    slug_vm::MatchPattern::Binding,
                )],
                rest: slug_vm::MatchRest::None,
                exact: false,
            }),
            constraint: slug_vm::MatchType::Struct(Some(0)),
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
fn matches_schema_values_through_private_pattern_bytecode() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::StructSchema(vec![]))
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Constrained {
                pattern: Box::new(slug_vm::MatchPattern::Wildcard),
                constraint: slug_vm::MatchType::Schema,
            },
            bindings: 0,
            operands: 0,
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
fn rejects_missing_selected_callable_identity_in_private_bytecode() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil)
        .emit(Op::CallSelected {
            kinds: Vec::new(),
            identity: 0,
        })
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("missing selected identity must be rejected");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(error.message, "selected callable identity does not exist");
}

#[test]
fn rejects_structurally_invalid_private_bytecode_before_execution() {
    let cases = [
        (Op::Constant(0), "references missing constant 0"),
        (Op::GetLocal(0), "references missing local 0"),
        (Op::Add, "requires 2 stack values, has 0"),
        (Op::Jump(2), "jumps to missing instruction 2"),
        (
            Op::MakeClosure {
                chunk: 1,
                captures: Vec::new(),
            },
            "references missing function chunk 1",
        ),
        (Op::Select(Vec::new()), "has no select cases"),
        (
            Op::TryMatch {
                pattern: slug_vm::MatchPattern::Wildcard,
                bindings: 1,
                operands: 0,
            },
            "match pattern binding count is invalid",
        ),
        (
            Op::TryMatch {
                pattern: slug_vm::MatchPattern::Wildcard,
                bindings: 0,
                operands: usize::MAX,
            },
            "match stack count is too large",
        ),
    ];
    for (op, expected) in cases {
        let mut main = Chunk::new("main", 0);
        main.emit(op).emit(Op::Return);
        let error = Vm::new()
            .run(&program_with_main(main), 0)
            .expect_err("structural defect must be rejected before execution");
        assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

#[test]
fn rejects_stack_underflow_after_try_match_before_execution() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil)
        .emit(Op::TryMatch {
            pattern: slug_vm::MatchPattern::Wildcard,
            bindings: 0,
            operands: 0,
        })
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("match stack underflow must be rejected before execution");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert!(
        error
            .message
            .contains("instruction 3 requires 1 stack values, has 0"),
        "{}",
        error.message
    );
}

#[test]
fn rejects_reachable_fallthrough_and_unmatched_scopes_before_execution() {
    let cases = [
        (Chunk::new("main", 0), "falls through without Return"),
        (
            {
                let mut chunk = Chunk::new("main", 0);
                chunk.emit(Op::Nil);
                chunk
            },
            "instruction 0 falls through without Return",
        ),
        (
            {
                let mut chunk = Chunk::new("main", 0);
                chunk.emit(Op::LeaveScope).emit(Op::Nil).emit(Op::Return);
                chunk
            },
            "leaves no active scope",
        ),
    ];
    for (main, expected) in cases {
        let error = Vm::new()
            .run(&program_with_main(main), 0)
            .expect_err("structural defect must be rejected before execution");
        assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

#[test]
fn rejects_scope_depth_mismatches_at_control_flow_joins() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::EnterScope)
        .emit(Op::Nil)
        .emit(Op::JumpIfFalse(5))
        .emit(Op::LeaveScope)
        .emit(Op::Jump(5))
        .emit(Op::Nil)
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("scope-depth mismatch must be rejected before execution");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert!(error.message.contains("inconsistent scope depth"));
}

#[test]
fn malformed_bytecode_does_not_mutate_globals_before_rejection() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil).emit(Op::DefineGlobal("changed".into()));

    let mut vm = Vm::new();
    let error = vm
        .run(&program_with_main(main), 0)
        .expect_err("fallthrough must be rejected before dispatch");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(vm.global("changed"), None);
    assert!(vm.global("cfg").is_some());
}

#[test]
fn generated_malformed_private_bytecode_returns_checked_errors() {
    let templates = [
        Op::Pop,
        Op::Add,
        Op::GetLocal(0),
        Op::LeaveScope,
        Op::Call(usize::MAX),
        Op::Map(usize::MAX),
        Op::Jump(usize::MAX),
        Op::MakeClosure {
            chunk: usize::MAX,
            captures: Vec::new(),
        },
        Op::Select(Vec::new()),
        Op::MakeClosurePooled {
            chunk: 0,
            captures: CaptureListId::new(u32::MAX),
        },
        Op::TryMatch {
            pattern: slug_vm::MatchPattern::Wildcard,
            bindings: 0,
            operands: 0,
        },
        Op::TryMatch {
            pattern: slug_vm::MatchPattern::Wildcard,
            bindings: 0,
            operands: usize::MAX,
        },
    ];
    let mut state = 0x5EED_u64;
    for _ in 0..64 {
        let mut main = Chunk::new("main", 0);
        for _ in 0..4 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let template_count = u64::try_from(templates.len()).expect("template count fits u64");
            let index = usize::try_from(state % template_count).expect("index fits usize");
            main.emit(templates[index].clone());
        }
        main.emit(Op::Return);

        let result = catch_unwind(AssertUnwindSafe(|| {
            Vm::new().run(&program_with_main(main), 0)
        }));
        let result = result.expect("malformed private bytecode must not panic");
        assert!(result.is_err());
    }
}
