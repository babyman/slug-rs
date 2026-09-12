use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Diagnostic;

/// The initial interactive-session protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// One client request decoded from a NDJSON line.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// One terminal response to a protocol request.
#[derive(Clone, Debug, Serialize)]
pub struct Response {
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Diagnostic>,
}

impl Response {
    pub(crate) fn success(id: u64, session: Option<String>, result: Value) -> Self {
        Self {
            id: Some(id),
            session,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub(crate) fn failure(id: Option<u64>, session: Option<String>, error: Diagnostic) -> Self {
        Self {
            id,
            session,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

/// An unsolicited session-associated protocol event.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub session: String,
    pub event: String,
    pub data: Value,
}
