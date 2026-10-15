//! Interactive session protocol support for embedded Slug hosts.
//!
//! The server owns protocol lifecycle, persistent session state, and output
//! events without exposing raw program bytes on the NDJSON transport.

mod protocol;
mod server;

pub use crate::diagnostic::{
    Diagnostic, DiagnosticCategory, DiagnosticEdit, DiagnosticFix, DiagnosticFrame,
    DiagnosticLocation, DiagnosticPosition, DiagnosticRange, NativeDiagnostic, value_json,
};
pub use protocol::{Event, EventOrigin, IncomingMessage, PROTOCOL_VERSION, Request, Response};
pub use server::{OutputError, OutputStream, Server};
