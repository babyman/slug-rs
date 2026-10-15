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
