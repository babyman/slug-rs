//! Desktop external-import resolution and import-scoped activation.
//!
//! Callers provide already-selected project and library roots. This crate does
//! not discover process environment, command-line arguments, or configuration.

use std::{
    fs,
    path::{Path, PathBuf},
};

use slug_loader::{ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource};

/// Filesystem resolver for an explicitly configured desktop module search path.
#[derive(Clone, Debug)]
pub struct DesktopResolver {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
}

impl DesktopResolver {
    /// Creates a resolver using caller-selected project and library roots.
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self {
            source_root: source_root.into(),
            library_root,
        }
    }
}

impl ModuleResolver for DesktopResolver {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        let relative = module_path(request.name)?;
        let mut candidates = Vec::new();
        if let Some(importer) = request
            .importer
            .map(ModuleKey::as_str)
            .map(Path::new)
            .and_then(Path::parent)
        {
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
                        key: ModuleKey::new(path.to_string_lossy()),
                        diagnostic_name: path.to_string_lossy().into_owned(),
                        text,
                        activation: None,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ModuleLoadError::Read {
                        location: path.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(ModuleLoadError::NotFound {
            name: request.name.into(),
            searched: candidates
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use slug_loader::{ModuleKey, ModuleRequest, ModuleResolver};

    use super::DesktopResolver;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "slug-desktop-resolver-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn resolves_importer_relative_then_project_then_library_modules() {
        let root = root();
        let project = root.join("project");
        let library = root.join("library");
        fs::create_dir_all(project.join("nested")).expect("create project directory");
        fs::create_dir_all(&library).expect("create library directory");
        fs::write(project.join("nested/math.slug"), "relative").expect("write relative module");
        fs::write(project.join("fallback.slug"), "project").expect("write project module");
        fs::write(library.join("library.slug"), "library").expect("write library module");

        let resolver = DesktopResolver::new(&project, Some(library.clone()));
        let importer = ModuleKey::new(project.join("nested/main.slug").to_string_lossy());
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(Some(&importer), "math"))
                .expect("resolve relative module")
                .text,
            "relative"
        );
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(None, "fallback"))
                .expect("resolve project module")
                .text,
            "project"
        );
        assert_eq!(
            resolver
                .resolve(ModuleRequest::new(None, "library"))
                .expect("resolve library module")
                .text,
            "library"
        );
        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }
}
