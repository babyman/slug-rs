//! Typed providers for static images: validate each selected clutch, check its
//! bindings against the module contracts, compile its generated shim with the
//! target C toolchain, and generate the Rust registration that installs it
//! through the shared adapter.
//!
//! See `docs/decisions/2026-10-11-static-typed-provider-selection.md`.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use slug_native_manifest::{
    Binding, NativeManifest, PRODUCER_HEADER, PRODUCER_HEADER_NAME, Profile, SHIM_C_FLAGS,
    check_foreign_declarations, generate, table_function,
};

use crate::{DescriptorError, ProviderKind, TargetDescriptor};

/// A C compiler and archiver for the target, with target-specific flags
/// added to the shim flags.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CToolchain {
    pub compiler: String,
    pub archiver: String,
    pub flags: Vec<String>,
}

impl CToolchain {
    /// The build host's toolchain: `$CC` or `cc` (`clang` on Windows), `$AR`
    /// or `ar` (`llvm-ar` on Windows), and `-fPIC` where Rust links PIE.
    #[must_use]
    pub fn host() -> Self {
        let archiver = std::env::var("AR")
            .unwrap_or_else(|_| if cfg!(windows) { "llvm-ar" } else { "ar" }.into());
        Self {
            compiler: slug_native_manifest::c_compiler(),
            archiver,
            flags: if cfg!(windows) {
                Vec::new()
            } else {
                vec!["-fPIC".into()]
            },
        }
    }
}

/// One clutch selected as a typed provider for one or more modules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedProvider {
    /// C identifier naming the shim's table function and static library.
    pub name: String,
    pub clutch: PathBuf,
    pub manifest: NativeManifest,
    pub modules: Vec<String>,
    /// Bindings whose text result is declared `str` without `nil`.
    pub non_null_text: Vec<(String, String)>,
}

/// Outputs of [`TargetDescriptor::build_typed_providers`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypedBuild {
    /// Rust source to `include!` in the board package. It defines
    /// `typed_provider_functions(owner)`, which installs every typed
    /// provider; the including crate must have `alloc` in scope.
    pub registration: String,
    /// Directory holding the static libraries.
    pub library_directory: PathBuf,
    /// Static library names, without `lib` or extension.
    pub libraries: Vec<String>,
    /// Files whose change must rebuild the board package.
    pub inputs: Vec<PathBuf>,
}

impl TypedBuild {
    /// Cargo build-script directives that link the static libraries.
    #[must_use]
    pub fn cargo_directives(&self) -> Vec<String> {
        let mut directives = Vec::new();
        if !self.libraries.is_empty() {
            directives.push(format!(
                "cargo:rustc-link-search=native={}",
                self.library_directory.display()
            ));
        }
        for library in &self.libraries {
            directives.push(format!("cargo:rustc-link-lib=static={library}"));
        }
        for input in &self.inputs {
            directives.push(format!("cargo:rerun-if-changed={}", input.display()));
        }
        directives
    }
}

impl TargetDescriptor {
    /// Loads and checks every typed provider: one per selected clutch.
    ///
    /// # Errors
    ///
    /// Returns a checked error before any compilation when a clutch manifest is
    /// invalid, does not provide a selected module from its contract, binds a
    /// module the descriptor does not select from it, needs a profile the
    /// target does not offer, or does not exactly cover the module's
    /// `foreign` declarations with compatible types.
    pub fn typed_providers(&self) -> Result<Vec<TypedProvider>, DescriptorError> {
        let mut by_clutch = BTreeMap::<PathBuf, Vec<&crate::ProviderSelection>>::new();
        for provider in &self.providers {
            if let ProviderKind::Typed { clutch } = &provider.kind {
                by_clutch.entry(clutch.clone()).or_default().push(provider);
            }
        }
        let mut typed = Vec::new();
        for (clutch, selections) in by_clutch {
            let provider = load_provider(&clutch, &selections, &self.profiles)?;
            if typed
                .iter()
                .any(|other: &TypedProvider| other.name == provider.name)
            {
                return Err(DescriptorError::new(
                    "typed-provider",
                    format!(
                        "typed provider name `{}` is not unique; rename one clutch",
                        provider.name
                    ),
                ));
            }
            typed.push(provider);
        }
        Ok(typed)
    }

