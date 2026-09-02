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
