use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{FixtureMetadata, FixtureOutcome};

/// Runs portable Slug conformance fixtures through a Slug executable.
pub struct FixtureRunner {
    executable: PathBuf,
    default_timeout: Duration,
}

impl FixtureRunner {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            default_timeout: Duration::from_secs(1),
        }
    }

    /// Runs every fixture below `directory`.
    ///
    /// # Errors
    ///
    /// Returns a checked report when a fixture is missing metadata, exceeds its
    /// timeout, crashes the host, or disagrees with its declared expectation.
    pub fn run_directory(&self, directory: &Path) -> Result<(), String> {
        let fixtures = fixture_sources(directory)?;
        let mut failures = Vec::new();
        for source in fixtures {
            if let Err(error) = self.run_fixture(&source) {
                failures.push(format!("{}: {error}", source.display()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }

    fn run_fixture(&self, source: &Path) -> Result<(), String> {
        let sidecar = source.with_extension("fixture.toml");
        let metadata = FixtureMetadata::load(&sidecar).map_err(|error| error.to_string())?;
        let root = sidecar
            .parent()
            .ok_or_else(|| "fixture sidecar has no parent directory".to_owned())?;
        let module_root = metadata
            .module_root
            .as_ref()
            .map_or_else(|| root.to_path_buf(), |path| root.join(path));
        let library_root = metadata.library_root.as_ref().map(|path| root.join(path));
        let mut command = Command::new(&self.executable);
        command
            .arg(source)
            .env_clear()
            .env("SLUG_FIXTURE_MODULE_ROOT", module_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(library_root) = library_root {
            command.env("SLUG_FIXTURE_LIBRARY_ROOT", library_root);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start fixture: {error}"))?;
        let timeout = metadata
            .timeout_ms
            .map_or(self.default_timeout, Duration::from_millis);
        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|error| format!("cannot wait for fixture: {error}"))?
                .is_some()
            {
                break;
            }
            if started.elapsed() >= timeout {
                child
                    .kill()
                    .map_err(|error| format!("cannot stop timed-out fixture: {error}"))?;
                let _ = child.wait();
                return Err(format!("timed out after {} ms", timeout.as_millis()));
            }
            thread::sleep(Duration::from_millis(5));
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("cannot collect fixture output: {error}"))?;
        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| "fixture stdout is not UTF-8".to_owned())?;
        let stderr = String::from_utf8(output.stderr)
            .map_err(|_| "fixture stderr is not UTF-8".to_owned())?;
        let outcome = observed_outcome(output.status.success(), &stderr)?;
        if outcome != metadata.outcome {
            return Err(format!(
                "expected {:?}, observed {:?}",
                metadata.outcome, outcome
            ));
        }
        if let Some(expected) = metadata.stdout.as_deref()
            && stdout != expected
        {
            return Err(format!(
                "stdout mismatch: expected {expected:?}, observed {stdout:?}"
            ));
        }
        if let Some(expected) = metadata.stderr.as_deref()
            && stderr != expected
        {
            return Err(format!(
                "stderr mismatch: expected {expected:?}, observed {stderr:?}"
            ));
        }
        if let Some(expected) = metadata.diagnostic.as_deref()
            && stderr != expected
        {
            return Err(format!(
                "diagnostic mismatch: expected {expected:?}, observed {stderr:?}"
            ));
        }
        Ok(())
    }
}

fn fixture_sources(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut sources = Vec::new();
    visit(directory, &mut sources)?;
    sources.sort();
    if sources.is_empty() {
        return Err("fixture directory contains no .slug sources".into());
    }
    Ok(sources)
}

fn visit(directory: &Path, sources: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read fixture entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, sources)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "slug")
        {
            sources.push(path);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".fixture.toml"))
        {
            let source = path.with_file_name(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_suffix(".fixture.toml"))
                    .map_or_else(String::new, |name| format!("{name}.slug")),
            );
            if !source.exists() {
                return Err(format!("fixture sidecar {} has no source", path.display()));
            }
        }
    }
    Ok(())
}

fn observed_outcome(success: bool, stderr: &str) -> Result<FixtureOutcome, String> {
    if success {
        return Ok(FixtureOutcome::Success);
    }
    for (prefix, outcome) in [
        ("slug: parse error:", FixtureOutcome::ParseError),
        ("slug: semantic error:", FixtureOutcome::SemanticError),
        ("slug: runtime error:", FixtureOutcome::RuntimeError),
    ] {
        if stderr.starts_with(prefix) {
            return Ok(outcome);
        }
    }
    Err(format!("host crash or unclassified failure: {stderr:?}"))
}
