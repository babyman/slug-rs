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
