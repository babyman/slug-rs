use std::{
    env,
    io::{self, BufRead, Write},
    process::ExitCode,
};

use serde::Serialize;
use slug_vm::host::build_default_host_vm;
use slug_vm::interactive::Server;

fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let source_root = match env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("slug-server: cannot determine current directory: {error}");
            return ExitCode::from(1);
        }
    };
    let slug_home = env::var_os("SLUG_HOME").map(std::path::PathBuf::from);
    let (vm, _) =
        match build_default_host_vm(&source_root, slug_home.as_deref(), &[], "interactive") {
            Ok(host) => host,
            Err(error) => {
                eprintln!("slug-server: cannot configure default host: {error}");
                return ExitCode::from(1);
            }
        };
    let mut server = Server::new(vm);
    let mut output = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("slug-server: cannot read standard input: {error}");
                return ExitCode::from(1);
            }
        };
        let response = server.handle_line(&line);
        for event in server.take_events() {
            if let Err(error) = write_message(&mut output, &event) {
                eprintln!("slug-server: cannot write standard output: {error}");
                return ExitCode::from(1);
            }
        }
        if let Err(error) = write_message(&mut output, &response) {
            eprintln!("slug-server: cannot write standard output: {error}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

fn write_message(message_output: &mut dyn Write, message: &impl Serialize) -> io::Result<()> {
    let encoded = serde_json::to_string(message).map_err(io::Error::other)?;
    writeln!(message_output, "{encoded}")?;
    message_output.flush()
}
