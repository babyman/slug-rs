//! Target descriptors for static embedded Slug firmware builds.
//!
//! A descriptor is build-host configuration, not a runtime capability source.
//! It selects the entry source, closed module contracts, native provider
//! identities, and finite artifact limits before the frontend compiles a graph.

use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use slug_frontend::ModuleHost;
use slug_loader::{
    ModuleKey, ModuleLoadError, ModuleSource, StaticModuleProvider, StaticModuleResolver,
    module_path,
};
use slug_vm::{EmptyVmConfiguration, NativeDescriptorError, NativeFunction, VmHost, cslug};
use std::rc::Rc;

/// Current supported target-descriptor format version.
pub const FORMAT_VERSION: i64 = 1;

/// Fully validated configuration for one static embedded firmware build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetDescriptor {
    /// Canonical directory containing the descriptor and all selected inputs.
    pub root: PathBuf,
    /// Canonical Slug entry source selected for this firmware image.
    pub entry: PathBuf,
    /// Board package responsible for startup, linking, and flashing.
    pub board_package: String,
    /// Finite limits applied to the generated artifact before firmware linking.
    pub limits: ArtifactLimits,
    /// Ordered source-contract and static-provider selections.
    pub providers: Vec<ProviderSelection>,
}

/// Finite decoder limits for one embedded artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactLimits {
    pub artifact_bytes: usize,
    pub section_bytes: usize,
    pub modules: usize,
    pub imports: usize,
    pub string_bytes: usize,
    pub chunks_per_module: usize,
    pub entries_per_table: usize,
}

/// One source-visible module and its target-native provider identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelection {
    /// Logical module name used by Slug `import(...)` expressions.
    pub module: String,
    /// Canonical source contract path below [`TargetDescriptor::root`].
    pub contract: PathBuf,
    /// Board-package-specific static provider identity.
    pub library: String,
}

/// Closed artifact bytes produced for one embedded target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledArtifact {
    /// Bytes to link into firmware flash.
    pub bytes: Vec<u8>,
    /// Canonical source inputs Cargo must watch for a rebuild.
    pub inputs: Vec<PathBuf>,
}

/// Checked target-descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorError {
    code: &'static str,
    message: String,
}

impl DescriptorError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Stable category for callers that render build diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "embedded target {}: {}", self.code, self.message)
    }
}

impl std::error::Error for DescriptorError {}

