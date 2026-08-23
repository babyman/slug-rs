use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
};

/// Expected terminal result for one portable conformance fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureOutcome {
    Success,
    ParseError,
    SemanticError,
    RuntimeError,
}

/// Versioned, host-boundary expectations for one Slug fixture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureMetadata {
    pub outcome: FixtureOutcome,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub module_root: Option<PathBuf>,
    pub library_root: Option<PathBuf>,
    pub timeout_ms: Option<u64>,
    pub diagnostic: Option<String>,
}

/// A checked failure while reading portable fixture metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureMetadataError {
    message: String,
}

impl fmt::Display for FixtureMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FixtureMetadataError {}

impl FixtureMetadata {
    /// Reads and validates a version-1 fixture TOML sidecar.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable, malformed, or unsupported sidecar.
    pub fn load(path: &Path) -> Result<Self, FixtureMetadataError> {
        let source = fs::read_to_string(path).map_err(|error| FixtureMetadataError {
            message: format!("cannot read {}: {error}", path.display()),
        })?;
        let document = source
            .parse::<toml::Value>()
            .map_err(|error| FixtureMetadataError {
                message: format!("cannot parse {}: {error}", path.display()),
            })?;
        let toml::Value::Table(mut fields) = document else {
            return Err(error("fixture metadata must be a TOML table"));
        };
        let schema = integer(&mut fields, "schema")?;
        if schema != 1 {
            return Err(error(format!(
                "unsupported fixture metadata schema {schema}"
            )));
        }
        let outcome = match string(&mut fields, "outcome")?.as_str() {
            "success" => FixtureOutcome::Success,
            "parse-error" => FixtureOutcome::ParseError,
            "semantic-error" => FixtureOutcome::SemanticError,
            "runtime-error" => FixtureOutcome::RuntimeError,
            value => return Err(error(format!("invalid fixture outcome `{value}`"))),
        };
        let stdout = optional_string(&mut fields, "stdout")?;
        let stderr = optional_string(&mut fields, "stderr")?;
        let module_root = optional_path(&mut fields, "module_root")?;
        let library_root = optional_path(&mut fields, "library_root")?;
        let timeout_ms = optional_integer(&mut fields, "timeout_ms")?
            .map(|value| u64::try_from(value).map_err(|_| error("timeout_ms must be positive")))
            .transpose()?;
        if timeout_ms == Some(0) {
            return Err(error("timeout_ms must be positive"));
        }
        let diagnostic = optional_string(&mut fields, "diagnostic")?;
        if outcome == FixtureOutcome::Success && diagnostic.is_some() {
            return Err(error("success fixtures cannot declare a diagnostic"));
        }
        if let Some(field) = fields.keys().next() {
            return Err(error(format!("unknown fixture metadata field `{field}`")));
        }
        Ok(Self {
            outcome,
            stdout,
            stderr,
            module_root,
            library_root,
            timeout_ms,
            diagnostic,
        })
    }
}

fn error(message: impl Into<String>) -> FixtureMetadataError {
    FixtureMetadataError {
        message: message.into(),
    }
}

fn integer(
    fields: &mut toml::map::Map<String, toml::Value>,
    name: &str,
) -> Result<i64, FixtureMetadataError> {
    optional_integer(fields, name)?
        .ok_or_else(|| error(format!("missing fixture metadata field `{name}`")))
}

fn optional_integer(
    fields: &mut toml::map::Map<String, toml::Value>,
    name: &str,
) -> Result<Option<i64>, FixtureMetadataError> {
    fields
        .remove(name)
        .map(|value| {
            value.as_integer().ok_or_else(|| {
                error(format!(
                    "fixture metadata field `{name}` must be an integer"
                ))
            })
        })
        .transpose()
}

fn string(
    fields: &mut toml::map::Map<String, toml::Value>,
    name: &str,
) -> Result<String, FixtureMetadataError> {
    optional_string(fields, name)?
        .ok_or_else(|| error(format!("missing fixture metadata field `{name}`")))
}

fn optional_string(
    fields: &mut toml::map::Map<String, toml::Value>,
    name: &str,
) -> Result<Option<String>, FixtureMetadataError> {
    fields
        .remove(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| error(format!("fixture metadata field `{name}` must be a string")))
        })
        .transpose()
}

fn optional_path(
    fields: &mut toml::map::Map<String, toml::Value>,
    name: &str,
) -> Result<Option<PathBuf>, FixtureMetadataError> {
    optional_string(fields, name)?.map_or(Ok(None), |value| {
        let path = PathBuf::from(value);
        if path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
        {
            Ok(Some(path))
        } else {
            Err(error(format!(
                "fixture metadata field `{name}` must be a relative path"
            )))
        }
    })
}
