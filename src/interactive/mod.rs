//! Interactive session protocol support for embedded Slug hosts.
//!
//! The initial server shell owns protocol lifecycle only. Source submission and
//! persistent runtime state are deliberately deferred to later milestones.

mod diagnostics;
mod protocol;
mod server;

pub use diagnostics::{Diagnostic, DiagnosticCategory, DiagnosticFrame, DiagnosticLocation};
pub use protocol::{Event, PROTOCOL_VERSION, Request, Response};
pub use server::Server;
