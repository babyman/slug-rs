//! Desktop external-import resolution and import-scoped activation.
//!
//! Callers provide already-selected project and library roots. This crate does
//! not discover process environment, command-line arguments, or configuration.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use slug_loader::{
    ModuleActivation, ModuleKey, ModuleLoadError, ModuleRequest, ModuleResolver, ModuleSource,
    module_path,
};
use slug_vm::{
    ClutchRepository, FfiPrototypeLibrary,
    clutch::{self, StagedClutchPlugin},
};

/// Filesystem resolver for an explicitly configured desktop module search path.
#[derive(Clone, Debug)]
pub struct DesktopResolver {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
    clutch_repository: ClutchRepository,
    activation_sources: Rc<RefCell<HashMap<ModuleActivation, ClutchPluginSource>>>,
}

impl DesktopResolver {
    /// Creates a resolver using caller-selected project and library roots.
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self::with_clutch_repository(source_root, library_root, ClutchRepository::default())
    }

    /// Creates a resolver with an explicit desktop Clutch repository.
    #[must_use]
    pub fn with_clutch_repository(
        source_root: impl Into<PathBuf>,
        library_root: Option<PathBuf>,
        clutch_repository: ClutchRepository,
    ) -> Self {
        Self {
            source_root: source_root.into(),
            library_root,
            clutch_repository,
            activation_sources: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Stages the native registrations required by a resolved module.
    ///
    /// The caller owns publication and cleanup of the returned transaction.
    /// This keeps desktop discovery separate from the frontend module graph.
    ///
    /// # Errors
    ///
    /// Returns a checked error when an activation lease is invalid or its
    /// plugin cannot be prepared.
    pub fn stage_activation(
        &self,
        source: &ModuleSource,
    ) -> Result<Option<StagedClutchPlugin>, ModuleLoadError> {
        let Some(lease) = &source.activation else {
            return Ok(None);
        };
        let plugin = self
            .activation_sources
            .borrow()
            .get(lease)
            .cloned()
            .ok_or_else(|| ModuleLoadError::Clutch {
                location: source.diagnostic_name.clone(),
                message: "module activation lease is no longer available".into(),
            })?;
        let mut registrar = clutch::ClutchPluginRegistrar::new(plugin.module_names().to_vec());
        let result = match &plugin {
            ClutchPluginSource::Host { root, entry, .. } => {
                let initializer = self.clutch_repository.plugin(entry).ok_or_else(|| {
                    ModuleLoadError::Clutch {
                        location: root.to_string_lossy().into_owned(),
                        message: format!("plugin entry `{entry}` is not configured by the host"),
                    }
                })?;
                initializer(&mut registrar).map_err(|error| ModuleLoadError::Clutch {
                    location: root.to_string_lossy().into_owned(),
                    message: format!("plugin initialization failed: {error}"),
                })
            }
            ClutchPluginSource::Native {
                root, library, abi, ..
            } => {
                if abi == slug_vm::ABI_PROFILE {
                    let module = FfiPrototypeLibrary::load(library).map_err(|error| {
                        ModuleLoadError::Clutch {
                            location: library.to_string_lossy().into_owned(),
                            message: format!("cannot load native plugin: {error}"),
                        }
                    })?;
                    module
                        .stage(&mut registrar)
                        .map_err(|error| ModuleLoadError::Clutch {
                            location: root.to_string_lossy().into_owned(),
                            message: format!("native plugin initialization failed: {error}"),
                        })?;
                    registrar
                        .set_cleanup_operation(move || {
                            module.shutdown();
                            Ok(())
                        })
                        .map_err(|error| ModuleLoadError::Clutch {
                            location: root.to_string_lossy().into_owned(),
                            message: format!("cannot retain native plugin cleanup: {error}"),
                        })
                } else {
                    Err(ModuleLoadError::Clutch {
                        location: root.to_string_lossy().into_owned(),
                        message: format!("unsupported native ABI `{abi}`"),
                    })
                }
            }
        };
        if let Err(error) = result {
            let mut staged = registrar.finish();
            let _ = staged.cleanup();
            return Err(error);
        }
        Ok(Some(registrar.finish()))
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
        if let Some(root) = self.clutch_repository.provider(request.name) {
            let module = clutch::load_module(root, request.name).map_err(|message| {
                ModuleLoadError::Clutch {
                    location: root.to_string_lossy().into_owned(),
                    message,
                }
            })?;
            let text = fs::read_to_string(&module.path).map_err(|error| ModuleLoadError::Read {
                location: module.path.to_string_lossy().into_owned(),
                message: error.to_string(),
            })?;
            let activation = match (module.plugin_entry, module.native_plugin) {
                (Some(entry), None) => Some(ClutchPluginSource::Host {
                    root: module.root,
                    entry,
                    module_names: vec![request.name.into()],
                }),
                (None, Some(native)) => Some(ClutchPluginSource::Native {
                    root: module.root,
                    library: native.library,
                    abi: native.abi,
                    module_names: module.module_names,
                }),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("clutch manifest validation is inconsistent"),
            };
            let lease = activation.as_ref().map(|plugin| {
                let lease = ModuleActivation::new(plugin.root().to_string_lossy().into_owned());
                self.activation_sources
                    .borrow_mut()
                    .insert(lease.clone(), plugin.clone());
                lease
            });
            return Ok(ModuleSource {
                key: ModuleKey::new(module.path.to_string_lossy()),
                diagnostic_name: module.path.to_string_lossy().into_owned(),
                text,
                activation: lease,
            });
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClutchPluginSource {
    Host {
        root: PathBuf,
        entry: String,
        module_names: Vec<String>,
    },
    Native {
        root: PathBuf,
        library: PathBuf,
        abi: String,
        module_names: Vec<String>,
    },
}

impl ClutchPluginSource {
    fn root(&self) -> &Path {
        match self {
            Self::Host { root, .. } | Self::Native { root, .. } => root,
        }
    }

    fn module_names(&self) -> &[String] {
        match self {
            Self::Host { module_names, .. } | Self::Native { module_names, .. } => module_names,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use slug_loader::{ModuleKey, ModuleRequest, ModuleResolver};
    use slug_vm::ClutchRepository;

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

    #[test]
    fn resolves_explicit_clutch_modules_with_activation_leases() {
        let root = root();
        let clutch = root.join("example.clutch");
        fs::create_dir_all(clutch.join("modules")).expect("create clutch module directory");
        fs::write(
            clutch.join("modules/library.slug"),
            "export val answer = 42\n",
        )
        .expect("write clutch module");
        fs::write(
            clutch.join("clutch.toml"),
            "[modules]\n\"example.library\" = { source = \"modules/library.slug\", plugin = \"test.plugin\" }\n",
        )
        .expect("write clutch manifest");
        let repository = ClutchRepository::new(vec![("example.library".into(), clutch)])
            .expect("create clutch repository");
        let resolver = DesktopResolver::with_clutch_repository(&root, None, repository);

        let source = resolver
            .resolve(ModuleRequest::new(None, "example.library"))
            .expect("resolve clutch module");

        assert_eq!(source.text, "export val answer = 42\n");
        assert!(source.activation.is_some());
        fs::remove_dir_all(root).expect("remove temporary resolver directory");
    }
}