/// Checked failure while compiling a descriptor-selected source graph.
#[derive(Debug)]
pub enum CompileError {
    /// Selected source could not be read after descriptor validation.
    Read(DescriptorError),
    /// The closed graph could not be compiled with the selected contracts.
    Source(ModuleLoadError),
    /// The graph contains a value unsupported by version-1 `.cslug`.
    Encode(cslug::Error),
    /// The encoded graph exceeds the target's declared decoder limits.
    Limits(cslug::Error),
    Descriptor(NativeDescriptorError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(formatter),
            Self::Source(error) => error.fmt(formatter),
            Self::Encode(error) => write!(formatter, "cannot encode embedded artifact: {error}"),
            Self::Limits(error) => write!(
                formatter,
                "embedded artifact exceeds target limits: {error}"
            ),
            Self::Descriptor(error) => write!(formatter, "invalid build-time provider: {error}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl TargetDescriptor {
    /// Loads and validates a descriptor from `path`.
    ///
    /// All selected source paths are canonicalized and must remain below the
    /// descriptor directory, including after symlink resolution.
    ///
    /// # Errors
    ///
    /// Returns a checked descriptor error for unreadable, malformed,
    /// incompatible, unbounded, duplicate, or out-of-root configuration.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DescriptorError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|error| {
            DescriptorError::new("read", format!("cannot read {}: {error}", path.display()))
        })?;
        let root = path.parent().ok_or_else(|| {
            DescriptorError::new("path", "descriptor path must have a parent directory")
        })?;
        Self::parse(root, &source)
    }

    /// Parses a descriptor whose relative paths are rooted at `root`.
    ///
    /// This exists for build integrations that already own descriptor input.
    ///
    /// # Errors
    ///
    /// Returns a checked descriptor error under the same rules as [`Self::load`].
    pub fn parse(root: impl AsRef<Path>, source: &str) -> Result<Self, DescriptorError> {
        let root = canonical_directory(root.as_ref(), "descriptor root")?;
        let document = source
            .parse::<toml::Value>()
            .map_err(|error| DescriptorError::new("toml", format!("invalid TOML: {error}")))?;
        let table = document
            .as_table()
            .ok_or_else(|| DescriptorError::new("shape", "descriptor root must be a table"))?;
        require_keys(
            table,
            &["format", "firmware", "limits", "providers"],
            "descriptor",
        )?;
        let format = required_integer(table, "format", "descriptor")?;
        if format != FORMAT_VERSION {
            return Err(DescriptorError::new(
                "version",
                format!("unsupported descriptor format {format}; expected {FORMAT_VERSION}"),
            ));
        }

        let firmware = required_table(table, "firmware", "descriptor")?;
        require_keys(firmware, &["entry", "board_package"], "firmware")?;
        let entry = rooted_file(
            &root,
            required_string(firmware, "entry", "firmware")?,
            "entry",
        )?;
        let board_package = nonempty(
            required_string(firmware, "board_package", "firmware")?,
            "`board_package` in firmware",
        )?;

        let limits = parse_limits(required_table(table, "limits", "descriptor")?)?;
        let providers = parse_providers(&root, required_array(table, "providers", "descriptor")?)?;

        Ok(Self {
            root,
            entry,
            board_package,
            limits,
            providers,
        })
    }

    /// Compiles the selected entry graph and validates its encoded bytes under
    /// this target's finite decoder limits.
    ///
    /// The generated bytes contain only the closed source-derived graph. Native
    /// provider code is linked and registered by the board package separately.
    ///
    /// # Errors
    ///
    /// Returns a checked source, encoding, or target-limit failure.
    pub fn compile_artifact(&self) -> Result<CompiledArtifact, CompileError> {
        self.compile_artifact_with_foreign([])
    }

    /// Compiles after installing build-host-only foreign descriptors.
    ///
    /// These callbacks are used only to resolve source declarations on the
    /// desktop build host. They are never serialized into `.cslug`.
    ///
    /// # Errors
    ///
    /// Returns a checked descriptor, source, encoding, limit, or native
    /// descriptor failure.
    pub fn compile_artifact_with_foreign(
        &self,
        functions: impl IntoIterator<Item = NativeFunction>,
    ) -> Result<CompiledArtifact, CompileError> {
        let source = fs::read_to_string(&self.entry).map_err(|error| {
            CompileError::Read(DescriptorError::new(
                "read",
                format!("cannot read entry {}: {error}", self.entry.display()),
            ))
        })?;
        let mut inputs = vec![self.entry.clone()];
        let providers = self
            .providers
            .iter()
            .map(|provider| {
                let text = fs::read_to_string(&provider.contract).map_err(|error| {
                    CompileError::Read(DescriptorError::new(
                        "read",
                        format!(
                            "cannot read provider contract {}: {error}",
                            provider.contract.display()
                        ),
                    ))
                })?;
                inputs.push(provider.contract.clone());
                StaticModuleProvider::new(
                    provider.module.clone(),
                    ModuleSource {
                        key: ModuleKey::new(format!("embedded:{}", provider.module)),
                        diagnostic_name: provider.contract.display().to_string(),
                        text,
                        activation: None,
                    },
                )
                .map_err(CompileError::Source)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let resolver = StaticModuleResolver::new(providers).map_err(CompileError::Source)?;
        let host = ModuleHost::with_resolver(Rc::new(resolver), Rc::new(EmptyVmConfiguration));
        host.define_foreign_batch(functions.into_iter().collect())
            .map_err(CompileError::Descriptor)?;
        let artifact = host
            .compile_artifact(&self.entry.to_string_lossy(), &source)
            .map_err(CompileError::Source)?;
        let bytes = cslug::encode(&artifact).map_err(CompileError::Encode)?;
        cslug::decode(&bytes, self.cslug_limits()).map_err(CompileError::Limits)?;
        Ok(CompiledArtifact { bytes, inputs })
    }

    /// Converts descriptor limits to the VM artifact decoder configuration.
    #[must_use]
    pub const fn cslug_limits(&self) -> cslug::Limits {
        cslug::Limits {
            artifact_bytes: self.limits.artifact_bytes,
            section_bytes: self.limits.section_bytes,
            modules: self.limits.modules,
            imports: self.limits.imports,
            string_bytes: self.limits.string_bytes,
            chunks_per_module: self.limits.chunks_per_module,
            entries_per_table: self.limits.entries_per_table,
        }
    }
}

fn parse_limits(
    table: &toml::map::Map<String, toml::Value>,
) -> Result<ArtifactLimits, DescriptorError> {
    require_keys(
        table,
        &[
            "artifact_bytes",
            "section_bytes",
            "modules",
            "imports",
            "string_bytes",
            "chunks_per_module",
            "entries_per_table",
        ],
        "limits",
    )?;
    Ok(ArtifactLimits {
        artifact_bytes: positive_usize(table, "artifact_bytes", "limits")?,
        section_bytes: positive_usize(table, "section_bytes", "limits")?,
        modules: positive_usize(table, "modules", "limits")?,
        imports: positive_usize(table, "imports", "limits")?,
        string_bytes: positive_usize(table, "string_bytes", "limits")?,
        chunks_per_module: positive_usize(table, "chunks_per_module", "limits")?,
        entries_per_table: positive_usize(table, "entries_per_table", "limits")?,
    })
}

fn parse_providers(
    root: &Path,
    values: &[toml::Value],
) -> Result<Vec<ProviderSelection>, DescriptorError> {
    if values.is_empty() {
        return Err(DescriptorError::new(
            "providers",
            "`providers` must select at least one module",
        ));
    }
    let mut modules = HashSet::new();
    let mut providers = Vec::with_capacity(values.len());
    for value in values {
        let table = value.as_table().ok_or_else(|| {
            DescriptorError::new("shape", "every `providers` entry must be a table")
        })?;
        require_keys(table, &["module", "contract", "library"], "provider")?;
        let module = nonempty(
            required_string(table, "module", "provider")?,
            "`module` in provider",
        )?;
        module_path(&module).map_err(|_| {
            DescriptorError::new("module", format!("invalid provider module `{module}`"))
        })?;
        if !modules.insert(module.clone()) {
            return Err(DescriptorError::new(
                "duplicate-module",
                format!("provider module `{module}` is selected more than once"),
            ));
        }
        let contract = rooted_file(
            root,
            required_string(table, "contract", "provider")?,
            "provider contract",
        )?;
        let library = nonempty(
            required_string(table, "library", "provider")?,
            "`library` in provider",
        )?;
        providers.push(ProviderSelection {
            module,
            contract,
            library,
        });
    }
    Ok(providers)
}

fn require_keys(
    table: &toml::map::Map<String, toml::Value>,
    expected: &[&str],
    context: &str,
) -> Result<(), DescriptorError> {
    for key in table.keys() {
        if !expected.contains(&key.as_str()) {
            return Err(DescriptorError::new(
                "field",
                format!("unknown field `{key}` in {context}"),
            ));
        }
    }
    for key in expected {
        if !table.contains_key(*key) {
            return Err(DescriptorError::new(
                "field",
                format!("missing required field `{key}` in {context}"),
            ));
        }
    }
    Ok(())
}

fn required_table<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<&'a toml::map::Map<String, toml::Value>, DescriptorError> {
    table
        .get(key)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            DescriptorError::new("shape", format!("`{key}` in {context} must be a table"))
        })
}

