use std::{
    cell::RefCell,
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{Program, Value, Vm, compile};

/// Host-owned roots used to load Slug module source.
#[derive(Clone, Debug)]
pub struct ModuleLoader {
    state: Rc<ModuleLoaderState>,
}

#[derive(Debug)]
struct ModuleLoaderState {
    source_root: PathBuf,
    library_root: Option<PathBuf>,
    compiled: RefCell<HashMap<PathBuf, Program>>,
    instances: RefCell<HashMap<PathBuf, ModuleInstance>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSource {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct ModuleInstance {
    pub path: PathBuf,
    pub program: Program,
    pub exports: Value,
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
    Source {
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
            Self::Source { path, message } => {
                write!(f, "cannot compile {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ModuleLoadError {}

impl ModuleLoader {
    #[must_use]
    pub fn new(source_root: impl Into<PathBuf>, library_root: Option<PathBuf>) -> Self {
        Self {
            state: Rc::new(ModuleLoaderState {
                source_root: source_root.into(),
                library_root,
                compiled: RefCell::new(HashMap::new()),
                instances: RefCell::new(HashMap::new()),
            }),
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
        candidates.push(self.state.source_root.join(&relative));
        if let Some(library_root) = &self.state.library_root {
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

    /// Loads and compiles a module, returning a cached program for repeat requests.
    ///
    /// # Errors
    ///
    /// Returns an error for loader failures or invalid module source.
    pub fn compile(&self, importer: Option<&Path>, name: &str) -> Result<Program, ModuleLoadError> {
        let source = self.load(importer, name)?;
        if let Some(program) = self.state.compiled.borrow().get(&source.path) {
            return Ok(program.clone());
        }
        let program = compile(&source.path.to_string_lossy(), &source.text).map_err(|error| {
            ModuleLoadError::Source {
                path: source.path.clone(),
                message: error.to_string(),
            }
        })?;
        self.state
            .compiled
            .borrow_mut()
            .insert(source.path, program.clone());
        Ok(program)
    }

    #[must_use]
    pub fn cached_module_count(&self) -> usize {
        self.state.compiled.borrow().len()
    }

    /// Compiles and initializes one isolated module instance.
    ///
    /// # Errors
    ///
    /// Returns checked loader, source, or module-runtime failures.
    pub fn initialize(
        &self,
        importer: Option<&Path>,
        name: &str,
    ) -> Result<ModuleInstance, ModuleLoadError> {
        let source = self.load(importer, name)?;
        if let Some(instance) = self.state.instances.borrow().get(&source.path) {
            return Ok(instance.clone());
        }
        let program = self.compile(importer, name)?;
        let mut vm = Vm::with_module_loader(self.clone());
        vm.run_named(&program, "main")
            .map_err(|error| ModuleLoadError::Source {
                path: source.path.clone(),
                message: error.to_string(),
            })?;
        let exports = vm.exported_values(&program);
        let instance = ModuleInstance {
            path: source.path,
            program,
            exports,
        };
        self.state
            .instances
            .borrow_mut()
            .insert(instance.path.clone(), instance.clone());
        Ok(instance)
    }

    #[must_use]
    pub fn initialized_module_count(&self) -> usize {
        self.state.instances.borrow().len()
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
