//! Audited source shape for the private firmware GPIO proof.

use super::{
    SourceError,
    ast::{CallArgument, Expr, ExprKind, Pattern},
    compile,
    lexer::Lexer,
    parser::Parser,
};
use slug_vm::Value;

/// Capabilities selected by one firmware build.
pub struct FrozenGpioCapabilities<'a> {
    pub modules: &'a [&'a str],
    pub output_aliases: &'a [&'a str],
    pub maximum_sleep_ms: u32,
}

/// Compiles the initial tail-recursive GPIO blink shape to private Rust build input.
///
/// The compiler accepts imports bound to `gpio` and `time`, one zero-argument
/// `blink` function with alternating GPIO writes and sleeps followed by
/// `recur()`, and a zero-argument `main` that calls `blink()`. Because `main`
/// does nothing else, the frozen image inlines `blink` into its entry chunk.
///
/// # Errors
///
/// Returns checked source diagnostics for invalid source, unsupported frozen
/// operations, missing modules, unknown output aliases, or invalid delays.
pub fn compile_frozen_gpio_entry(
    path: &str,
    source: &str,
    capabilities: &FrozenGpioCapabilities<'_>,
) -> Result<String, SourceError> {
    compile(path, source)?;
    let expressions = Parser::new(Lexer::new(path, source).tokens()?).parse()?;
    let [gpio_import, time_import, blink, main] = expressions.as_slice() else {
        return Err(shape_error(
            expressions.first(),
            path,
            "frozen GPIO entry requires gpio/time imports, `blink`, and `main`",
        ));
    };
    check_import(gpio_import, "gpio", "slug.gpio", capabilities)?;
    check_import(time_import, "time", "slug.time", capabilities)?;
    let body = function_body(blink, "blink")?;
    let [on, on_wait, off, off_wait, recur] = body else {
        return Err(unsupported(
            blink,
            "frozen `blink` requires on, sleep, off, sleep, and recur calls",
        ));
    };
    let on_alias = gpio_call(on, true, capabilities)?;
    let on_delay = sleep_call(on_wait, capabilities)?;
    let off_alias = gpio_call(off, false, capabilities)?;
    let off_delay = sleep_call(off_wait, capabilities)?;
    if !matches!(&recur.kind, ExprKind::Recur(arguments) if arguments.is_empty()) {
        return Err(unsupported(recur, "frozen `blink` must end with `recur()`"));
    }
    let [main_call] = function_body(main, "main")? else {
        return Err(unsupported(main, "frozen `main` must only call `blink()`"));
    };
    let ExprKind::Call { callee, arguments } = &main_call.kind else {
        return Err(unsupported(main_call, "frozen `main` must call `blink()`"));
    };
    if !arguments.is_empty() || !matches!(&callee.kind, ExprKind::Name(name) if name == "blink") {
        return Err(unsupported(main_call, "frozen `main` must call `blink()`"));
    }
    Ok(render_blink(on_alias, on_delay, off_alias, off_delay))
}

fn check_import(
    expression: &Expr,
    binding: &str,
    module: &str,
    capabilities: &FrozenGpioCapabilities<'_>,
) -> Result<(), SourceError> {
    let ExprKind::Declare {
        mutable: false,
        exported: false,
        pattern: Pattern::Binding(name),
        tags,
        annotation: None,
        value,
        ..
    } = &expression.kind
    else {
        return Err(unsupported(
            expression,
            "frozen imports must be immutable bindings",
        ));
    };
    if name != binding || !tags.is_empty() {
        return Err(unsupported(expression, "unexpected frozen import binding"));
    }
    let ExprKind::Call { callee, arguments } = &value.kind else {
        return Err(unsupported(value, "frozen import must call `import`"));
    };
    let [CallArgument::Positional(argument)] = arguments.as_slice() else {
        return Err(unsupported(value, "frozen import requires one module name"));
    };
    if !matches!(&callee.kind, ExprKind::Name(name) if name == "import")
        || !matches!(&argument.kind, ExprKind::Value(Value::Str(name)) if name.as_ref() == module)
    {
        return Err(unsupported(value, "unexpected frozen module import"));
    }
    if !capabilities.modules.contains(&module) {
        return Err(unsupported(
            argument,
            "module is not in the firmware capability manifest",
        ));
    }
    Ok(())
}

