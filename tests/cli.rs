use std::{fs, process::Command};

fn slug() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slug"));
    command.env("SLUG_HOME", env!("CARGO_MANIFEST_DIR"));
    command
}

fn channel_source(source: &str) -> String {
    format!(
        "val {{ await, chan, close, recv, send }} = import(\"slug.channel\")\n{}",
        source.replace("channel(", "chan(")
    )
}

fn fixture_path(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("slug-cli-{kind}-{}.slug", std::process::id()))
}

// Feature-oriented CLI test modules. Keep shared process and fixture helpers
// here so each module can focus on one observable source-language boundary.
#[path = "cli/basics.rs"]
mod basics;
#[path = "cli/concurrency.rs"]
mod concurrency;
#[path = "cli/diagnostics.rs"]
mod diagnostics;
#[path = "cli/language_core.rs"]
mod language_core;
#[path = "cli/modules.rs"]
mod modules;
#[path = "cli/patterns_and_cleanup.rs"]
mod patterns_and_cleanup;
#[path = "cli/types_and_metadata.rs"]
mod types_and_metadata;
#[path = "cli/types_and_values.rs"]
mod types_and_values;