fn required_array<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<&'a [toml::Value], DescriptorError> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| {
            DescriptorError::new("shape", format!("`{key}` in {context} must be an array"))
        })
}

fn required_string<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, DescriptorError> {
    table.get(key).and_then(toml::Value::as_str).ok_or_else(|| {
        DescriptorError::new("shape", format!("`{key}` in {context} must be a string"))
    })
}

fn required_integer(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<i64, DescriptorError> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            DescriptorError::new("shape", format!("`{key}` in {context} must be an integer"))
        })
}

fn positive_usize(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    context: &str,
) -> Result<usize, DescriptorError> {
    let value = required_integer(table, key, context)?;
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            DescriptorError::new(
                "limit",
                format!("`{key}` in {context} must be a positive integer"),
            )
        })
}

fn nonempty(value: &str, context: &str) -> Result<String, DescriptorError> {
    (!value.trim().is_empty())
        .then(|| value.to_owned())
        .ok_or_else(|| DescriptorError::new("value", format!("{context} must not be empty")))
}

fn canonical_directory(path: &Path, description: &str) -> Result<PathBuf, DescriptorError> {
    let path = fs::canonicalize(path).map_err(|error| {
        DescriptorError::new(
            "path",
            format!("cannot resolve {description} {}: {error}", path.display()),
        )
    })?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(DescriptorError::new(
            "path",
            format!("{description} {} is not a directory", path.display()),
        ))
    }
}

