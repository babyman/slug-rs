//! Deeply nested source compiles, or fails with `nesting_limit_exceeded`,
//! without overflowing the stack of a 2 MiB thread, the default for a Rust
//! test or spawned thread. Each case runs on such a thread.

use std::{fmt::Write as _, thread};

use slug_frontend::{SourceErrorCode, compile};

/// The bracketed nesting the parser accepts.
const NESTING: usize = 256;
/// The expression depth the parser accepts, reached by chains.
const CHAIN: usize = 4096;

/// A program binding `x` to `expression`, with `f` and `n` in scope.
fn program(expression: &str) -> String {
    format!(
        "val f = fn(x) {{ x }}\nval n = 1\nval x = {expression}\nval main = fn() {{ println(x) }}\n"
    )
}

fn chain(kind: &str, links: usize) -> String {
    program(&match kind {
        "add" => vec!["1"; links].join(" + "),
        "coalesce" => format!("{} ?? 1", vec!["nil"; links].join(" ?? ")),
        "prepend" => format!("{} +: []", vec!["1"; links].join(" +: ")),
        "pipeline" => format!("1{}", " /> f".repeat(links)),
        "field" => format!("{{a: 1}}{}", ".a".repeat(links)),
        "else-if" => {
            let mut text = String::from("if (n == 0) { 0 }");
            for i in 1..links {
                write!(text, " else if (n == {i}) {{ {i} }}").expect("write to a string");
            }
            text + " else { -1 }"
        }
        _ => unreachable!("unknown chain {kind}"),
    })
}

fn nesting(kind: &str, levels: usize) -> String {
    program(&match kind {
        "call" => format!("{}1{}", "f(".repeat(levels), ")".repeat(levels)),
        "list" => format!("{}1{}", "[".repeat(levels), "]".repeat(levels)),
        "function" => format!("{}1{}", "fn() { ".repeat(levels), " }".repeat(levels)),
        "spawn" => format!("{}1", "spawn ".repeat(levels)),
        "spawn-block" => format!("{}1{}", "spawn { ".repeat(levels), " }".repeat(levels)),
        "type" => format!(
            "{{ val t:{}num{} = []\n t }}",
            "list<".repeat(levels),
            ">".repeat(levels)
        ),
        _ => unreachable!("unknown nesting {kind}"),
    })
}

/// The error code compiling `source` on a 2 MiB thread gives, if any.
fn compile_on_small_stack(source: String) -> Option<SourceErrorCode> {
    thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || compile("deep.slug", &source).err().map(|error| error.code))
        .expect("spawn compiling thread")
        .join()
        .expect("compiling thread finishes")
}

fn assert_accepted(case: &str, source: String) {
    let code = compile_on_small_stack(source);
    assert_ne!(
        code,
        Some(SourceErrorCode::NestingLimitExceeded),
        "{case} is within the limits"
    );
}

fn assert_rejected(case: &str, source: String) {
    assert_eq!(
        compile_on_small_stack(source),
        Some(SourceErrorCode::NestingLimitExceeded),
        "{case} exceeds the limits"
    );
}

#[test]
fn chains_compile_up_to_the_expression_depth_limit() {
    for kind in ["add", "coalesce", "prepend", "pipeline", "field", "else-if"] {
        assert_accepted(kind, chain(kind, CHAIN - 16));
        assert_rejected(kind, chain(kind, CHAIN + 16));
    }
}

#[test]
fn bracketed_nesting_compiles_up_to_the_nesting_limit() {
    for kind in ["call", "list", "function", "spawn", "spawn-block"] {
        assert_accepted(kind, nesting(kind, NESTING));
        assert_rejected(kind, nesting(kind, NESTING + 16));
    }
    // The binding's own nesting leaves room for one level less of type.
    assert_accepted("type", nesting("type", NESTING - 2));
    assert_rejected("type", nesting("type", NESTING + 16));
}

#[test]
fn rejects_very_long_chains_before_parsing_them_through() {
    for kind in ["coalesce", "prepend", "else-if"] {
        assert_rejected(kind, chain(kind, 200_000));
    }
    assert_rejected("spawn", nesting("spawn", 200_000));
}
