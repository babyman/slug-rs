//! Calls generated typed native shims for the Slug VM.
//!
//! This crate is the only code that calls a shim entry. It verifies a shim
//! table against the bindings a host expects, registers each Slug-visible
//! binding as a native function, and encodes and decodes cells on each call.
//! It is `no_std + alloc`, so desktop loaders and static firmware builds share
//! it. Every `unsafe` operation lives in one private module.
//!
//! See `docs/decisions/2026-10-10-uniform-shim-entry.md` and
//! `docs/decisions/2026-10-11-typed-native-manifest-bindings.md`.

#![no_std]

extern crate alloc;

pub mod abi;
mod call;
mod raw;
mod signature;

use alloc::{
    borrow::ToOwned,
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    any::Any,
    cell::{Cell, OnceCell},
    fmt,
};

use slug_vm::{NativeArity, NativeFunction, NativeModule};

use abi::{MAX_CELLS, SHIM_ABI_VERSION};
use call::{Handle, Kind, ModuleState, RuntimeBinding};
use raw::TableBinding;
pub use raw::install;
use signature::{CellType, Direction};

/// A binding the host expects a shim to provide, with the canonical
/// signature the host derived from its manifest.
#[derive(Clone, Copy, Debug)]
pub struct ExpectedBinding<'a> {
    pub module: &'a str,
    /// The Slug function name, or `release:<resource>`.
    pub name: &'a str,
    pub signature: &'a str,
}

/// The result of [`install`]: the Slug-visible functions and the switches a
/// host sets after checking the bindings' `foreign` declarations.
pub struct Installed {
    pub functions: Vec<NativeFunction>,
    pub text_results: TextResults,
}

/// Per-binding switches for `cstr` results. By default a NULL text result
/// becomes `nil`; a binding whose declaration is annotated `str` without
/// `nil` must instead fail with `native.type`.
#[derive(Clone, Default)]
pub struct TextResults(Rc<Vec<(String, Rc<Cell<bool>>)>>);

impl TextResults {
    /// Makes a NULL text result of `module.name` fail with `native.type`.
    /// Returns whether the binding exists.
    #[must_use = "false means the binding does not exist"]
    pub fn reject_null(&self, module: &str, name: &str) -> bool {
        let qualified = format!("{module}.{name}");
        self.0
            .iter()
            .find(|(binding, _)| *binding == qualified)
            .map(|(_, reject)| reject.set(true))
            .is_some()
    }
}

impl fmt::Debug for TextResults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextResults")
            .finish_non_exhaustive()
    }
}

/// A shim table that does not match what the host expects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallError {
    message: String,
}

impl InstallError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl core::error::Error for InstallError {}

struct Verified {
    table: TableBinding,
    parsed: signature::Parsed,
}

