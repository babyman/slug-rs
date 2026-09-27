use std::{env, path::PathBuf, process::ExitCode};

use slug_vm::FixtureRunner;

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let Some(directory) = arguments.next() else {
        eprintln!("Usage: slug-fixtures fixture-directory --slug path/to/slug");
        return ExitCode::from(1);
    };
    let Some(flag) = arguments.next() else {
        eprintln!("Usage: slug-fixtures fixture-directory --slug path/to/slug");
        return ExitCode::from(1);
    };
    let Some(executable) = arguments.next() else {
        eprintln!("Usage: slug-fixtures fixture-directory --slug path/to/slug");
        return ExitCode::from(1);
    };
    if flag != "--slug" || arguments.next().is_some() {
        eprintln!("Usage: slug-fixtures fixture-directory --slug path/to/slug");
        return ExitCode::from(1);
    }
    match FixtureRunner::new(PathBuf::from(executable)).run_directory(&PathBuf::from(directory)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("slug-fixtures: {error}");
            ExitCode::from(1)
        }
    }
}
