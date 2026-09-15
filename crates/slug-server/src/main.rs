use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::Serialize;
use slug_server::interactive::Server;
use slug_vm::host::build_default_host_vm;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments.next().unwrap_or_else(|| "slug-server".into());
    let application = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        eprintln!("Usage: {} [app.slug]", executable.to_string_lossy());
        return ExitCode::from(1);
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let source_root = match application.as_deref().and_then(Path::parent) {
        Some(parent) => parent.to_path_buf(),
        None => match env::current_dir() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("slug-server: cannot determine current directory: {error}");
                return ExitCode::from(1);
            }
        },
    };
    let slug_home = env::var_os("SLUG_HOME").map(PathBuf::from);
    let (vm, loader) =
        match build_default_host_vm(&source_root, slug_home.as_deref(), &[], "interactive") {
            Ok(host) => host,
            Err(error) => {
                eprintln!("slug-server: cannot configure default host: {error}");
                return ExitCode::from(1);
            }
        };
    let mut server = Server::new(vm);
    let mut output = stdout.lock();

    if let Some(path) = application {
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("slug-server: cannot read {}: {error}", path.display());
                return ExitCode::from(1);
            }
        };
        let path_text = path.to_string_lossy();
        let mut program = match loader.compile_source(&path_text, &source) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("slug-server: cannot compile {}: {error}", path.display());
                return ExitCode::from(1);
            }
        };
        program.set_module_name(
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
        );
        if let Err(error) = server.run_root_program(&program) {
            eprintln!("slug-server: root program failed: {error}");
            return ExitCode::from(1);
        }
        if let Err(error) = write_events(&mut output, &mut server) {
            eprintln!("slug-server: cannot write standard output: {error}");
            return ExitCode::from(1);
        }
    }

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("slug-server: cannot read standard input: {error}");
                return ExitCode::from(1);
            }
        };
        let response = server.handle_line(&line);
        if let Err(error) = write_events(&mut output, &mut server) {
            eprintln!("slug-server: cannot write standard output: {error}");
            return ExitCode::from(1);
        }
        if let Err(error) = write_message(&mut output, &response) {
            eprintln!("slug-server: cannot write standard output: {error}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

fn write_events(output: &mut dyn Write, server: &mut Server) -> io::Result<()> {
    for event in server.take_events() {
        write_message(output, &event)?;
    }
    Ok(())
}

fn write_message(message_output: &mut dyn Write, message: &impl Serialize) -> io::Result<()> {
    let encoded = serde_json::to_string(message).map_err(io::Error::other)?;
    writeln!(message_output, "{encoded}")?;
    message_output.flush()
}