fn rooted_file(root: &Path, value: &str, description: &str) -> Result<PathBuf, DescriptorError> {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        return Err(DescriptorError::new(
            "path",
            format!("{description} path `{value}` must be relative to the descriptor root"),
        ));
    }
    let path = fs::canonicalize(root.join(candidate)).map_err(|error| {
        DescriptorError::new(
            "path",
            format!("cannot resolve {description} `{value}`: {error}"),
        )
    })?;
    if !path.starts_with(root) {
        return Err(DescriptorError::new(
            "path",
            format!("{description} path `{value}` escapes the descriptor root"),
        ));
    }
    path.is_file().then_some(path).ok_or_else(|| {
        DescriptorError::new(
            "path",
            format!("{description} path `{value}` is not a file"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{ArtifactLimits, CompileError, FORMAT_VERSION, TargetDescriptor};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("slug-embedded-build-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("contracts")).expect("create fixture directories");
            fs::write(root.join("app.slug"), "val main = fn() { nil }\n")
                .expect("write entry source");
            fs::write(
                root.join("contracts/gpio.slug"),
                "export foreign write = fn():nil\n",
            )
            .expect("write source contract");
            Self { root }
        }

        fn descriptor(&self, providers: &str) -> String {
            format!(
                "format = {FORMAT_VERSION}\n\
                 [firmware]\n\
                 entry = \"app.slug\"\n\
                 board_package = \"slug-esp32-s3\"\n\
                 [limits]\n\
                 artifact_bytes = 16384\n\
                 section_bytes = 8192\n\
                 modules = 4\n\
                 imports = 3\n\
                 string_bytes = 128\n\
                 chunks_per_module = 8\n\
                 entries_per_table = 64\n\
                 {providers}"
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove descriptor fixture");
        }
    }

    #[test]
    fn parses_a_bounded_descriptor() {
        let fixture = Fixture::new("valid");
        let descriptor = TargetDescriptor::parse(
            &fixture.root,
            &fixture.descriptor(
                "[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n",
            ),
        )
        .expect("parse descriptor");

        assert_eq!(descriptor.board_package, "slug-esp32-s3");
        assert_eq!(descriptor.providers.len(), 1);
        assert_eq!(descriptor.providers[0].module, "slug.gpio");
        assert_eq!(
            descriptor.limits,
            ArtifactLimits {
                artifact_bytes: 16_384,
                section_bytes: 8_192,
                modules: 4,
                imports: 3,
                string_bytes: 128,
                chunks_per_module: 8,
                entries_per_table: 64,
            }
        );
    }

    #[test]
    fn rejects_incompatible_duplicate_and_unbounded_inputs() {
        let fixture = Fixture::new("invalid");
        let incompatible = TargetDescriptor::parse(
            &fixture.root,
            &fixture
                .descriptor("[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n")
                .replacen("format = 1", "format = 2", 1),
        )
        .expect_err("reject version");
        assert_eq!(incompatible.code(), "version");

        let duplicate = TargetDescriptor::parse(
            &fixture.root,
            &fixture.descriptor(
                "[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n\
                 [[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"second-provider\"\n",
            ),
        )
        .expect_err("reject duplicate module");
        assert_eq!(duplicate.code(), "duplicate-module");

        let unbounded = TargetDescriptor::parse(
            &fixture.root,
            &fixture
                .descriptor("[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n")
                .replacen("artifact_bytes = 16384", "artifact_bytes = 0", 1),
        )
        .expect_err("reject zero limit");
        assert_eq!(unbounded.code(), "limit");
    }

    #[test]
    fn rejects_paths_that_escape_the_descriptor_root() {
        let fixture = Fixture::new("paths");
        let outside = fixture.root.with_extension("outside.slug");
        fs::write(&outside, "export val escaped = true\n").expect("write outside source");
        let outside_name = outside
            .file_name()
            .expect("outside fixture has a file name")
            .to_string_lossy();
        let error = TargetDescriptor::parse(
            &fixture.root,
            &fixture.descriptor(&format!(
                "[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"../{outside_name}\"\nlibrary = \"gpio-provider\"\n"
            )),
        )
        .expect_err("reject out-of-root contract");
        fs::remove_file(outside).expect("remove outside source");
        assert_eq!(error.code(), "path");
    }

    #[test]
    fn compiles_a_closed_artifact_under_descriptor_limits() {
        let fixture = Fixture::new("compile");
        fs::write(
            fixture.root.join("app.slug"),
            "val gpio = import(\"slug.gpio\")\nval main = fn() { gpio.write() }\n",
        )
        .expect("write entry source");
        let descriptor = TargetDescriptor::parse(
            &fixture.root,
            &fixture.descriptor(
                "[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n",
            ),
        )
        .expect("parse descriptor");

        let artifact = descriptor.compile_artifact().expect("compile artifact");
        assert!(!artifact.bytes.is_empty());
        assert_eq!(artifact.inputs.len(), 2);
    }

    #[test]
    fn produces_distinct_artifacts_and_rejects_unselected_imports() {
        let fixture = Fixture::new("closure");
        let descriptor = TargetDescriptor::parse(
            &fixture.root,
            &fixture.descriptor(
                "[[providers]]\nmodule = \"slug.gpio\"\ncontract = \"contracts/gpio.slug\"\nlibrary = \"gpio-provider\"\n",
            ),
        )
        .expect("parse descriptor");
        let first = descriptor
            .compile_artifact()
            .expect("compile first artifact");
        fs::write(fixture.root.join("app.slug"), "val main = fn() { 42 }\n")
            .expect("replace entry source");
        let second = descriptor
            .compile_artifact()
            .expect("compile second artifact");
        assert_ne!(first.bytes, second.bytes);

        fs::write(
            fixture.root.join("app.slug"),
            "val main = fn() { import(\"slug.unselected\") }\n",
        )
        .expect("write unselected import");
        assert!(matches!(
            descriptor.compile_artifact(),
            Err(CompileError::Source(_))
        ));
    }
}
