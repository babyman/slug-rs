//! One typed call: encode arguments into cells, call the entry, decode the
//! results, and run cleanup.

use alloc::{format, rc::Rc, string::String, vec::Vec};
use core::{any::Any, cell::OnceCell, ffi::c_void};

use slug_vm::{
    NativeCall, NativeError, NativeOwnedValue, NativeResourceType, NativeStatus, NativeValueKind,
};

use crate::{
    InstallError,
    abi::{Cell, Entry, MAX_CELLS},
    raw,
    signature::CellType,
};

/// How one argument or result cell is encoded.
#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Bool,
    Signed {
        min: i64,
        max: i64,
        name: &'static str,
    },
    Unsigned {
        max: u64,
        name: &'static str,
    },
    F32,
    F64,
    Str,
    Bytes,
    Cstr,
    /// Index into the module's resource slots.
    Handle(usize),
}

impl Kind {
    pub(crate) fn new(
        cell_type: &CellType,
        resource: impl FnOnce(&str) -> Result<usize, InstallError>,
    ) -> Result<Self, InstallError> {
        Ok(match cell_type {
            CellType::Bool => Self::Bool,
            CellType::Signed { min, max, name } => Self::Signed {
                min: *min,
                max: *max,
                name,
            },
            CellType::Unsigned { max, name } => Self::Unsigned { max: *max, name },
            CellType::F32 => Self::F32,
            CellType::F64 => Self::F64,
            CellType::Str => Self::Str,
            CellType::Bytes => Self::Bytes,
            CellType::Cstr => Self::Cstr,
            CellType::Handle(name) => Self::Handle(resource(name)?),
        })
    }
}

pub(crate) struct RuntimeBinding {
    /// `module.function`, for diagnostics.
    pub qualified: String,
    pub entry: Entry,
    pub arguments: Vec<Kind>,
    /// The C result, if any, then each `out` value.
    pub results: Vec<Kind>,
    /// Argument index of the handle an explicit close consumes.
    pub close: Option<usize>,
    /// Results return to Slug as a list because the binding has `out` values.
    pub list: bool,
    /// A NULL `cstr` return fails with `native.type` instead of becoming nil.
    pub reject_null_text: Rc<core::cell::Cell<bool>>,
}

pub(crate) struct ResourceSlot {
    pub resource_type: NativeResourceType<Handle>,
    pub release: Entry,
    pub release_results: usize,
}

pub(crate) struct ModuleState {
    /// Keeps the shim's library loaded while any function or resource lives.
    pub _owner: Rc<dyn Any>,
    pub bindings: Vec<Rc<RuntimeBinding>>,
    pub resources: Rc<OnceCell<Rc<[ResourceSlot]>>>,
}

/// A native handle owned by a Slug resource. `address` is `None` once an
/// explicit close has consumed it, so release never runs twice.
pub(crate) struct Handle {
    address: Option<usize>,
    release: Entry,
    release_results: usize,
}

impl Handle {
    /// The VM's close callback: release a still-open handle at most once.
    pub(crate) fn close(&mut self) {
        if let Some(address) = self.address.take() {
            let arguments = [Cell::pointer(address, 0)];
            let mut results = [Cell::default(); MAX_CELLS];
            // The release result and status are ignored by design.
            raw::invoke(
                self.release,
                &arguments,
                &mut results[..self.release_results],
                |_, _| (),
            );
        }
    }

    pub(crate) fn destroy(mut self) {
        self.close();
    }
}

enum Failure {
    Boundary(NativeError),
    Contract(String),
}

impl From<NativeError> for Failure {
    fn from(error: NativeError) -> Self {
        Self::Boundary(error)
    }
}

/// The callback registered for every Slug-visible typed binding.
pub(crate) fn call_binding(call: &mut NativeCall<'_>) -> NativeStatus {
    let Some((binding, resources)) = binding(call) else {
        return call.report_contract_violation("typed native binding has no adapter state");
    };
    match run(call, &binding, &resources) {
        Ok(value) => call.return_value(value),
        Err(Failure::Boundary(error)) => call.raise(error),
        Err(Failure::Contract(message)) => call.report_contract_violation(message),
    }
}

fn binding(call: &NativeCall<'_>) -> Option<(Rc<RuntimeBinding>, Rc<[ResourceSlot]>)> {
    let state = call.state::<ModuleState>()?;
    let index = call.member_key()?.parse::<usize>().ok()?;
    Some((
        state.bindings.get(index)?.clone(),
        state.resources.get()?.clone(),
    ))
}

