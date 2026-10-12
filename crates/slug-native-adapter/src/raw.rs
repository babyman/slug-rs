//! Every `unsafe` operation in the adapter: reading a shim table, calling an
//! entry, running its cleanup, and copying text an entry returned.

#![allow(unsafe_code)]

use alloc::{boxed::Box, format, rc::Rc, string::String, vec::Vec};

use core::{
    any::Any,
    ffi::{CStr, c_char},
    ptr,
};
use slug_vm::{NativeChannelProducer, NativeProducerStatus, NativeSendValue};

use crate::{
    ExpectedBinding, InstallError, Installed,
    abi::{Cell, Cleanup, Entry, RawTable},
};

/// Verifies a shim table against `expected` and returns one native function
/// per Slug-visible binding, grouped into one native module per Slug module,
/// with the switches for their text results.
///
/// The table must contain exactly the expected bindings, each with the same
/// canonical signature and cell counts. `owner` is kept alive as long as any
/// returned function or resource, so a dynamic library can be held there.
///
/// # Safety
///
/// `table` must be NULL or point to a shim table with the layout of
/// [`RawTable`], valid for reads for as long as `owner` lives, whose entries
/// follow the uniform entry contract for the signatures the table lists.
///
/// # Errors
///
/// Returns a checked error naming the binding for a NULL or malformed table,
/// an unsupported ABI version, a missing, unexpected, or duplicate binding, a
/// signature or count mismatch, or a handle without a release binding.
pub unsafe fn install(
    table: *const RawTable,
    expected: &[ExpectedBinding<'_>],
    owner: &Rc<dyn Any>,
) -> Result<Installed, InstallError> {
    // SAFETY: forwarded from this function's contract.
    let (abi_version, bindings) = unsafe { read_table(table)? };
    crate::install_bindings(abi_version, bindings, expected, owner)
}

/// One binding copied out of a shim table.
pub(crate) struct TableBinding {
    pub module: String,
    pub name: String,
    pub signature: String,
    pub entry: Entry,
    pub argument_count: u32,
    pub result_count: u32,
}

/// Copies a shim table into owned bindings.
///
/// # Safety
///
/// `table` must be NULL or point to a table with the layout of
/// [`RawTable`] whose bindings and strings are valid for reads, and every
/// entry it names must follow the uniform entry contract for its signature.
pub(crate) unsafe fn read_table(
    table: *const RawTable,
) -> Result<(u32, Vec<TableBinding>), InstallError> {
    // SAFETY: the caller guarantees `table` is NULL or a valid table.
    let Some(table) = (unsafe { table.as_ref() }) else {
        return Err(InstallError::new("shim table function returned NULL"));
    };
    if table.binding_count == 0 {
        return Ok((table.abi_version, Vec::new()));
    }
    if table.bindings.is_null() {
        return Err(InstallError::new(
            "shim table has bindings but a NULL array",
        ));
    }
    let count = usize::try_from(table.binding_count)
        .map_err(|_| InstallError::new("shim table binding count is too large"))?;
    // SAFETY: the caller guarantees `bindings` holds `binding_count` entries.
    let raw = unsafe { core::slice::from_raw_parts(table.bindings, count) };
    let mut bindings = Vec::with_capacity(count);
    for (index, binding) in raw.iter().enumerate() {
        // SAFETY: the caller guarantees each string is NULL or NUL-terminated.
        let text = |pointer: *const c_char, field: &str| unsafe {
            text(pointer).ok_or_else(|| {
                InstallError::new(format!("shim binding {index} has an invalid {field}"))
            })
        };
        let entry = binding
            .entry
            .ok_or_else(|| InstallError::new(format!("shim binding {index} has a NULL entry")))?;
        bindings.push(TableBinding {
            module: text(binding.module, "module")?,
            name: text(binding.name, "name")?,
            signature: text(binding.signature, "signature")?,
            entry,
            argument_count: binding.argument_count,
            result_count: binding.result_count,
        });
    }
    Ok((table.abi_version, bindings))
}

/// # Safety
///
/// `pointer` must be NULL or point to a NUL-terminated string.
unsafe fn text(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    let text = unsafe { CStr::from_ptr(pointer) };
    text.to_str().ok().map(String::from)
}

/// Runs a shim's cleanup when dropped, so it runs even if decoding unwinds.
struct ReleaseGuard(Cleanup);

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        if let Some(release) = self.0.release.take() {
            // SAFETY: an entry sets release only to its own function and a
            // context that stays valid until release runs; the results array
            // it may point at is still borrowed by `invoke`.
            unsafe { release(self.0.context) };
        }
    }
}

