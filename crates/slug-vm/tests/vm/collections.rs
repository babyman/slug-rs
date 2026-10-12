use super::*;

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
fn maps_compare_by_key_value_membership_not_entry_order() {
    let first = Value::Map(std::rc::Rc::new(vec![
        (Value::string("name"), Value::string("Slug")),
        (Value::string("version"), Value::Int(1)),
    ]));
    let second = Value::Map(std::rc::Rc::new(vec![
        (Value::string("version"), Value::Int(1)),
        (Value::string("name"), Value::string("Slug")),
    ]));
    assert_eq!(first, second);
}

#[test]
fn numeric_key_equality_is_exact_across_integer_and_float_representations() {
    assert_eq!(Value::Int(1), Value::Float(1.0));
    assert_eq!(Value::Int(0), Value::Float(-0.0));
    assert_ne!(
        Value::Int(9_007_199_254_740_993),
        Value::Float(9_007_199_254_740_992.0)
    );
}

#[test]
fn rejects_non_finite_private_bytecode_constants() {
    let mut main = Chunk::new("main", 0);
    let key = main.constant(Value::Float(f64::NAN));
    let value = main.constant(Value::Int(1));
    main.emit(Op::Constant(key))
        .emit(Op::Constant(value))
        .emit(Op::Map(1))
        .emit(Op::Return);

    let error = Vm::new().run(&program_with_main(main), 0).unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert_eq!(error.message, "bytecode contains a non-finite number");
}

#[test]
#[cfg(feature = "metrics")]
fn records_map_construction_lookup_and_update_costs() {
    let mut main = Chunk::new("main", 0);
    let key = main.constant(Value::string("name"));
    let initial = main.constant(Value::string("Slug"));
    let replacement = main.constant(Value::string("VM"));
    main.emit(Op::Constant(key))
        .emit(Op::Constant(initial))
        .emit(Op::Map(1))
        .emit(Op::Constant(key))
        .emit(Op::Constant(replacement))
        .emit(Op::Map(1))
        .emit(Op::Add)
        .emit(Op::Constant(key))
        .emit(Op::GetIndex)
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run(&program_with_main(main), 0).unwrap(),
        Value::string("VM")
    );
    let metrics = vm.metrics();
    assert_eq!(metrics.collection_constructions, 2);
    assert_eq!(metrics.collection_elements_constructed, 2);
    assert_eq!(metrics.collection_lookups, 1);
    assert_eq!(metrics.map_entries_examined, 1);
    assert_eq!(metrics.collection_updates, 1);
    assert_eq!(metrics.collection_elements_copied, 0);
    assert_eq!(metrics.collection_unique_owner_updates, 1);
    assert_eq!(metrics.collection_shared_owner_updates, 0);
}

#[test]
#[cfg(feature = "metrics")]
fn records_unique_map_copy_update_costs() {
    let mut main = Chunk::new("main", 0);
    let key = main.constant(Value::string("name"));
    let initial = main.constant(Value::string("Slug"));
    let replacement = main.constant(Value::string("VM"));
    main.emit(Op::Constant(key))
        .emit(Op::Constant(initial))
        .emit(Op::Map(1))
        .emit(Op::Constant(replacement))
        .emit(Op::StructCopy(vec!["name".into()]))
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run(&program_with_main(main), 0).unwrap(),
        Value::Map(std::rc::Rc::new(vec![(
            Value::string("name"),
            Value::string("VM"),
        )]))
    );
    let metrics = vm.metrics();
    assert_eq!(metrics.collection_updates, 1);
    assert_eq!(metrics.collection_elements_copied, 0);
    assert_eq!(metrics.collection_unique_owner_updates, 1);
    assert_eq!(metrics.collection_shared_owner_updates, 0);
}

#[test]
fn indexes_and_slices_bytes() {
    let mut main = Chunk::new("main", 0);
    let bytes = main.constant(Value::Bytes(vec![2, 3, 4].into()));
    let index = main.constant(Value::Int(0));
    main.emit(Op::Constant(bytes))
        .emit(Op::Constant(index))
        .emit(Op::GetIndex)
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Int(2)
    );

    let mut main = Chunk::new("main", 0);
    let bytes = main.constant(Value::Bytes(vec![2, 3, 4].into()));
    let start = main.constant(Value::Int(1));
    main.emit(Op::Constant(bytes))
        .emit(Op::Constant(start))
        .emit(Op::GetSlice {
            has_start: true,
            has_end: false,
            has_step: false,
        })
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::Bytes(vec![3, 4].into())
    );
}

