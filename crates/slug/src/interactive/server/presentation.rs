//! JSON-facing request decoding and value projection for interactive sessions.
//!
//! Session scheduling stays in the parent server. This module owns the
//! protocol representation so transport changes cannot alter VM lifecycle.

use serde::Deserialize;
use serde_json::{Value, json};
use slug_vm::Value as SlugValue;

use super::super::{Diagnostic, Request};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InitializeParams {
    pub(super) protocol: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubmitParams {
    pub(super) source: String,
}

pub(super) fn completed_result(value: &Value, pending: bool) -> Value {
    json!({
        "status": "completed",
        "value": value,
        "session_state": session_state(pending),
    })
}

pub(super) fn stalled_result(pending: bool) -> Value {
    json!({
        "status": "stalled",
        "session_state": session_state(pending),
    })
}

pub(super) fn session_result(pending: bool) -> Value {
    json!({ "session_state": session_state(pending) })
}

const fn session_state(pending: bool) -> &'static str {
    if pending { "stalled" } else { "idle" }
}

pub(super) fn protocol_value(value: &SlugValue) -> Value {
    match value {
        SlugValue::Nil => Value::Null,
        SlugValue::Bool(value) => Value::Bool(*value),
        SlugValue::Int(value) => json!(value),
        SlugValue::Float(value) if value.is_finite() => json!(value),
        SlugValue::Str(value) => json!(value.as_ref()),
        SlugValue::List(values) => Value::Array(values.iter().map(protocol_value).collect()),
        _ => json!({ "kind": value_kind(value), "display": value.to_string() }),
    }
}

fn value_kind(value: &SlugValue) -> &'static str {
    match value {
        SlugValue::Uninitialized | SlugValue::Binding { .. } => "binding",
        SlugValue::Nil => "nil",
        SlugValue::Bool(_) => "bool",
        SlugValue::Int(_) | SlugValue::Float(_) => "num",
        SlugValue::Str(_) => "str",
        SlugValue::Bytes(_) => "bytes",
        SlugValue::List(_) => "list",
        SlugValue::Map(_) => "map",
        SlugValue::StructSchema(_) => "struct_schema",
        SlugValue::Struct(_) => "struct",
        SlugValue::Enum(_) => "enum",
        SlugValue::Channel(_) => "chan",
        SlugValue::Closure(_)
        | SlugValue::Native(_)
        | SlugValue::DeclaredNative { .. }
        | SlugValue::Builtin(_) => "fn",
        #[cfg(feature = "concurrency")]
        SlugValue::Task(_) => "task",
        SlugValue::NativeResource(_) => "native_resource",
        SlugValue::Overloads(_) => "overloads",
    }
}

pub(super) fn required_params<T>(request: &Request, method: &str) -> Result<T, Box<Diagnostic>>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(params) = request.params.clone() else {
        return Err(Box::new(Diagnostic::protocol(
            "missing_params",
            format!("`{method}` requires params"),
        )));
    };
    serde_json::from_value(params).map_err(|error| {
        Box::new(Diagnostic::protocol(
            "invalid_params",
            format!("invalid `{method}` params: {error}"),
        ))
    })
}