fn run(
    call: &mut NativeCall<'_>,
    binding: &RuntimeBinding,
    resources: &[ResourceSlot],
) -> Result<NativeOwnedValue, Failure> {
    let close = match binding.close {
        Some(index) => {
            let Kind::Handle(slot) = binding.arguments[index] else {
                return Err(Failure::Contract(format!(
                    "`{}` closes a parameter that is not a handle",
                    binding.qualified
                )));
            };
            let resource_type = &resources[slot].resource_type;
            match call.with_resource(index, resource_type, |handle| handle.address) {
                Ok(Some(_)) => Some((index, resource_type)),
                // Explicit close is idempotent: no native call, every result nil.
                Ok(None) => return Ok(nil_results(binding)),
                Err(error) if error.code() == "native.resource_closed" => {
                    return Ok(nil_results(binding));
                }
                Err(error) => return Err(error.into()),
            }
        }
        None => None,
    };

    let mut arguments = [Cell::default(); MAX_CELLS];
    let mut texts = Vec::new();
    for (index, kind) in binding.arguments.iter().enumerate() {
        arguments[index] = encode(call, index, kind, resources, &mut texts)?;
    }
    if let Some((index, resource_type)) = close {
        // Take the handle so the VM's close callback does not release it.
        call.with_resource(index, resource_type, |handle| handle.address.take())?;
    }
    let mut results = [Cell::default(); MAX_CELLS];
    let decoded = raw::invoke(
        binding.entry,
        &arguments[..binding.arguments.len()],
        &mut results[..binding.results.len()],
        |status, results| {
            (status == 0)
                .then(|| {
                    results
                        .iter()
                        .zip(&binding.results)
                        .map(|(cell, kind)| decode(*cell, kind))
                        .collect::<Vec<_>>()
                })
                .ok_or(status)
        },
    );
    drop(texts);
    if let Some((index, resource_type)) = close {
        call.close_resource(index, resource_type)?;
    }
    let decoded = decoded.map_err(|status| {
        Failure::Contract(format!(
            "typed native shim for `{}` failed with status {status}",
            binding.qualified
        ))
    })?;

    if binding.reject_null_text.get()
        && !binding.list
        && matches!(binding.results.first(), Some(Kind::Cstr))
        && matches!(decoded.first(), Some(Decoded::Value(value)) if value.as_ref().kind() == NativeValueKind::Nil)
    {
        return Err(NativeError::new(
            "native.type",
            format!("`{}` returned NULL for a `str` result", binding.qualified),
        )
        .into());
    }

    // Wrap every handle first, so an error below still releases it.
    let mut values = Vec::with_capacity(decoded.len());
    let mut failure = None;
    for value in decoded {
        values.push(match value {
            Decoded::Value(value) => value,
            Decoded::Handle(address, slot) => {
                let slot = &resources[slot];
                call.resource(
                    &slot.resource_type,
                    Handle {
                        address: Some(address),
                        release: slot.release,
                        release_results: slot.release_results,
                    },
                )?
            }
            Decoded::Error(error) => {
                failure.get_or_insert(error);
                NativeOwnedValue::nil()
            }
        });
    }
    if let Some(error) = failure {
        return Err(error.into());
    }
    Ok(if binding.list {
        NativeOwnedValue::list(values)
    } else {
        values.pop().unwrap_or_else(NativeOwnedValue::nil)
    })
}

fn nil_results(binding: &RuntimeBinding) -> NativeOwnedValue {
    if binding.list {
        NativeOwnedValue::list(
            binding
                .results
                .iter()
                .map(|_| NativeOwnedValue::nil())
                .collect(),
        )
    } else {
        NativeOwnedValue::nil()
    }
}

