use std::{env, path::PathBuf, process::ExitCode};

use slug::FixtureRunner;

const USAGE: &str =
    "Usage: slug-fixtures fixture-directory --slug path/to/slug [--timeout-scale N]";

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let (directory, executable, scale) = match arguments.as_slice() {
        [directory, flag, executable] if flag == "--slug" => (directory, executable, 1),
        [directory, flag, executable, scale_flag, scale]
            if flag == "--slug" && scale_flag == "--timeout-scale" =>
        {
            let Ok(scale) = scale.parse::<u32>() else {
                eprintln!("{USAGE}");
                return ExitCode::from(1);
            };
            (directory, executable, scale)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(1);
        }
    };
    match FixtureRunner::new(PathBuf::from(executable))
        .with_timeout_scale(scale)
        .run_directory(&PathBuf::from(directory))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("slug-fixtures: {error}");
            ExitCode::from(1)
        }
    }
}
