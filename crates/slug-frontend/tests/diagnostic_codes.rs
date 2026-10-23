use std::collections::HashSet;

use slug_frontend::{SourceErrorCode, compile};

#[test]
fn every_source_code_is_unique_and_registered() {
    let registry = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reference/diagnostic-codes.md"
    ))
    .expect("read diagnostic code registry");
    let mut seen = HashSet::new();
    for code in SourceErrorCode::ALL {
        let name = code.as_str();
        assert!(seen.insert(name), "duplicate code `{name}`");
        assert!(
            registry.contains(&format!("| `{name}` |")),
            "`{name}` is missing from docs/reference/diagnostic-codes.md"
        );
    }
}

#[test]
fn source_errors_carry_their_failure_class() {
    for (source, code) in [
        ("val =", SourceErrorCode::ExpectedSyntax),
        ("val s = \"open", SourceErrorCode::UnterminatedLiteral),
        ("val x = 1\nx = 2", SourceErrorCode::ImmutableAssignment),
        ("val x:Missing = 1", SourceErrorCode::UnknownType),
        ("enum E {}", SourceErrorCode::EmptyEnum),
    ] {
        let error = compile("codes.slug", source).expect_err(source);
        assert_eq!(error.code, code, "{source}: {}", error.message);
    }
}

#[test]
fn source_errors_cover_the_offending_source() {
    for (source, (start, end)) in [
        ("val x = 1 + \"a\"", ((1, 9), (1, 16))),
        ("val x = 1\nx = 2", ((2, 1), (2, 6))),
        ("val = 1", ((1, 5), (1, 6))),
        ("val n = 1__0", ((1, 9), (1, 13))),
        ("val p = struct { x }\np.y", ((2, 1), (2, 4))),
    ] {
        let error = compile("ranges.slug", source).expect_err(source);
        let range = error.range.unwrap_or_else(|| panic!("{source}: no range"));
        assert_eq!(
            (
                (range.start.line, range.start.column),
                (range.end.line, range.end.column)
            ),
            (start, end),
            "{source}: {}",
            error.message
        );
        let span = error.span.expect("span");
        assert!(
            (range.start.line, range.start.column) <= (span.line, span.column),
            "{source}: range starts after its span"
        );
    }
}

#[test]
fn source_errors_report_what_was_expected_and_found() {
    for (source, expected, actual) in [
        ("val x:num = \"a\"", Some("num"), Some("str")),
        ("val x = [1", Some("`]`"), Some("end of input")),
        ("match 1 { 1 2 }", Some("`=>`"), Some("number")),
        ("val = 1", None, Some("`=`")),
    ] {
        let error = compile("expected.slug", source).expect_err(source);
        assert_eq!(
            (error.expected.as_deref(), error.actual.as_deref()),
            (expected, actual),
            "{source}: {}",
            error.message
        );
    }
}

fn only_fix(source: &str) -> Option<String> {
    let error = compile("fixes.slug", source).expect_err(source);
    assert!(error.fixes().len() <= 1, "{source}: {:?}", error.fixes());
    error
        .fixes()
        .first()
        .map(|fix| fix.apply(source).expect("fix applies to its source"))
}

fn run(source: &str) -> slug_vm::Value {
    let program = compile("fixed.slug", source).unwrap_or_else(|error| panic!("{source}: {error}"));
    slug_vm::Vm::new()
        .run_named(&program, "main")
        .unwrap_or_else(|error| panic!("{source}: {error:?}"))
}

#[test]
fn closing_delimiters_missing_at_end_of_input_are_inserted() {
    for (source, fixed) in [
        ("val x = [1", "val x = [1]"),
        ("val f = fn() {\n  [1, (2", "val f = fn() {\n  [1, (2)]}"),
        ("val m = {a: 1", "val m = {a: 1}"),
    ] {
        let mut current = source.to_string();
        for _ in 0..4 {
            match only_fix(&current) {
                Some(next) => current = next,
                None => break,
            }
            if compile("fixes.slug", &current).is_ok() {
                break;
            }
        }
        assert_eq!(current, fixed, "{source}");
        compile("fixes.slug", &current).unwrap_or_else(|error| panic!("{current}: {error}"));
    }
}

