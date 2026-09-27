//! Native bindings installed into each interactive server VM.

use std::{cell::RefCell, rc::Rc};

use slug_vm::{
    NativeCall, NativeError, NativeOwnedValue, NativeStatus, NativeValueKind, NativeValueRef,
};

use super::{OutputSink, OutputStream};

pub(super) type NativeCallback = for<'call> fn(&mut NativeCall<'call>) -> NativeStatus;

pub(super) fn native_print(call: &mut NativeCall<'_>) -> NativeStatus {
    native_write(call, false)
}

pub(super) fn native_println(call: &mut NativeCall<'_>) -> NativeStatus {
    native_write(call, true)
}

pub(super) fn native_len(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call.argument(0) {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    let length = match value.kind() {
        NativeValueKind::String => match value.as_str() {
            Ok(value) => value.chars().count(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::Bytes => match value.as_bytes() {
            Ok(value) => value.len(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::List | NativeValueKind::Map => {
            value.len().expect("collection kind has a length")
        }
        kind => {
            return call.raise(NativeError::new(
                "native.type",
                format!("`len` expects str, bytes, list, or map, got {kind:?}"),
            ));
        }
    };
    let Ok(length) = i64::try_from(length) else {
        return call.raise(NativeError::new(
            "native.range",
            "`len` result exceeds the supported integer range",
        ));
    };
    call.return_value(NativeOwnedValue::integer(length))
}

pub(super) fn native_channel(call: &mut NativeCall<'_>) -> NativeStatus {
    let capacity = match call.argument_count() {
        0 => 0,
        1 => match call.argument(0).and_then(NativeValueRef::as_i64) {
            Ok(value) => match usize::try_from(value) {
                Ok(value) => value,
                Err(_) => {
                    return call.raise(NativeError::new(
                        "native.type",
                        "channel capacity must not be negative or too large",
                    ));
                }
            },
            Err(error) => return call.raise(error),
        },
        count => {
            return call.raise(NativeError::new(
                "native.arity",
                format!("`chan` expects at most 1 argument, got {count}"),
            ));
        }
    };
    let channel = call.plain_channel(capacity);
    call.return_value(channel)
}

pub(super) fn native_close(call: &mut NativeCall<'_>) -> NativeStatus {
    if let Err(error) = call.close_channel(0) {
        return call.raise(error);
    }
    call.return_value(NativeOwnedValue::nil())
}

fn native_write(call: &mut NativeCall<'_>, newline: bool) -> NativeStatus {
    let output = (0..call.argument_count())
        .map(|index| {
            call.argument(index)
                .expect("index comes from the argument count")
                .to_display_string()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let output = if newline {
        format!("{output}\n")
    } else {
        output
    };
    let Some(sink) = call.state::<Rc<RefCell<OutputSink>>>() else {
        return call.raise(NativeError::new(
            "native.output",
            "interactive output sink is unavailable",
        ));
    };
    if !sink.borrow_mut().write_active(OutputStream::Stdout, output) {
        return call.raise(NativeError::new(
            "native.output",
            "interactive output was produced outside a submission",
        ));
    }
    call.return_value(NativeOwnedValue::nil())
}