/// Calls `entry`, then `decode` with its status and results, then the
/// entry's cleanup. The results stay alive and unmodified until cleanup runs.
pub(crate) fn invoke<R>(
    entry: Entry,
    arguments: &[Cell],
    results: &mut [Cell],
    decode: impl FnOnce(i32, &[Cell]) -> R,
) -> R {
    let mut guard = ReleaseGuard(Cleanup {
        release: None,
        context: ptr::null_mut(),
    });
    // SAFETY: install verified that `entry` came from a table whose binding
    // has exactly these argument and result counts; the arrays outlive the
    // call, and pointer cells point at memory that outlives it.
    let status = unsafe { entry(arguments.as_ptr(), results.as_mut_ptr(), &raw mut guard.0) };
    let decoded = decode(status, results);
    drop(guard);
    decoded
}

/// Copies the data a text or bytes result cell points at.
///
/// Only call this on a non-NULL `cstr`, `str`, or `bytes` cell an entry has
/// just written, before its cleanup runs. The shim contract makes `length` the
/// text's `strlen` for `cstr` and the length the callee wrote otherwise.
pub(crate) fn copy_bytes(cell: Cell) -> Option<Vec<u8>> {
    let length = usize::try_from(cell.length).ok()?;
    let pointer = ptr::with_exposed_provenance::<u8>(cell.address());
    // SAFETY: per the shim contract, a non-NULL text or bytes result points
    // at `length` readable bytes that live at least until cleanup runs.
    Some(unsafe { core::slice::from_raw_parts(pointer, length) }.to_vec())
}

/// The host function table a `slug_producer *` carries: C's
/// `slug_producer_api`, version 1.
#[repr(C)]
struct ProducerApi {
    version: u32,
    send_nil: unsafe extern "C" fn(*mut ProducerHandle) -> i32,
    send_bool: unsafe extern "C" fn(*mut ProducerHandle, bool) -> i32,
    send_i64: unsafe extern "C" fn(*mut ProducerHandle, i64) -> i32,
    send_f64: unsafe extern "C" fn(*mut ProducerHandle, f64) -> i32,
    send_text: unsafe extern "C" fn(*mut ProducerHandle, *const c_char, usize) -> i32,
    send_bytes: unsafe extern "C" fn(*mut ProducerHandle, *const u8, usize) -> i32,
    is_closed: unsafe extern "C" fn(*mut ProducerHandle) -> bool,
    close: unsafe extern "C" fn(*mut ProducerHandle),
    release: unsafe extern "C" fn(*mut ProducerHandle),
}

/// C's `slug_producer`, extended with the VM producer it wraps. The table
/// pointer must stay the first field.
#[repr(C)]
struct ProducerHandle {
    api: *const ProducerApi,
    producer: NativeChannelProducer,
}

static PRODUCER_API: ProducerApi = ProducerApi {
    version: 1,
    send_nil: producer_send_nil,
    send_bool: producer_send_bool,
    send_i64: producer_send_i64,
    send_f64: producer_send_f64,
    send_text: producer_send_text,
    send_bytes: producer_send_bytes,
    is_closed: producer_is_closed,
    close: producer_close,
    release: producer_release,
};

const SENT: i32 = 0;
const FULL: i32 = 1;
const CLOSED: i32 = 2;
const INVALID: i32 = 3;