fn encode(
    call: &NativeCall<'_>,
    index: usize,
    kind: &Kind,
    resources: &[ResourceSlot],
    texts: &mut Vec<Vec<u8>>,
) -> Result<Cell, NativeError> {
    let argument = call.argument(index)?;
    Ok(match kind {
        Kind::Bool => Cell::unsigned(u64::from(argument.as_bool()?)),
        Kind::Signed { min, max, name } => {
            let value = argument.as_i64()?;
            if value < *min || value > *max {
                return Err(out_of_range(index, name, value));
            }
            Cell::signed(value)
        }
        Kind::Unsigned { max, name } => {
            let value = argument.as_i64()?;
            match u64::try_from(value) {
                Ok(unsigned) if unsigned <= *max => Cell::unsigned(unsigned),
                _ => return Err(out_of_range(index, name, value)),
            }
        }
        Kind::F32 => {
            let value = number(call, index)?;
            if value.is_finite() && value.abs() > f64::from(f32::MAX) {
                return Err(out_of_range(index, "f32", value));
            }
            Cell::float(value)
        }
        Kind::F64 => Cell::float(number(call, index)?),
        Kind::Str => {
            let text = argument.as_str()?;
            Cell::pointer(text.as_ptr().expose_provenance(), text.len() as u64)
        }
        Kind::Bytes => {
            let bytes = argument.as_bytes()?;
            Cell::pointer(bytes.as_ptr().expose_provenance(), bytes.len() as u64)
        }
        Kind::Cstr => {
            let text = argument.as_str()?;
            if text.as_bytes().contains(&0) {
                return Err(NativeError::new(
                    "native.interior_nul",
                    format!("argument {index} contains a NUL byte"),
                ));
            }
            let mut copy = Vec::with_capacity(text.len() + 1);
            copy.extend_from_slice(text.as_bytes());
            copy.push(0);
            // The copy's heap buffer does not move when `texts` grows.
            let address = copy.as_ptr().expose_provenance();
            texts.push(copy);
            Cell::pointer(address, 0)
        }
        Kind::Handle(slot) => {
            let address = call
                .with_resource(index, &resources[*slot].resource_type, |handle| {
                    handle.address
                })?
                .ok_or_else(|| {
                    NativeError::new("native.resource_closed", "native resource is closed")
                })?;
            Cell::pointer(address, 0)
        }
    })
}

/// A Slug `num`: an integer or a float.
fn number(call: &NativeCall<'_>, index: usize) -> Result<f64, NativeError> {
    let argument = call.argument(index)?;
    match argument.kind() {
        #[allow(clippy::cast_precision_loss)] // C receives the nearest float
        NativeValueKind::Int => Ok(argument.as_i64()? as f64),
        _ => argument.as_f64(),
    }
}

fn out_of_range(index: usize, name: &str, value: impl core::fmt::Display) -> NativeError {
    NativeError::new(
        "native.range",
        format!("argument {index} value {value} does not fit `{name}`"),
    )
}

enum Decoded {
    Value(NativeOwnedValue),
    Handle(usize, usize),
    Error(NativeError),
}

fn decode(cell: Cell, kind: &Kind) -> Decoded {
    if matches!(kind, Kind::Cstr | Kind::Handle(_)) && cell.address() == 0 {
        return Decoded::Value(NativeOwnedValue::nil());
    }
    let value = match kind {
        Kind::Bool => NativeOwnedValue::boolean(cell.word != 0),
        Kind::Signed { .. } => NativeOwnedValue::integer(cell.as_signed()),
        Kind::Unsigned { name, .. } => match i64::try_from(cell.word) {
            Ok(value) => NativeOwnedValue::integer(value),
            Err(_) => {
                return Decoded::Error(NativeError::new(
                    "native.range",
                    format!(
                        "`{name}` result {} exceeds the Slug integer range",
                        cell.word
                    ),
                ));
            }
        },
        Kind::F32 | Kind::F64 => NativeOwnedValue::float(cell.as_float()),
        Kind::Cstr => match raw::copy_text(cell).map(String::from_utf8) {
            Some(Ok(text)) => NativeOwnedValue::string(text),
            Some(Err(_)) => {
                return Decoded::Error(NativeError::new(
                    "native.encoding",
                    "text result is not valid UTF-8",
                ));
            }
            None => {
                return Decoded::Error(NativeError::new(
                    "native.range",
                    "text result is too long for this platform",
                ));
            }
        },
        Kind::Handle(slot) => return Decoded::Handle(cell.address(), *slot),
        // Rejected for results when the signature is parsed.
        Kind::Str | Kind::Bytes => NativeOwnedValue::nil(),
    };
    Decoded::Value(value)
}

#[allow(dead_code)]
const fn assert_cell_layout() {
    assert!(core::mem::size_of::<Cell>() == 16);
    assert!(core::mem::size_of::<*const c_void>() <= 8);
}
const _: () = assert_cell_layout();