fn function_body<'a>(expression: &'a Expr, name: &str) -> Result<&'a [Expr], SourceError> {
    let ExprKind::Declare {
        mutable: false,
        exported: false,
        pattern: Pattern::Binding(binding),
        tags,
        annotation: None,
        value,
        ..
    } = &expression.kind
    else {
        return Err(unsupported(
            expression,
            "frozen function must be an immutable binding",
        ));
    };
    if binding != name || !tags.is_empty() {
        return Err(unsupported(expression, "unexpected frozen function name"));
    }
    let ExprKind::Function {
        type_parameters,
        parameters,
        return_annotation: None,
        body,
        ..
    } = &value.kind
    else {
        return Err(unsupported(value, "frozen binding must contain a function"));
    };
    if !type_parameters.is_empty() || !parameters.is_empty() {
        return Err(unsupported(
            value,
            "frozen function must take no parameters",
        ));
    }
    let ExprKind::Block(expressions) = &body.kind else {
        return Err(unsupported(body, "frozen function requires a block body"));
    };
    Ok(expressions)
}

fn module_call<'a>(
    expression: &'a Expr,
    binding: &str,
    method: &str,
) -> Result<&'a [CallArgument], SourceError> {
    let ExprKind::Call { callee, arguments } = &expression.kind else {
        return Err(unsupported(
            expression,
            "frozen statement must call a module function",
        ));
    };
    let ExprKind::Index { collection, index } = &callee.kind else {
        return Err(unsupported(callee, "frozen call must use a module member"));
    };
    if !matches!(&collection.kind, ExprKind::Name(name) if name == binding)
        || !matches!(&index.kind, ExprKind::Value(Value::Str(name)) if name.as_ref() == method)
    {
        return Err(unsupported(callee, "unexpected frozen module function"));
    }
    Ok(arguments)
}

fn gpio_call<'a>(
    expression: &'a Expr,
    expected_level: bool,
    capabilities: &FrozenGpioCapabilities<'_>,
) -> Result<&'a str, SourceError> {
    let arguments = module_call(expression, "gpio", "write")?;
    let [
        CallArgument::Positional(alias),
        CallArgument::Positional(level),
    ] = arguments
    else {
        return Err(unsupported(
            expression,
            "GPIO write requires an alias and boolean level",
        ));
    };
    let ExprKind::Value(Value::Str(name)) = &alias.kind else {
        return Err(unsupported(
            alias,
            "GPIO output alias must be a string literal",
        ));
    };
    if !capabilities.output_aliases.contains(&name.as_ref()) {
        return Err(unsupported(
            alias,
            "GPIO alias is not in the firmware capability manifest",
        ));
    }
    if !matches!(&level.kind, ExprKind::Value(Value::Bool(value)) if *value == expected_level) {
        return Err(unsupported(
            level,
            "frozen blink requires alternating boolean levels",
        ));
    }
    Ok(name)
}

fn sleep_call(
    expression: &Expr,
    capabilities: &FrozenGpioCapabilities<'_>,
) -> Result<u32, SourceError> {
    let arguments = module_call(expression, "time", "sleepMs")?;
    let [CallArgument::Positional(milliseconds)] = arguments else {
        return Err(unsupported(
            expression,
            "sleepMs requires one millisecond value",
        ));
    };
    let ExprKind::Value(Value::Int(value)) = &milliseconds.kind else {
        return Err(unsupported(
            milliseconds,
            "sleepMs requires a whole millisecond literal",
        ));
    };
    let delay = u32::try_from(*value).map_err(|_| {
        unsupported(
            milliseconds,
            "sleepMs requires a non-negative bounded delay",
        )
    })?;
    if delay > capabilities.maximum_sleep_ms {
        return Err(unsupported(
            milliseconds,
            "sleepMs exceeds the firmware capability manifest limit",
        ));
    }
    Ok(delay)
}

