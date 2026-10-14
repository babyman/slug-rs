use std::{
    env,
    fmt::Write as FmtWrite,
    fs,
    io::Write as IoWrite,
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::Serialize;
use slug::{DesktopLoader, build_default_host_vm, default_library_root, interactive::value_json};
use slug_frontend::{ArtifactHost, SourceError, SourceErrorKind};
use slug_vm::{
    NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus, RuntimeError,
    RuntimeErrorKind, SourceSpan, Vm,
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
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let result = if newline {
        writeln!(stdout, "{output}")
    } else {
        write!(stdout, "{output}")
    }
    .and_then(|()| stdout.flush());
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
                format!("`chan` expects at most 1 argument, got {count}"),
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

/// The `keys` builtin: a map's keys, in its enumeration order.
fn native_keys(call: &mut NativeCall<'_>) -> NativeStatus {
    let result = (|| {
        let map = call.argument(0)?;
        if map.kind() != slug_vm::NativeValueKind::Map {
            return Err(slug_vm::NativeError::new(
                "native.type",
                format!("`keys` expects map, got {:?}", map.kind()),
            ));
        }
        let mut keys = Vec::new();
        let mut index = 0;
        while let Some((key, _)) = map.map_get(index)? {
            keys.push(key.to_owned());
            index += 1;
        }
        Ok(NativeOwnedValue::list(keys))
    })();
    match result {
        Ok(keys) => call.return_value(keys),
        Err(error) => call.raise(error),
    }
}

fn builtin_functions() -> Vec<slug_vm::NativeFunction> {
    let builtins = NativeModule::new("slug.builtin", ()).expect("static native module is valid");
    vec![
        builtins
            .function("print", NativeArity::Variadic { minimum: 0 }, native_print)
            .expect("static builtin function is valid"),
        builtins
            .function(
                "println",
                NativeArity::Variadic { minimum: 0 },
                native_println,
            )
            .expect("static builtin function is valid"),
        builtins
            .function("len", NativeArity::Exact(1), native_len)
            .expect("static builtin function is valid"),
        builtins
            .function("keys", NativeArity::Exact(1), native_keys)
            .expect("static builtin function is valid"),
        builtins
            .function(
                "chan",
                NativeArity::Range {
                    minimum: 0,
                    maximum: 1,
                },
                native_channel,
            )
            .expect("static builtin function is valid"),
        builtins
            .function("close", NativeArity::Exact(1), native_close)
            .expect("static builtin function is valid"),
    ]
}

fn register_native_modules(vm: &mut Vm) {
    for function in builtin_functions() {
        vm.define_builtin(function)
            .expect("static builtin binding is unique");
    }
}

fn main() -> ExitCode {
    if env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--server")
    {
        return slug::run_server();
    }
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
            let build = if cfg!(feature = "concurrency") {
                "full build"
            } else {
                "slim build"
            };
            println!(
                "slug-vm {} ({build})\n\nUsage:\n  {executable} [--diagnostic-format=json] program.slug [arguments...]\n  {executable} [--diagnostic-format=json] compile program.slug output.cslug\n  {executable} [--diagnostic-format=json] run program.cslug [arguments...]\n\nSupports the Slug core: bindings, functions, blocks, conditionals, match, return, throw, defer, recur, collections, arithmetic and logic, calls, print, println, and len. Compiled .cslug artifacts are closed programs and run without source resolution.",
                env!("CARGO_PKG_VERSION"),
            );
            ExitCode::SUCCESS
        }
        Some("compile") => {
            let Some(source_path) = remaining.next() else {
                eprint_startup_error(
                    "usage",
                    "compile",
                    "compile requires a source path and output path".into(),
                    json,
                );
                return ExitCode::from(1);
            };
            let Some(output_path) = remaining.next() else {
                eprint_startup_error(
                    "usage",
                    "compile",
                    "compile requires a source path and output path".into(),
                    json,
                );
                return ExitCode::from(1);
            };
            if remaining.next().is_some() {
                eprint_startup_error(
                    "usage",
                    "compile",
                    "compile accepts only a source path and output path".into(),
                    json,
                );
                return ExitCode::from(1);
            }
            compile(&source_path, &output_path, json)
        }
        Some("run") => {
            let Some(artifact_path) = remaining.next() else {
                eprint_startup_error("usage", "run", "run requires a .cslug path".into(), json);
                return ExitCode::from(1);
            };
            run_artifact(&artifact_path, &remaining.collect::<Vec<_>>(), json)
        }
        Some(path) => {
            let program_arguments = remaining.collect::<Vec<_>>();
            run(path, &program_arguments, json)
        }
    }
}

