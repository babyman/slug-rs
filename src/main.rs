use std::{env, fs, process::ExitCode};

use slug_vm::{SourceErrorKind, Value, Vm, compile};

fn main() -> ExitCode {
    let mut args = env::args();
    let executable = args.next().unwrap_or_else(|| "slug".into());
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("slug-vm {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!(
                "Usage: {executable} program.slug\n\nSupports the Slug core: bindings, functions, blocks, conditionals, collections, arithmetic, calls, and println."
            );
            ExitCode::SUCCESS
        }
        Some(path) => run(path),
    }
}

fn run(path: &str) -> ExitCode {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("slug: cannot read {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let program = match compile(path, &source) {
        Ok(program) => program,
        Err(error) => {
            let category = match error.kind {
                SourceErrorKind::Parse => "parse",
                SourceErrorKind::Semantic => "semantic",
            };
            eprintln!("slug: {category} error: {error}");
            return ExitCode::from(1);
        }
    };
    let mut vm = Vm::new();
    vm.define_native("println", |values| {
        println!(
            "{}",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        );
        Ok(Value::Nil)
    });
    match vm.run_named(&program, "main") {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("slug: {error}");
            ExitCode::from(1)
        }
    }
}
