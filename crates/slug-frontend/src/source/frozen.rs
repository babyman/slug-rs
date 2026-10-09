//! Desktop-only lowering for firmware-local frozen program images.
//!
//! This deliberately supports one audited entry shape while the portable
//! `.cslug` contract remains unimplemented. It emits Rust build input, not a
//! bytecode file or a compatibility promise.

use super::{
    SourceError,
    ast::{CallArgument, Expr, ExprKind, Pattern},
    compile,
    lexer::Lexer,
    parser::Parser,
};
use slug_vm::Value;

/// Compiles a minimal UART hello entry into Rust input for the ESP32-S3 build.
///
/// The accepted source is one immutable `main` function with no parameters,
/// whose body is one positional `println("text")` call. Unsupported source is
/// rejected at the source location rather than being partially frozen.
///
/// # Errors
///
/// Returns the ordinary source diagnostic for invalid Slug input, or a checked
/// semantic diagnostic when valid input is outside this initial frozen-entry
/// subset.
pub fn compile_frozen_uart_entry(path: &str, source: &str) -> Result<String, SourceError> {
    compile(path, source)?;
    let expressions = Parser::new(Lexer::new(path, source).tokens()?).parse()?;
    let expression = only_expression(&expressions)?;
    let text = entry_text(expression)?;
    Ok(render_image(text))
}

fn only_expression(expressions: &[Expr]) -> Result<&Expr, SourceError> {
    match expressions {
        [expression] => Ok(expression),
        [expression, ..] => Err(unsupported(
            expression,
            "a frozen ESP32 entry must contain exactly one declaration",
        )),
        [] => Err(SourceError::semantic(
            "a frozen ESP32 entry must declare `main`",
            slug_vm::SourceSpan::new("<frozen entry>", 1, 1),
        )),
    }
}

fn entry_text(expression: &Expr) -> Result<&str, SourceError> {
    let ExprKind::Declare {
        mutable: false,
        pattern: Pattern::Binding(name),
        value,
        ..
    } = &expression.kind
    else {
        return Err(unsupported(
            expression,
            "a frozen ESP32 entry must be `val main = fn() { ... }`",
        ));
    };
    if name != "main" {
        return Err(unsupported(
            expression,
            "the frozen entry declaration must be `main`",
        ));
    }
    let ExprKind::Function {
        type_parameters,
        parameters,
        body,
        ..
    } = &value.kind
    else {
        return Err(unsupported(expression, "`main` must be a function"));
    };
    if !type_parameters.is_empty() || !parameters.is_empty() {
        return Err(unsupported(value, "frozen `main` must not take parameters"));
    }
    let ExprKind::Block(expressions) = &body.kind else {
        return Err(unsupported(body, "frozen `main` must have a block body"));
    };
    let call = only_expression(expressions)?;
    let ExprKind::Call { callee, arguments } = &call.kind else {
        return Err(unsupported(call, "frozen `main` must call `println`"));
    };
    let ExprKind::Name(name) = &callee.kind else {
        return Err(unsupported(callee, "frozen `main` must call `println`"));
    };
    if name != "println" {
        return Err(unsupported(callee, "frozen `main` may only call `println`"));
    }
    let [CallArgument::Positional(argument)] = arguments.as_slice() else {
        return Err(unsupported(
            call,
            "frozen `println` requires one string argument",
        ));
    };
    let ExprKind::Value(Value::Str(text)) = &argument.kind else {
        return Err(unsupported(
            argument,
            "frozen `println` requires a string literal",
        ));
    };
    Ok(text)
}

fn unsupported(expression: &Expr, message: &str) -> SourceError {
    SourceError::semantic(message, expression.span.clone())
}

fn render_image(text: &str) -> String {
    format!(
        "const FROZEN_CONSTANTS: &[FrozenConstant] = &[FrozenConstant::Text({text:?})];\n\
         const FROZEN_INSTRUCTIONS: &[FrozenInstruction] = &[\n\
             FrozenInstruction::GetGlobal(\"println\"),\n\
             FrozenInstruction::Constant(0),\n\
             FrozenInstruction::Call(1),\n\
             FrozenInstruction::Return,\n\
         ];\n\
         const FROZEN_CHUNKS: &[FrozenChunk] = &[FrozenChunk {{\n\
             name: \"main\",\n\
             constants: FROZEN_CONSTANTS,\n\
             instructions: FROZEN_INSTRUCTIONS,\n\
         }}];\n\
         const FROZEN_PROGRAM: FrozenProgram = FrozenProgram {{ chunks: FROZEN_CHUNKS, entry: 0 }};\n"
    )
}

#[cfg(test)]
mod tests {
    use super::compile_frozen_uart_entry;

    #[test]
    fn emits_a_firmware_image_for_the_minimal_uart_entry() {
        let image =
            compile_frozen_uart_entry("hello.slug", "val main = fn() { println(\"hello\") }")
                .expect("minimal entry is supported");
        assert!(image.contains("FrozenConstant::Text(\"hello\")"));
        assert!(image.contains("FrozenInstruction::Call(1)"));
    }

    #[test]
    fn rejects_a_non_string_println_argument() {
        let error = compile_frozen_uart_entry("invalid.slug", "val main = fn() { println(1) }")
            .expect_err("only the initial literal entry shape is supported");
        assert_eq!(error.kind, super::super::SourceErrorKind::Semantic);
        assert!(error.message.contains("string literal"));
    }
}