fn compile(path: &str, output_path: &str, json: bool) -> ExitCode {
    let configured_source_root = env::var_os("SLUG_FIXTURE_MODULE_ROOT").map(PathBuf::from);
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let library_root = default_library_root(slug_home.as_deref());
    let (resolved_path, source) = match DesktopLoader::load_entry(
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
    let (_, loader) =
        match build_default_host_vm(&source_root, slug_home.as_deref(), &[], entry_module) {
            Ok(host) => host,
            Err(error) => {
                eprint_startup_error("host", "clutch", error.to_string(), json);
                return ExitCode::from(1);
            }
        };
    let resolved_path = resolved_path.to_string_lossy();
    let artifact = match loader.compile_artifact(&resolved_path, &source) {
        Ok(artifact) => artifact,
        Err(error) => {
            eprint_startup_error("artifact", "compile", error.to_string(), json);
            return ExitCode::from(1);
        }
    };
    let bytes = match slug_vm::cslug::encode(&artifact) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprint_startup_error("artifact", "encode", error.to_string(), json);
            return ExitCode::from(1);
        }
    };
    if let Err(error) = fs::write(output_path, bytes) {
        eprint_startup_error(
            "io",
            "artifact_write",
            format!("cannot write {output_path}: {error}"),
            json,
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run_artifact(path: &str, program_arguments: &[String], json: bool) -> ExitCode {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprint_startup_error(
                "io",
                "artifact_read",
                format!("cannot read {path}: {error}"),
                json,
            );
            return ExitCode::from(1);
        }
    };
    let artifact = match slug_vm::cslug::decode(&bytes, slug_vm::cslug::Limits::default()) {
        Ok(artifact) => artifact,
        Err(error) => {
            eprint_startup_error("artifact", "decode", error.to_string(), json);
            return ExitCode::from(1);
        }
    };
    let artifact_path = Path::new(path);
    let source_root = artifact_path.parent().unwrap_or_else(|| Path::new("."));
    let entry_module = artifact_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let configuration = slug::Configuration::load(
        source_root,
        slug_home.as_deref(),
        env::vars(),
        program_arguments,
        entry_module,
    );
    let host = match ArtifactHost::new(artifact, std::rc::Rc::new(configuration)) {
        Ok(host) => host,
        Err(error) => {
            eprint_startup_error("artifact", "load", error.to_string(), json);
            return ExitCode::from(1);
        }
    };
    for function in builtin_functions() {
        if let Err(error) = host.register_builtin(function) {
            eprint_startup_error("host", "builtin", error.to_string(), json);
            return ExitCode::from(1);
        }
    }
    match host.run() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprint_runtime_error(&error, path, "", json);
            ExitCode::from(1)
        }
    }
}

fn run(path: &str, program_arguments: &[String], json: bool) -> ExitCode {
    let configured_source_root = env::var_os("SLUG_FIXTURE_MODULE_ROOT").map(PathBuf::from);
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let library_root = default_library_root(slug_home.as_deref());
    let (resolved_path, source) = match DesktopLoader::load_entry(
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
    let (mut vm, loader) = match build_default_host_vm(
        &source_root,
        slug_home.as_deref(),
        program_arguments,
        entry_module,
    ) {
        Ok(host) => host,
        Err(error) => {
            eprint_startup_error("host", "clutch", error.to_string(), json);
            return ExitCode::from(1);
        }
    };
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

#[derive(Serialize)]
struct JsonDiagnostic {
    version: u8,
    category: String,
    kind: Option<String>,
    message: String,
    location: Option<JsonLocation>,
    frames: Vec<JsonFrame>,
    /// Present only for `thrown` runtime errors, so `throw nil` stays visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    thrown: Option<serde_json::Value>,
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
            thrown: None,
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
        thrown: None,
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
        thrown: error.thrown.as_deref().map(value_json),
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
    use slug_frontend::{SourceError, SourceErrorKind};
    use slug_vm::SourceSpan;

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