#[test]
fn indexes_and_slices_strings_by_unicode_scalar_value() {
    let mut main = Chunk::new("main", 0);
    let string = main.constant(Value::string("aébc"));
    let index = main.constant(Value::Int(1));
    main.emit(Op::Constant(string))
        .emit(Op::Constant(index))
        .emit(Op::GetIndex)
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("é")
    );

    let mut main = Chunk::new("main", 0);
    let string = main.constant(Value::string("aébc"));
    let start = main.constant(Value::Int(1));
    let end = main.constant(Value::Int(-1));
    main.emit(Op::Constant(string))
        .emit(Op::Constant(start))
        .emit(Op::Constant(end))
        .emit(Op::GetSlice {
            has_start: true,
            has_end: true,
            has_step: false,
        })
        .emit(Op::Return);
    assert_eq!(
        Vm::new().run(&program_with_main(main), 0).unwrap(),
        Value::string("éb")
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
        Value::list(vec![Value::Int(1), Value::Int(2)])
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
fn constant_key_indexing_matches_stack_key_indexing() {
    let map = Value::Map(std::rc::Rc::new(vec![
        (Value::string("name"), Value::string("Slug")),
        (Value::string("name"), Value::string("shadowing")),
    ]));
    let list = Value::list(vec![Value::Int(10), Value::Int(20)]);
    let cases = [
        (map.clone(), Value::string("name")),
        (map, Value::string("missing")),
        (list.clone(), Value::Int(-1)),
        (list, Value::Int(5)),
        (Value::Int(1), Value::string("name")),
    ];
    for (collection, key) in cases {
        let run = |fused: bool| {
            let mut main = Chunk::new("main", 0);
            let collection = main.constant(collection.clone());
            let key = main.constant(key.clone());
            main.emit(Op::Constant(collection));
            if fused {
                main.emit(Op::GetIndexConstant(key));
            } else {
                main.emit(Op::Constant(key)).emit(Op::GetIndex);
            }
            main.emit(Op::Return);
            Vm::new()
                .run(&program_with_main(main), 0)
                .map_err(|error| (error.kind, error.message))
        };
        assert_eq!(run(true), run(false), "indexing {collection} with {key}");
    }
}

#[test]
fn rejects_constant_key_indexing_without_a_value_constant() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil)
        .emit(Op::GetIndexConstant(0))
        .emit(Op::Return);

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("a missing index constant must be rejected before execution");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert!(
        error
            .message
            .contains("references missing index constant 0"),
        "{}",
        error.message
    );
}

#[test]
#[cfg(feature = "metrics")]
fn last_local_read_moves_a_list_for_in_place_append() {
    let mut main = Chunk::new("main", 0);
    main.locals = 1;
    let one = main.constant(Value::Int(1));
    main.emit(Op::List(0))
        .emit(Op::SetLocal(0))
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(one))
        .emit(Op::ListAppend)
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run(&program_with_main(main), 0).unwrap(),
        Value::list(vec![Value::Int(1)])
    );
    let metrics = vm.metrics();
    assert_eq!(metrics.collection_unique_owner_updates, 1);
    assert_eq!(metrics.collection_shared_owner_updates, 0);
}

#[test]
#[cfg(feature = "metrics")]
fn local_read_before_a_later_read_still_clones() {
    let mut main = Chunk::new("main", 0);
    main.locals = 1;
    let one = main.constant(Value::Int(1));
    main.emit(Op::List(0))
        .emit(Op::SetLocal(0))
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(one))
        .emit(Op::ListAppend)
        .emit(Op::Pop)
        .emit(Op::GetLocal(0))
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run(&program_with_main(main), 0).unwrap(),
        Value::list(Vec::new())
    );
    assert_eq!(vm.metrics().collection_shared_owner_updates, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn local_read_on_one_branch_keeps_the_slot_live() {
    let mut main = Chunk::new("main", 0);
    main.locals = 1;
    let one = main.constant(Value::Int(1));
    main.emit(Op::List(0))
        .emit(Op::SetLocal(0))
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(one))
        .emit(Op::ListAppend)
        .emit(Op::False)
        .emit(Op::JumpIfFalse(9))
        .emit(Op::Pop)
        .emit(Op::Return)
        .emit(Op::Pop)
        .emit(Op::Pop)
        .emit(Op::GetLocal(0))
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run(&program_with_main(main), 0).unwrap(),
        Value::list(Vec::new())
    );
    assert_eq!(vm.metrics().collection_shared_owner_updates, 1);
}

#[test]
fn last_read_of_a_captured_local_preserves_the_closure_binding() {
    let mut main = Chunk::new("main", 0);
    main.locals = 2;
    let one = main.constant(Value::Int(1));
    main.emit(Op::List(0))
        .emit(Op::SetLocal(0))
        .emit(Op::MakeClosure {
            chunk: 1,
            captures: vec![Capture::Local(0)],
        })
        .emit(Op::SetLocal(1))
        .emit(Op::GetLocal(0))
        .emit(Op::Constant(one))
        .emit(Op::ListAppend)
        .emit(Op::Pop)
        .emit(Op::GetLocal(1))
        .emit(Op::Call(0))
        .emit(Op::Return);
    let mut captured = Chunk::new("captured", 0);
    captured.emit(Op::GetCapture(0)).emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(main);
    program.add_chunk(captured);
    assert_eq!(Vm::new().run(&program, 0).unwrap(), Value::list(Vec::new()));
}