    /// Checks and builds every typed provider into `output`: one generated
    /// shim and one static library per clutch, plus the Rust registration.
    ///
    /// # Errors
    ///
    /// Returns the checked errors of [`Self::typed_providers`], or a `typed-build`
    /// error with compiler diagnostics when a shim fails its header checks or a
    /// file cannot be written.
    pub fn build_typed_providers(
        &self,
        toolchain: &CToolchain,
        output: &Path,
    ) -> Result<TypedBuild, DescriptorError> {
        let providers = self.typed_providers()?;
        let mut build = TypedBuild {
            registration: registration(&providers),
            library_directory: output.to_path_buf(),
            ..TypedBuild::default()
        };
        for provider in &providers {
            build_provider(provider, toolchain, output, &mut build)?;
        }
        Ok(build)
    }
}

fn load_provider(
    clutch: &Path,
    selections: &[&crate::ProviderSelection],
    profiles: &[String],
) -> Result<TypedProvider, DescriptorError> {
    let manifest_path = clutch.join("clutch.toml");
    let source = fs::read_to_string(&manifest_path).map_err(|error| {
        DescriptorError::new(
            "read",
            format!("cannot read {}: {error}", manifest_path.display()),
        )
    })?;
    let invalid = |message: String| {
        DescriptorError::new(
            "typed-provider",
            format!("{}: {message}", manifest_path.display()),
        )
    };
    let manifest =
        NativeManifest::from_clutch_source(&source).map_err(|error| invalid(error.to_string()))?;
    let module_sources = clutch_module_sources(&source).map_err(&invalid)?;
    let required = match manifest.profile {
        Profile::Core => "core",
        Profile::Events => "events",
    };
    if !profiles.iter().any(|profile| profile == required) {
        return Err(invalid(format!(
            "the provider needs the `{required}` profile, which the target does not offer"
        )));
    }
    let selected = selections
        .iter()
        .map(|selection| selection.module.as_str())
        .collect::<Vec<_>>();
    if let Some(module) = manifest
        .bindings()
        .map(|binding| binding.module.as_str())
        .find(|module| !selected.contains(module))
    {
        return Err(invalid(format!(
            "the clutch binds module `{module}`, which the descriptor does not select from it"
        )));
    }
    let mut non_null_text = Vec::new();
    for selection in selections {
        let module = &selection.module;
        let declared = module_sources
            .get(module)
            .map(|source| clutch.join(source))
            .and_then(|path| fs::canonicalize(path).ok());
        if declared.as_deref() != Some(selection.contract.as_path()) {
            return Err(invalid(format!(
                "module `{module}` must be provided by the clutch from the selected contract {}",
                selection.contract.display()
            )));
        }
        let text = fs::read_to_string(&selection.contract).map_err(|error| {
            DescriptorError::new(
                "read",
                format!("cannot read {}: {error}", selection.contract.display()),
            )
        })?;
        let declarations =
            slug_frontend::foreign_declarations(&selection.contract.display().to_string(), &text)
                .map_err(|error| invalid(error.to_string()))?;
        let names =
            check_foreign_declarations(&manifest, module, &declarations).map_err(&invalid)?;
        non_null_text.extend(names.into_iter().map(|name| (module.clone(), name)));
    }
    let name = clutch
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_suffix(".clutch").unwrap_or(name))
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if name.is_empty() || name.starts_with(|character: char| character.is_ascii_digit()) {
        return Err(invalid(
            "the clutch directory name is not a usable provider name".into(),
        ));
    }
    Ok(TypedProvider {
        name,
        clutch: clutch.to_path_buf(),
        manifest,
        modules: selected.into_iter().map(str::to_owned).collect(),
        non_null_text,
    })
}

/// Reads `[modules]` from a clutch manifest: module name to source path.
fn clutch_module_sources(source: &str) -> Result<BTreeMap<String, String>, String> {
    let value = source
        .parse::<toml::Value>()
        .map_err(|error| format!("invalid TOML: {error}"))?;
    let modules = value
        .get("modules")
        .and_then(toml::Value::as_table)
        .ok_or("missing `modules` table")?;
    modules
        .iter()
        .map(|(name, entry)| {
            entry
                .get("source")
                .and_then(toml::Value::as_str)
                .map(|source| (name.clone(), source.to_owned()))
                .ok_or_else(|| format!("module `{name}` must name a `source`"))
        })
        .collect()
}

