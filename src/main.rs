use std::{
    env,
    fmt::Write as FmtWrite,
    fs,
    io::Write as IoWrite,
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::Serialize;
use slug_vm::{
    ClutchRepository, Configuration, ModuleLoader, NativeArity, NativeCall, NativeModule,
    NativeOwnedValue, NativeStatus, RuntimeError, RuntimeErrorKind, SourceError, SourceErrorKind,
    SourceSpan, Vm,
};

fn native_print(call: &mut NativeCall<'_>) -> NativeStatus {
    native_write(call, false)
}

fn native_println(call: &mut NativeCall<'_>) -> NativeStatus {
    native_write(call, true)
}

fn native_write(call: &mut NativeCall<'_>, newline: bool) -> NativeStatus {
    let output = (0..call.argument_count())
        .map(|index| {
            call.argument(index)
                .expect("index comes from the argument count")
                .to_display_string()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let result = if newline {
        writeln!(std::io::stdout().lock(), "{output}")
    } else {
        write!(std::io::stdout().lock(), "{output}")
    };
    if let Err(error) = result {
        return call.raise(slug_vm::NativeError::new(
            "native.io",
            format!("cannot write standard output: {error}"),
        ));
    }
    call.return_value(NativeOwnedValue::nil())
}

fn native_len(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call.argument(0) {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    let length = match value.kind() {
        slug_vm::NativeValueKind::String => match value.as_str() {
            Ok(value) => value.chars().count(),
            Err(error) => return call.raise(error),
        },
        slug_vm::NativeValueKind::Bytes => match value.as_bytes() {
            Ok(value) => value.len(),
            Err(error) => return call.raise(error),
        },
        slug_vm::NativeValueKind::List | slug_vm::NativeValueKind::Map => {
            value.len().expect("collection kind has a length")
        }
        kind => {
            return call.raise(slug_vm::NativeError::new(
                "native.type",
                format!("`len` expects str, bytes, list, or map, got {kind:?}"),
            ));
        }
    };
    let Ok(length) = i64::try_from(length) else {
        return call.raise(slug_vm::NativeError::new(
            "native.range",
            "`len` result exceeds the supported integer range",
        ));
    };
    call.return_value(NativeOwnedValue::integer(length))
}

fn native_channel(call: &mut NativeCall<'_>) -> NativeStatus {
    let capacity = match call.argument_count() {
        0 => 0,
        1 => match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
            Ok(value) => match usize::try_from(value) {
                Ok(value) => value,
                Err(_) => {
                    return call.raise(slug_vm::NativeError::new(
                        "native.type",
                        "channel capacity must not be negative or too large",
                    ));
                }
            },
            Err(error) => {
                return call.raise(error);
            }
        },
        count => {
            return call.raise(slug_vm::NativeError::new(
                "native.arity",
                format!("`slug.channel.chan` expects at most 1 argument, got {count}"),
            ));
        }
    };
    let channel = call.plain_channel(capacity);
    call.return_value(channel)
}

fn native_close(call: &mut NativeCall<'_>) -> NativeStatus {
    if let Err(error) = call.close_channel(0) {
        return call.raise(error);
    }
    call.return_value(NativeOwnedValue::nil())
}

fn register_native_modules(vm: &mut Vm) {
    let builtins = NativeModule::new("slug.builtin", ()).expect("static native module is valid");
    vm.define_builtin(
        builtins
            .function("print", NativeArity::Variadic { minimum: 0 }, native_print)
            .expect("static builtin function is valid"),
    )
    .expect("static builtin binding is unique");
    vm.define_builtin(
        builtins
            .function(
                "println",
                NativeArity::Variadic { minimum: 0 },
                native_println,
            )
            .expect("static builtin function is valid"),
    )
    .expect("static builtin binding is unique");
    vm.define_builtin(
        builtins
            .function("len", NativeArity::Exact(1), native_len)
            .expect("static builtin function is valid"),
    )
    .expect("static builtin binding is unique");

    let channel = NativeModule::new("slug.channel", ()).expect("static native module is valid");
    vm.define_foreign(
        channel
            .function(
                "chan",
                NativeArity::Range {
                    minimum: 0,
                    maximum: 1,
                },
                native_channel,
            )
            .expect("static foreign function is valid"),
    )
    .expect("static foreign binding is unique");
    vm.define_foreign(
        channel
            .function("close", NativeArity::Exact(1), native_close)
            .expect("static foreign function is valid"),
    )
    .expect("static foreign binding is unique");
}

fn main() -> ExitCode {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "slug".into());
    let mut remaining = args.collect::<Vec<_>>().into_iter().peekable();
    let json = remaining
        .peek()
        .is_some_and(|argument| argument == "--diagnostic-format=json");
    if json {
        remaining.next();
    }
    match remaining.next().as_deref() {
        Some("--version" | "-V") => {
            println!("slug-vm {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!(
                "Usage: {executable} [--diagnostic-format=json] program.slug [arguments...]\n\nSupports the Slug core: bindings, functions, blocks, conditionals, match, return, throw, defer, recur, collections, arithmetic and logic, calls, print, println, and len."
            );
            ExitCode::SUCCESS
        }
        Some(path) => {
            let program_arguments = remaining.collect::<Vec<_>>();
            run(path, &program_arguments, json)
        }
    }
}

fn run(path: &str, program_arguments: &[String], json: bool) -> ExitCode {
    let configured_source_root = env::var_os("SLUG_FIXTURE_MODULE_ROOT").map(PathBuf::from);
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let library_root = env::var_os("SLUG_FIXTURE_LIBRARY_ROOT")
        .map(PathBuf::from)
        .or_else(|| slug_home.as_ref().map(|home| home.join("lib")));
    let (resolved_path, source) = match read_entry_source(
        path,
        configured_source_root.as_deref(),
        library_root.as_deref(),
    ) {
        Ok(source) => source,
        Err((path, error)) => {
            eprint_startup_error(
                "io",
                "entry_read",
                format!("cannot read {}: {error}", path.display()),
                json,
            );
            return ExitCode::from(1);
        }
    };
    let source_root = configured_source_root.unwrap_or_else(|| {
        resolved_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .into()
    });
    let entry_module = resolved_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let configuration = Configuration::load(
        &source_root,
        slug_home.as_deref(),
        env::vars(),
        program_arguments,
        entry_module,
    );
    let clutches = match slug_home.as_ref() {
        Some(home) if home.join("clutch/manifest.toml").exists() => {
            match ClutchRepository::from_manifest(home.join("clutch")) {
                Ok(repository) => repository,
                Err(error) => {
                    eprint_startup_error("host", "clutch", error.to_string(), json);
                    return ExitCode::from(1);
                }
            }
        }
        _ => ClutchRepository::default(),
    };
    let loader = ModuleLoader::with_configuration_and_clutch_repository(
        source_root,
        library_root,
        configuration,
        clutches,
    );
    let resolved_path = resolved_path.to_string_lossy();
    let mut program = match loader.compile_source(&resolved_path, &source) {
        Ok(program) => program,
        Err(error) => {
            let category = match error.kind {
                SourceErrorKind::Parse => "parse",
                SourceErrorKind::Semantic => "semantic",
            };
            eprint_source_error(category, &error, &resolved_path, &source, json);
            return ExitCode::from(1);
        }
    };
    program.set_module_name(entry_module);
    let mut vm = Vm::with_module_loader(loader.clone());
    register_native_modules(&mut vm);
    match vm.run_program(&program) {
        Ok(_) => {
            for warning in loader.take_warnings() {
                eprintln!("slug: warning: {warning}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprint_runtime_error(&error, &resolved_path, &source, json);
            ExitCode::from(1)
        }
    }
}

/// Reads an entry program by explicit path, module root, or installed library name.
///
/// The library fallback accepts a bare name such as `hello` and reads
/// `lib/hello.slug`; explicit paths retain their supplied extension.
fn read_entry_source(
    path: &str,
    source_root: Option<&Path>,
    library_root: Option<&Path>,
) -> Result<(PathBuf, String), (PathBuf, std::io::Error)> {
    let requested = Path::new(path);
    let mut candidates = vec![requested.to_path_buf()];
    if let Some(source_root) = source_root {
        let candidate = source_root.join(requested);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    if let Some(library_root) = library_root {
        let library_entry = if requested.extension().is_some() {
            requested.to_path_buf()
        } else {
            requested.with_extension("slug")
        };
        candidates.push(library_root.join(library_entry));
    }

    for candidate in candidates {
        match fs::read_to_string(&candidate) {
            Ok(source) => return Ok((candidate, source)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err((candidate, error)),
        }
    }
    Err((
        requested.to_path_buf(),
        std::io::Error::from(std::io::ErrorKind::NotFound),
    ))
}

#[derive(Serialize)]
struct JsonDiagnostic {
    version: u8,
    category: String,
    kind: Option<String>,
    message: String,
    location: Option<JsonLocation>,
    frames: Vec<JsonFrame>,
    cause: Option<Box<JsonDiagnostic>>,
}

#[derive(Serialize)]
struct JsonLocation {
    path: String,
    line: u32,
    column: u32,
}

#[derive(Serialize)]
struct JsonFrame {
    function: String,
    location: Option<JsonLocation>,
}

fn eprint_startup_error(category: &str, kind: &str, message: String, json: bool) {
    if json {
        eprint_json_diagnostic(&JsonDiagnostic {
            version: 1,
            category: category.into(),
            kind: Some(kind.into()),
            message,
            location: None,
            frames: Vec::new(),
            cause: None,
        });
    } else {
        eprintln!("slug: {message}");
    }
}

fn eprint_source_error(
    category: &str,
    error: &SourceError,
    input_path: &str,
    input: &str,
    json: bool,
) {
    if json {
        eprint_json_diagnostic(&source_diagnostic(category, error));
        return;
    }
    eprintln!(
        "{}",
        render_source_error(category, error, input_path, input)
    );
}

fn render_source_error(
    category: &str,
    error: &SourceError,
    input_path: &str,
    input: &str,
) -> String {
    let headline = format!("slug: {category} error: {}", error.message);
    render_source_context(&headline, error.span.as_ref(), input_path, input)
        .unwrap_or_else(|| format!("slug: {category} error: {error}"))
}

fn eprint_runtime_error(error: &RuntimeError, input_path: &str, input: &str, json: bool) {
    if json {
        eprint_json_diagnostic(&runtime_diagnostic(error));
        return;
    }
    eprintln!("{}", render_runtime_error(error, input_path, input));
}

fn eprint_json_diagnostic(diagnostic: &JsonDiagnostic) {
    println_to_stderr(
        &serde_json::to_string(&diagnostic).expect("diagnostic JSON is serializable"),
    );
}

fn println_to_stderr(text: &str) {
    eprintln!("{text}");
}

fn source_diagnostic(category: &str, error: &SourceError) -> JsonDiagnostic {
    JsonDiagnostic {
        version: 1,
        category: category.into(),
        kind: None,
        message: error.message.clone(),
        location: error.span.as_ref().map(json_location),
        frames: Vec::new(),
        cause: None,
    }
}

fn runtime_diagnostic(error: &RuntimeError) -> JsonDiagnostic {
    let category = if error.kind == RuntimeErrorKind::Module {
        "module"
    } else {
        "runtime"
    };
    JsonDiagnostic {
        version: 1,
        category: category.into(),
        kind: Some(runtime_kind(&error.kind).into()),
        message: error.message.clone(),
        location: error.span.as_ref().map(json_location),
        frames: error
            .frames
            .iter()
            .map(|frame| JsonFrame {
                function: frame.function.clone(),
                location: frame.span.as_ref().map(json_location),
            })
            .collect(),
        cause: error
            .cause
            .as_deref()
            .map(|cause| Box::new(runtime_diagnostic(cause))),
    }
}

fn json_location(span: &SourceSpan) -> JsonLocation {
    JsonLocation {
        path: span.path.to_string(),
        line: span.line,
        column: span.column,
    }
}

fn runtime_kind(kind: &RuntimeErrorKind) -> &'static str {
    match kind {
        RuntimeErrorKind::InvalidBytecode => "invalid_bytecode",
        RuntimeErrorKind::Type => "type",
        RuntimeErrorKind::Name => "name",
        RuntimeErrorKind::Arity => "arity",
        RuntimeErrorKind::DivideByZero => "divide_by_zero",
        RuntimeErrorKind::InvalidCall => "invalid_call",
        RuntimeErrorKind::Native => "native",
        RuntimeErrorKind::NativeContract => "native_contract",
        RuntimeErrorKind::Module => "module",
        RuntimeErrorKind::NotImplemented => "not_implemented",
        RuntimeErrorKind::Match => "match",
        RuntimeErrorKind::Thrown => "thrown",
    }
}

fn render_runtime_error(error: &RuntimeError, input_path: &str, input: &str) -> String {
    let headline = format!("slug: runtime error: {}", error.message);
    let Some(mut rendered) =
        render_source_context(&headline, error.span.as_ref(), input_path, input)
    else {
        return format!("slug: {error}");
    };
    for frame in &error.frames {
        write!(rendered, "\n  in {}", frame.function).expect("writing to a string cannot fail");
        if let Some(span) = &frame.span {
            write!(rendered, " at {}:{}:{}", span.path, span.line, span.column)
                .expect("writing to a string cannot fail");
        }
    }
    rendered
}

fn render_source_context(
    headline: &str,
    span: Option<&SourceSpan>,
    input_path: &str,
    input: &str,
) -> Option<String> {
    let span = span?;
    let source = if span.path.as_ref() == input_path {
        Some(input.to_owned())
    } else {
        fs::read_to_string(span.path.as_ref()).ok()
    };
    let source = source?;
    let line = source.lines().nth(span.line.saturating_sub(1) as usize)?;

    let mut rendered = format!(
        "{headline}\n    --> {}:{}:{}\n",
        span.path, span.line, span.column
    );
    let first_line = span.line.saturating_sub(2).max(1);
    let line_width = span.line.to_string().len();
    for (index, source_line) in source
        .lines()
        .enumerate()
        .skip(first_line as usize - 1)
        .take((span.line - first_line + 1) as usize)
    {
        let number = index + 1;
        let source_line = expand_tabs(source_line);
        if number == span.line as usize {
            writeln!(rendered, "  > {number:>line_width$} | {source_line}")
                .expect("writing to a string cannot fail");
        } else {
            writeln!(rendered, "    {number:>line_width$} | {source_line}")
                .expect("writing to a string cannot fail");
        }
    }
    let caret_offset = display_column(line, span.column);
    write!(
        rendered,
        "    {} | {}^ here",
        " ".repeat(line_width),
        " ".repeat(caret_offset)
    )
    .expect("writing to a string cannot fail");
    Some(rendered)
}

fn expand_tabs(text: &str) -> String {
    let mut expanded = String::new();
    let mut column = 0;
    for character in text.chars() {
        if character == '\t' {
            let spaces = 4 - (column % 4);
            expanded.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            expanded.push(character);
            column += 1;
        }
    }
    expanded
}

fn display_column(text: &str, source_column: u32) -> usize {
    let characters = source_column.saturating_sub(1) as usize;
    let mut column = 0;
    for character in text.chars().take(characters) {
        if character == '\t' {
            column += 4 - (column % 4);
        } else {
            column += 1;
        }
    }
    column
}

#[cfg(test)]
mod tests {
    use slug_vm::{SourceError, SourceErrorKind, SourceSpan};

    use super::render_source_error;

    #[test]
    fn falls_back_when_diagnostic_source_is_unavailable() {
        let error = SourceError {
            kind: SourceErrorKind::Parse,
            message: "expected expression".into(),
            span: Some(SourceSpan::new("missing.slug", 4, 2)),
        };

        assert_eq!(
            render_source_error("parse", &error, "input.slug", "val = 1\n"),
            "slug: parse error: expected expression at missing.slug:4:2"
        );
    }
}