/// Boxes `producer` as a C `slug_producer` and returns its address. C owns it
/// until it calls release, or [`release_producer`] frees it if the call never
/// starts.
pub(crate) fn mint_producer(producer: NativeChannelProducer) -> usize {
    Box::into_raw(Box::new(ProducerHandle {
        api: &raw const PRODUCER_API,
        producer,
    }))
    .expose_provenance()
}

/// Frees a producer minted for a call that failed before reaching C.
pub(crate) fn release_producer(address: usize) {
    // SAFETY: `address` came from `mint_producer` and was never handed to C.
    unsafe { producer_release(ptr::with_exposed_provenance_mut(address)) };
}

/// # Safety
///
/// `handle` must be NULL or a live handle from `mint_producer`.
unsafe fn send(handle: *mut ProducerHandle, value: NativeSendValue) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return CLOSED;
    };
    match handle.producer.try_send(value) {
        NativeProducerStatus::Sent => SENT,
        NativeProducerStatus::Full(_) => FULL,
        NativeProducerStatus::Closed(_) => CLOSED,
    }
}

unsafe extern "C" fn producer_send_nil(handle: *mut ProducerHandle) -> i32 {
    // SAFETY: C passes the handle it was given, per the producer contract.
    unsafe { send(handle, NativeSendValue::nil()) }
}

unsafe extern "C" fn producer_send_bool(handle: *mut ProducerHandle, value: bool) -> i32 {
    // SAFETY: as above.
    unsafe { send(handle, NativeSendValue::boolean(value)) }
}

unsafe extern "C" fn producer_send_i64(handle: *mut ProducerHandle, value: i64) -> i32 {
    // SAFETY: as above.
    unsafe { send(handle, NativeSendValue::integer(value)) }
}

unsafe extern "C" fn producer_send_f64(handle: *mut ProducerHandle, value: f64) -> i32 {
    if !value.is_finite() {
        return INVALID;
    }
    // SAFETY: as above.
    unsafe { send(handle, NativeSendValue::float(value)) }
}

/// # Safety
///
/// `data` must be NULL with `length` 0, or point to `length` readable bytes.
unsafe fn copy_buffer(data: *const u8, length: usize) -> Option<Vec<u8>> {
    if length == 0 {
        return Some(Vec::new());
    }
    if data.is_null() {
        return None;
    }
    // SAFETY: forwarded from this function's contract.
    Some(unsafe { core::slice::from_raw_parts(data, length) }.to_vec())
}

unsafe extern "C" fn producer_send_text(
    handle: *mut ProducerHandle,
    text: *const c_char,
    length: usize,
) -> i32 {
    // SAFETY: C passes `length` readable bytes, per the producer contract.
    let Some(bytes) = (unsafe { copy_buffer(text.cast(), length) }) else {
        return INVALID;
    };
    match String::from_utf8(bytes) {
        // SAFETY: C passes the handle it was given.
        Ok(text) => unsafe { send(handle, NativeSendValue::string(text)) },
        Err(_) => INVALID,
    }
}

unsafe extern "C" fn producer_send_bytes(
    handle: *mut ProducerHandle,
    data: *const u8,
    length: usize,
) -> i32 {
    // SAFETY: C passes `length` readable bytes, per the producer contract.
    let Some(bytes) = (unsafe { copy_buffer(data, length) }) else {
        return INVALID;
    };
    // SAFETY: C passes the handle it was given.
    unsafe { send(handle, NativeSendValue::bytes(bytes)) }
}

unsafe extern "C" fn producer_is_closed(handle: *mut ProducerHandle) -> bool {
    // SAFETY: C passes the handle it was given.
    unsafe { handle.as_ref() }.is_none_or(|handle| handle.producer.is_closed())
}

unsafe extern "C" fn producer_close(handle: *mut ProducerHandle) {
    // SAFETY: C passes the handle it was given.
    if let Some(handle) = unsafe { handle.as_ref() } {
        handle.producer.close();
    }
}

unsafe extern "C" fn producer_release(handle: *mut ProducerHandle) {
    if !handle.is_null() {
        // SAFETY: release is called once, on a handle from `mint_producer`.
        drop(unsafe { Box::from_raw(handle) });
    }
}
