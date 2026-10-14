use super::*;

fn source_json(source: &str) -> String {
    let program = compile("json.slug", source).expect("compile JSON source");
    Vm::new()
        .run_named(&program, "main")
        .expect("run JSON source")
        .to_json()
}

#[test]
fn encodes_scalars_and_lists_as_plain_json() {
    assert_eq!(Value::Nil.to_json(), "null");
    assert_eq!(Value::Bool(false).to_json(), "false");
    assert_eq!(Value::Int(-42).to_json(), "-42");
    assert_eq!(Value::string("hi").to_json(), "\"hi\"");
    assert_eq!(
        Value::list(vec![Value::Int(1), Value::string("a"), Value::list(vec![])]).to_json(),
        "[1,\"a\",[]]"
    );
}

#[test]
fn encodes_numbers_in_map_key_canonical_form() {
    for (value, expected) in [
        (1.0, "1"),
        (-0.0, "0"),
        (0.1, "0.1"),
        (-2.5, "-2.5"),
        (1e-6, "0.000001"),
        (1.5e-7, "1.5e-7"),
        (1e20, "100000000000000000000"),
        (1e21, "1e21"),
        (-1e21, "-1e21"),
        (-1.5e-7, "-1.5e-7"),
        (f64::MAX, "1.7976931348623157e308"),
    ] {
        assert_eq!(Value::Float(value).to_json(), expected, "{value:?}");
    }
    assert_eq!(Value::Int(i64::MIN).to_json(), "-9223372036854775808");
}

#[test]
fn escapes_strings_as_json() {
    assert_eq!(
        Value::string("q\"b\\\u{8}\t\n\u{c}\r\u{1}\u{7f}é").to_json(),
        "\"q\\\"b\\\\\\b\\t\\n\\f\\r\\u0001\u{7f}é\""
    );
}

#[test]
fn encodes_maps_with_entries_sorted_by_encoded_key() {
    let first = Value::map(vec![
        (Value::string("b"), Value::Int(2)),
        (Value::Int(1), Value::Bool(true)),
        (Value::string("a"), Value::Nil),
    ]);
    let second = Value::map(vec![
        (Value::string("a"), Value::Nil),
        (Value::Float(1.0), Value::Bool(true)),
        (Value::string("b"), Value::Int(2)),
    ]);
    let expected = "{\"$map\":[[\"a\",null],[\"b\",2],[1,true]]}";
    assert_eq!(first.to_json(), expected);
    assert_eq!(second.to_json(), expected);
}

#[test]
fn encodes_tagged_source_values() {
    assert_eq!(
        source_json(
            "val Point = struct { x, y = 2 }\nenum Color { Red, Green }\n[0x\"00ff\", Point {x: 1}, Color.Green, {a: [1]}]\n"
        ),
        "[{\"$bytes\":\"00ff\"},{\"$struct\":{\"x\":1,\"y\":2}},{\"$enum\":\"Color\",\"case\":\"Green\"},{\"$map\":[[\"a\",[1]]]}]"
    );
}

#[test]
fn encodes_functions_and_schemas_as_opaque_descriptors() {
    assert_eq!(
        source_json("[fn(x) { x }, struct { x }]\n"),
        "[{\"$opaque\":\"fn\",\"display\":\"<fn>\"},{\"$opaque\":\"schema\",\"display\":\"<struct schema>\"}]"
    );
    assert_eq!(
        Value::Float(f64::NAN).to_json(),
        "{\"$opaque\":\"num\",\"display\":\"NaN\"}"
    );
}

#[test]
fn truncates_containers_past_the_depth_limit() {
    let mut value = Value::Int(0);
    for _ in 0..200 {
        value = Value::list(vec![value]);
    }
    let encoded = value.to_json();
    assert!(encoded.starts_with(&"[".repeat(95)));
    assert!(encoded.contains(&format!(
        "{}{{\"$truncated\":\"list\"}}{}",
        "[".repeat(95),
        "]".repeat(95)
    )));
    assert_eq!(encoded.matches('[').count(), 95);

    let mut map = Value::Int(0);
    for _ in 0..40 {
        map = Value::map(vec![(Value::string("k"), map)]);
    }
    let encoded = map.to_json();
    assert_eq!(encoded.matches("$map").count(), 31);
    assert!(encoded.contains("{\"$truncated\":\"map\"}"));
}
