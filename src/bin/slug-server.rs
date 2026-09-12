use std::{
    io::{self, BufRead, Write},
    process::ExitCode,
};

use slug_vm::interactive::Server;

fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = Server::default();
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
        let encoded = match serde_json::to_string(&response) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("slug-server: cannot encode protocol response: {error}");
                return ExitCode::from(1);
            }
        };
        if let Err(error) = writeln!(output, "{encoded}").and_then(|()| output.flush()) {
            eprintln!("slug-server: cannot write standard output: {error}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}
