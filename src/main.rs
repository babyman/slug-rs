use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use slug_vm::{
    Configuration, ModuleLoader, NativeArity, NativeCall, NativeModule, NativeOwnedValue,
    NativeStatus, SourceErrorKind, Vm, compile, compile_type_checked,
};

fn native_println(call: &mut NativeCall<'_>) -> NativeStatus {
    println!(
        "{}",
        (0..call.argument_count())
            .map(|index| {
                call.argument(index)
                    .expect("index comes from the argument count")
                    .to_display_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
    );
    call.return_value(NativeOwnedValue::nil())
}

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
                "Usage: {executable} program.slug\n\nSupports the Slug core: bindings, functions, blocks, conditionals, match, return, throw, defer, recur, collections, arithmetic and logic, calls, and println."
            );
            ExitCode::SUCCESS
        }
        Some("-type-check") => {
            if let Some(path) = args.next() {
                let program_arguments = args.collect::<Vec<_>>();
                run(&path, true, &program_arguments)
            } else {
                eprintln!("Usage: {executable} -type-check program.slug");
                ExitCode::from(1)
            }
        }
        Some(path) => {
            let program_arguments = args.collect::<Vec<_>>();
            run(path, false, &program_arguments)
        }
    }
}

fn run(path: &str, type_check: bool, program_arguments: &[String]) -> ExitCode {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("slug: cannot read {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let mut program = match if type_check {
        compile_type_checked(path, &source)
    } else {
        compile(path, &source)
    } {
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
    let source_root = env::var_os("SLUG_FIXTURE_MODULE_ROOT").map_or_else(
        || {
            Path::new(path)
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .into()
        },
        PathBuf::from,
    );
    let entry_module = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    program.set_module_name(entry_module);
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let configuration = Configuration::load(
        &source_root,
        slug_home.as_deref(),
        env::vars(),
        program_arguments,
        entry_module,
    );
    let library_root = env::var_os("SLUG_FIXTURE_LIBRARY_ROOT").map(PathBuf::from);
    let loader = ModuleLoader::with_configuration(source_root, library_root, configuration);
    let mut vm = Vm::with_module_loader(loader.clone());
    let host = NativeModule::new("slug.host", ()).expect("static native module is valid");
    let println = host
        .function(
            "println",
            NativeArity::Variadic { minimum: 0 },
            native_println,
        )
        .expect("static native function is valid");
    vm.define_native(println)
        .expect("static native binding is unique");
    match vm.run_program(&program) {
        Ok(_) => {
            for warning in loader.take_warnings() {
                eprintln!("slug: warning: {warning}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("slug: {error}");
            ExitCode::from(1)
        }
    }
}
