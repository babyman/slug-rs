//! Restricted-host external import loader.
//!
//! This loader deliberately has no filesystem, environment, configuration,
//! Clutch, network, or native-library dependency.

use slug_loader::{ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource};

/// A resolver that declines every external import.
#[derive(Clone, Copy, Debug, Default)]
pub struct NilLoader;

impl ModuleResolver for NilLoader {
    fn resolve(&self, request: ModuleRequest<'_>) -> Result<ModuleSource, ModuleLoadError> {
        Err(ModuleLoadError::NotFound {
            name: request.name.into(),
            searched: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use slug_loader::{ModuleKey, ModuleResolver};

    use super::NilLoader;

    #[test]
    fn rejects_external_imports_without_a_resolution_path() {
        let importer = ModuleKey::new("memory:main");
        let error = NilLoader
            .resolve(slug_loader::ModuleRequest::new(
                Some(&importer),
                "example.module",
            ))
            .expect_err("nil loader must reject every external import");
        assert_eq!(
            error,
            slug_loader::ModuleLoadError::NotFound {
                name: "example.module".into(),
                searched: Vec::new(),
            }
        );
    }
}