#[test]
fn deleting_an_unreachable_case_preserves_the_result() {
    for source in [
        "match 1 {\n  _ => 1\n  2 => 2\n}",
        "enum Color { Red, Green }\nmatch Color.Red {\n  Color.Red => 1\n  Color.Green => 2\n  Color.Red => 3\n}",
    ] {
        let fixed = only_fix(source).unwrap_or_else(|| panic!("{source}: no fix"));
        assert!(
            !fixed.contains("=> 3") && !fixed.contains("2 => 2"),
            "{fixed}"
        );
        assert_eq!(run(&fixed), slug_vm::Value::Int(1), "{fixed}");
    }
}

#[test]
fn uncertain_repairs_offer_no_fix() {
    for source in [
        "val x = [1 2]",
        "val s = \"open",
        "val x = 1\nx = 2",
        "val pin = 2\nmatch 1 {\n  _ => 1\n  ^pin => 2\n}",
        "match 1 { 1 2 }",
    ] {
        assert_eq!(only_fix(source), None, "{source}");
    }
}

#[test]
fn a_nil_result_ending_in_an_assignment_is_fixed_with_nil() {
    for source in [
        "var v = 0\nval f = fn():nil { v = 7 }\nf()\nv",
        "var v = 0\nval f = fn(c:bool):nil {\n  if (c) { v = 7 }\n}\nf(true)\nv",
        "var v = 0\nval f = fn(n:num):nil {\n  match n {\n    1 => { v = 7 }\n    _ => nil\n  }\n}\nf(1)\nv",
    ] {
        let fixed = only_fix(source).unwrap_or_else(|| panic!("{source}: no fix"));
        assert!(fixed.contains("; nil"), "{fixed}");
        assert_eq!(run(&fixed), slug_vm::Value::Int(7), "{fixed}");
    }
}

#[test]
fn a_nil_result_ending_in_another_value_only_suggests_nil() {
    let error = compile("fixes.slug", "val f = fn():nil { 5 }").expect_err("a num result");
    assert_eq!(error.fixes().len(), 0);
    assert_eq!(error.suggestions().len(), 1);
    assert_eq!(
        error.suggestions()[0]
            .apply("val f = fn():nil { 5 }")
            .expect("applies"),
        "val f = fn():nil { 5; nil }"
    );
}

#[test]
fn a_prefix_copy_is_fixed_by_moving_copy_after_the_value() {
    for (source, fixed) in [
        (
            "val P = struct { x:num, y:num }\nval p = P { x: 1, y: 2 }\nval q = copy p { y: 5 }\nq.x + q.y",
            "val P = struct { x:num, y:num }\nval p = P { x: 1, y: 2 }\nval q = p copy { y: 5 }\nq.x + q.y",
        ),
        (
            "val P = struct { x:num, y:num }\nval W = struct { p:struct<P> }\nval m = W { p: P { x: 1, y: 2 } }\nval q = copy m.p {\n  y: 5\n}\nq.x + q.y",
            "val P = struct { x:num, y:num }\nval W = struct { p:struct<P> }\nval m = W { p: P { x: 1, y: 2 } }\nval q = m.p copy {\n  y: 5\n}\nq.x + q.y",
        ),
    ] {
        let error = compile("fixes.slug", source).expect_err(source);
        assert!(
            error.message.contains("`copy` follows"),
            "{}",
            error.message
        );
        assert_eq!(only_fix(source).as_deref(), Some(fixed), "{source}");
        assert_eq!(run(fixed), slug_vm::Value::Int(6), "{fixed}");
    }
    assert_eq!(only_fix("val q = copy p"), None);
}
