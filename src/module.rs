use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

/// Host-owned roots used to load Slug module source.
#[derive(Clone, Debug)]
pub struct ModuleLoader {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSource {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleLoadError {
    InvalidName(String),
    NotFound {
        name: String,
        searched: Vec<PathBuf>,
    },
    Read {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ModuleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(f, "invalid module name `{name}`"),
            Self::NotFound { name, .. } => write!(f, "module `{name}` was not found"),
            Self::Read { path, message } => write!(f, "cannot read {}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ModuleLoadError {}

impl ModuleLoader {
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self {
            source_root: source_root.into(),
            library_root,
        }
    }

    /// Loads a dotted module name without exposing file-system operations to Slug code.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid name, an unavailable module, or a host read failure.
    pub fn load(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleSource, ModuleLoadError> {
        let relative = module_path(name)?;
        let mut candidates = Vec::new();
        if let Some(importer) = importer.and_then(Path::parent) {
            candidates.push(importer.join(&relative));
        }
        candidates.push(self.source_root.join(&relative));
        if let Some(library_root) = &self.library_root {
            candidates.push(library_root.join(&relative));
        }
        for path in &candidates {
            match fs::read_to_string(path) {
                Ok(text) => {
                    return Ok(ModuleSource {
                        path: path.clone(),
                        text,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ModuleLoadError::Read {
                        path: path.clone(),
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(ModuleLoadError::NotFound {
            name: name.into(),
            searched: candidates,
        })
    }
}

fn module_path(name: &str) -> Result<PathBuf, ModuleLoadError> {
    let mut path = PathBuf::new();
    for part in name.split('.') {
        if part.is_empty()
            || !part
                .chars()
                .all(|value| value == '_' || value.is_ascii_alphanumeric())
        {
            return Err(ModuleLoadError::InvalidName(name.into()));
        }
        path.push(part);
    }
    path.set_extension("slug");
    Ok(path)
}