fn build_provider(
    provider: &TypedProvider,
    toolchain: &CToolchain,
    output: &Path,
    build: &mut TypedBuild,
) -> Result<(), DescriptorError> {
    let failure = |message: String| DescriptorError::new("typed-build", message);
    let work = output.join(format!("typed-{}", provider.name));
    fs::create_dir_all(&work)
        .map_err(|error| failure(format!("cannot create {}: {error}", work.display())))?;
    let shim = work.join(format!("{}_shim.c", provider.name));
    let text =
        generate(&provider.manifest, &provider.name).map_err(|error| failure(error.to_string()))?;
    fs::write(&shim, text)
        .map_err(|error| failure(format!("cannot write {}: {error}", shim.display())))?;
    let header = work.join(PRODUCER_HEADER_NAME);
    fs::write(&header, PRODUCER_HEADER)
        .map_err(|error| failure(format!("cannot write {}: {error}", header.display())))?;

    build.inputs.push(provider.clutch.join("clutch.toml"));
    let mut sources = vec![shim];
    for file in provider
        .manifest
        .headers
        .iter()
        .chain(&provider.manifest.sources)
    {
        build.inputs.push(provider.clutch.join(file));
    }
    sources.extend(
        provider
            .manifest
            .sources
            .iter()
            .map(|source| provider.clutch.join(source)),
    );
    let mut objects = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let object = work.join(format!("{index}.o"));
        run(
            Command::new(&toolchain.compiler)
                .args(SHIM_C_FLAGS)
                .args(&toolchain.flags)
                .arg("-I")
                .arg(&provider.clutch)
                .arg("-I")
                .arg(&work)
                .arg("-c")
                .arg(source)
                .arg("-o")
                .arg(&object),
            &format!("compiling {}", source.display()),
        )?;
        objects.push(object);
    }
    let library = format!("slug_typed_{}", provider.name);
    let archive = output.join(format!("lib{library}.a"));
    let _ = fs::remove_file(&archive);
    run(
        Command::new(&toolchain.archiver)
            .arg("crs")
            .arg(&archive)
            .args(&objects),
        &format!("archiving {}", archive.display()),
    )?;
    build.libraries.push(library);
    Ok(())
}

fn run(command: &mut Command, what: &str) -> Result<(), DescriptorError> {
    let output = command.output().map_err(|error| {
        DescriptorError::new(
            "typed-build",
            format!(
                "{what}: cannot start {}: {error}",
                command.get_program().display()
            ),
        )
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(DescriptorError::new(
        "typed-build",
        format!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ),
    ))
}

/// Generates the board package's registration for every typed provider.
fn registration(providers: &[TypedProvider]) -> String {
    let mut source = String::from("// Generated by slug-embedded-build. Do not edit.\n");
    if !providers.is_empty() {
        source.push_str("#[allow(unsafe_code)]\nunsafe extern \"C\" {\n");
        for provider in providers {
            let _ = writeln!(
                source,
                "    fn {}() -> *const slug_native_adapter::abi::RawTable;",
                table_function(&provider.name)
            );
        }
        source.push_str("}\n");
    }
    source.push_str(
        "\n/// Installs every typed provider the target descriptor selects.\n\
         #[allow(unsafe_code, unused_mut, clippy::let_unit_value)]\n\
         fn typed_provider_functions(\n    \
         owner: &alloc::rc::Rc<dyn core::any::Any>,\n\
         ) -> Result<alloc::vec::Vec<slug_vm::NativeFunction>, slug_native_adapter::InstallError> {\n    \
         let _ = owner;\n    \
         let mut functions = alloc::vec::Vec::new();\n",
    );
    for provider in providers {
        source.push_str("    {\n        let expected = [\n");
        for binding in provider.manifest.bindings() {
            write_expected(&mut source, binding);
        }
        let _ = write!(
            source,
            "        ];\n        \
             // SAFETY: the table is a static object linked into this image by the\n        \
             // typed provider build, generated from the same manifest.\n        \
             let installed = unsafe {{\n            \
             slug_native_adapter::install({}(), &expected, owner)?\n        \
             }};\n",
            table_function(&provider.name)
        );
        for (module, name) in &provider.non_null_text {
            let _ = writeln!(
                source,
                "        let _ = installed.text_results.reject_null({module:?}, {name:?});"
            );
        }
        source.push_str("        functions.extend(installed.functions);\n    }\n");
    }
    source.push_str("    Ok(functions)\n}\n");
    source
}

fn write_expected(source: &mut String, binding: &Binding) {
    let _ = writeln!(
        source,
        "            slug_native_adapter::ExpectedBinding {{ module: {:?}, name: {:?}, signature: {:?} }},",
        binding.module,
        binding.name,
        binding.canonical_signature()
    );
}
