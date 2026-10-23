//! JSON-facing request decoding and result shapes for interactive sessions.
//!
//! Session scheduling stays in the parent server. This module owns the
//! protocol representation so transport changes cannot alter VM lifecycle.

use serde::Deserialize;
use serde_json::{Value, json};

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
