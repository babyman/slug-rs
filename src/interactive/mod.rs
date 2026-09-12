//! Interactive session protocol support for embedded Slug hosts.
//!
//! The server owns protocol lifecycle, persistent session state, and output
//! events without exposing raw program bytes on the NDJSON transport.

mod diagnostics;
mod protocol;
mod server;

pub use diagnostics::{Diagnostic, DiagnosticCategory, DiagnosticFrame, DiagnosticLocation};
pub use protocol::{Event, PROTOCOL_VERSION, Request, Response};
pub use server::{OutputError, OutputStream, Server};