fn render_blink(on_alias: &str, on_delay: u32, off_alias: &str, off_delay: u32) -> String {
    format!(
        "const FROZEN_CONSTANTS: &[FlashConstant] = &[\n\
         FlashConstant::Text({on_alias:?}), FlashConstant::Number({on_delay}),\n\
         FlashConstant::Text({off_alias:?}), FlashConstant::Number({off_delay}),\n\
         ];\n\
         const FROZEN_INSTRUCTIONS: &[FlashInstruction] = &[\n\
         FlashInstruction::GetGlobal(\"gpio.write\"), FlashInstruction::Constant(0),\n\
         FlashInstruction::True, FlashInstruction::Call(2), FlashInstruction::Pop,\n\
         FlashInstruction::GetGlobal(\"time.sleepMs\"), FlashInstruction::Constant(1),\n\
         FlashInstruction::Call(1), FlashInstruction::Pop,\n\
         FlashInstruction::GetGlobal(\"gpio.write\"), FlashInstruction::Constant(2),\n\
         FlashInstruction::False, FlashInstruction::Call(2), FlashInstruction::Pop,\n\
         FlashInstruction::GetGlobal(\"time.sleepMs\"), FlashInstruction::Constant(3),\n\
         FlashInstruction::Call(1), FlashInstruction::Pop,\n\
         FlashInstruction::Recur, FlashInstruction::Nil, FlashInstruction::Return,\n\
         ];\n\
         const FROZEN_PROGRAM: FlashProgram = FlashProgram {{\n\
         name: \"main\", constants: FROZEN_CONSTANTS, instructions: FROZEN_INSTRUCTIONS,\n\
         }};\n"
    )
}

fn unsupported(expression: &Expr, message: &str) -> SourceError {
    SourceError::semantic(message, expression.span.clone())
}

fn shape_error(expression: Option<&Expr>, path: &str, message: &str) -> SourceError {
    expression.map_or_else(
        || SourceError::semantic(message, slug_vm::SourceSpan::new(path, 1, 1)),
        |expression| unsupported(expression, message),
    )
}

#[cfg(test)]
mod tests {
    use super::{FrozenGpioCapabilities, compile_frozen_gpio_entry};

    const CAPABILITIES: FrozenGpioCapabilities<'_> = FrozenGpioCapabilities {
        modules: &["slug.gpio", "slug.time"],
        output_aliases: &["led"],
        maximum_sleep_ms: 1_000,
    };
    const BLINK: &str = "val gpio = import(\"slug.gpio\")\n\
        val time = import(\"slug.time\")\n\
        val blink = fn() {\n\
            gpio.write(\"led\", true)\n\
            time.sleepMs(250)\n\
            gpio.write(\"led\", false)\n\
            time.sleepMs(250)\n\
            recur()\n\
        }\n\
        val main = fn() { blink() }\n";

    #[test]
    fn freezes_the_selected_blink_with_vm_tail_recursion() {
        let image = compile_frozen_gpio_entry("blink.slug", BLINK, &CAPABILITIES)
            .expect("blink shape is supported");
        assert!(image.contains("FlashInstruction::Recur"));
        assert!(image.contains("FlashInstruction::GetGlobal(\"gpio.write\")"));
        assert!(image.contains("FlashConstant::Number(250)"));
    }

    #[test]
    fn rejects_unselected_module_and_unknown_output_alias() {
        let missing_time = FrozenGpioCapabilities {
            modules: &["slug.gpio"],
            ..CAPABILITIES
        };
        let error = compile_frozen_gpio_entry("blink.slug", BLINK, &missing_time)
            .expect_err("time module is not selected");
        assert!(error.message.contains("module is not in"));

        let source = BLINK.replace("\"led\"", "\"other\"");
        let error = compile_frozen_gpio_entry("blink.slug", &source, &CAPABILITIES)
            .expect_err("alias is not selected");
        assert!(error.message.contains("GPIO alias is not in"));
    }

    #[test]
    fn rejects_unsupported_operation_and_delay() {
        let source = BLINK.replace("gpio.write(\"led\", true)", "println(\"hi\")");
        let error = compile_frozen_gpio_entry("blink.slug", &source, &CAPABILITIES)
            .expect_err("unsupported body operation");
        assert!(error.message.contains("module member"));

        let source = BLINK.replace("sleepMs(250)", "sleepMs(5000)");
        let error = compile_frozen_gpio_entry("blink.slug", &source, &CAPABILITIES)
            .expect_err("out of range delay");
        assert!(error.message.contains("manifest limit"));
    }
}
