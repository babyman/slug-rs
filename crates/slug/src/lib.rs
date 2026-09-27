//! Executable-host composition for Slug.
//!
//! CLI parsing, environment and configuration policy, entry lookup, builtin
//! registration, and VM/frontend assembly belong here.

mod host;

pub use host::{DesktopLoader, build_default_host_vm, default_library_root};