pub(crate) fn install_bindings(
    abi_version: u32,
    table: Vec<TableBinding>,
    expected: &[ExpectedBinding<'_>],
    owner: &Rc<dyn Any>,
) -> Result<Installed, InstallError> {
    if abi_version != SHIM_ABI_VERSION {
        return Err(InstallError::new(format!(
            "shim ABI version {abi_version} is not the supported version {SHIM_ABI_VERSION}"
        )));
    }
    let mut modules = BTreeMap::<String, Vec<Verified>>::new();
    for binding in table {
        let qualified = format!("{}.{}", binding.module, binding.name);
        let wanted = expected
            .iter()
            .find(|wanted| wanted.module == binding.module && wanted.name == binding.name)
            .ok_or_else(|| {
                InstallError::new(format!("shim has unexpected binding `{qualified}`"))
            })?;
        if wanted.signature != binding.signature {
            return Err(InstallError::new(format!(
                "shim binding `{qualified}` is stale: expected `{}`, found `{}`",
                wanted.signature, binding.signature
            )));
        }
        let parsed = signature::parse(&binding.signature).map_err(|message| {
            InstallError::new(format!("shim binding `{qualified}`: {message}"))
        })?;
        let arguments = parsed.argument_cells();
        let results = parsed.result_cells();
        if parsed.module != binding.module
            || parsed.name != binding.name
            || usize::try_from(binding.argument_count) != Ok(arguments)
            || usize::try_from(binding.result_count) != Ok(results)
            || arguments > MAX_CELLS
            || results > MAX_CELLS
        {
            return Err(InstallError::new(format!(
                "shim binding `{qualified}` does not match its signature `{}`",
                binding.signature
            )));
        }
        let module = modules.entry(binding.module.clone()).or_default();
        if module.iter().any(|other| other.table.name == binding.name) {
            return Err(InstallError::new(format!(
                "shim has duplicate binding `{qualified}`"
            )));
        }
        module.push(Verified {
            table: binding,
            parsed,
        });
    }
    if let Some(missing) = expected.iter().find(|wanted| {
        !modules
            .get(wanted.module)
            .is_some_and(|module| module.iter().any(|found| found.table.name == wanted.name))
    }) {
        return Err(InstallError::new(format!(
            "shim is missing binding `{}.{}`",
            missing.module, missing.name
        )));
    }
    let mut functions = Vec::new();
    let mut text_results = Vec::new();
    for (module, bindings) in modules {
        let (module_functions, runtime) = install_module(&module, bindings, owner)?;
        functions.extend(module_functions);
        text_results.extend(
            runtime
                .iter()
                .map(|binding| (binding.qualified.clone(), binding.reject_null_text.clone())),
        );
    }
    Ok(Installed {
        functions,
        text_results: TextResults(Rc::new(text_results)),
    })
}

fn install_module(
    module_name: &str,
    bindings: Vec<Verified>,
    owner: &Rc<dyn Any>,
) -> Result<(Vec<NativeFunction>, Vec<Rc<RuntimeBinding>>), InstallError> {
    let (releases, functions): (Vec<_>, Vec<_>) = bindings
        .into_iter()
        .partition(|binding| binding.table.name.starts_with("release:"));

    let mut resources = Vec::new();
    for release in &releases {
        let resource = &release.table.name["release:".len()..];
        let parameters = &release.parsed.parameters;
        if parameters.len() != 1
            || parameters[0] != (Direction::In, CellType::Handle(resource.to_owned()))
        {
            return Err(InstallError::new(format!(
                "shim binding `{module_name}.{}` must take exactly one `handle<{resource}>`",
                release.table.name
            )));
        }
        resources.push((resource.to_owned(), release));
    }

    let resource_names = resources
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let runtime = functions
        .iter()
        .map(|function| runtime_binding(module_name, function, &resource_names).map(Rc::new))
        .collect::<Result<Vec<_>, _>>()?;

    let slots = Rc::new(OnceCell::new());
    let state = ModuleState {
        _owner: owner.clone(),
        bindings: runtime.clone(),
        resources: slots.clone(),
    };
    let descriptor = |error: slug_vm::NativeDescriptorError| InstallError::new(error.to_string());
    let module = NativeModule::new(module_name, state).map_err(descriptor)?;
    let resource_types = resources
        .iter()
        .map(|(name, release)| {
            Ok(call::ResourceSlot {
                resource_type: module
                    .resource_type(name.as_str(), Handle::close, Handle::destroy)
                    .map_err(descriptor)?,
                release: release.table.entry,
                release_results: release.parsed.result_cells(),
            })
        })
        .collect::<Result<Rc<[_]>, InstallError>>()?;
    let _ = slots.set(resource_types);
    let functions = functions
        .iter()
        .zip(&runtime)
        .enumerate()
        .map(|(index, (function, binding))| {
            module
                .function_with_member_key(
                    function.table.name.as_str(),
                    NativeArity::Exact(binding.arguments.len()),
                    index.to_string(),
                    call::call_binding,
                )
                .map_err(descriptor)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((functions, runtime))
}

fn runtime_binding(
    module_name: &str,
    function: &Verified,
    resources: &[&str],
) -> Result<RuntimeBinding, InstallError> {
    let qualified = format!("{module_name}.{}", function.table.name);
    let kind = |cell_type: &CellType| -> Result<Kind, InstallError> {
        Kind::new(cell_type, |resource| {
            resources
                .iter()
                .position(|name| *name == resource)
                .ok_or_else(|| {
                    InstallError::new(format!(
                        "shim binding `{qualified}` uses resource `{resource}`, which has no `release:{resource}` binding"
                    ))
                })
        })
    };
    let parsed = &function.parsed;
    let mut arguments = Vec::new();
    let mut outs = Vec::new();
    let mut close = None;
    for (direction, cell_type) in &parsed.parameters {
        match direction {
            Direction::Out => outs.push(kind(cell_type)?),
            Direction::In | Direction::Close => {
                if *direction == Direction::Close {
                    if close.is_some() {
                        return Err(InstallError::new(format!(
                            "shim binding `{qualified}` closes more than one handle"
                        )));
                    }
                    close = Some(arguments.len());
                }
                arguments.push(kind(cell_type)?);
            }
        }
    }
    let list = !outs.is_empty();
    let results = parsed
        .result
        .as_ref()
        .map(&kind)
        .transpose()?
        .into_iter()
        .chain(outs)
        .collect();
    Ok(RuntimeBinding {
        qualified,
        entry: function.table.entry,
        arguments,
        results,
        close,
        list,
        reject_null_text: Rc::new(Cell::new(false)),
    })
}
