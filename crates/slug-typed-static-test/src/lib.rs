//! Two generated typed shims statically linked into one image, installed
//! through the shared adapter as a firmware board package does.

extern crate alloc;

use std::{any::Any, rc::Rc};

use slug_vm::{
    EmptyVmConfiguration,
    cslug::{self, ArtifactHost, StaticProvider},
};

include!(concat!(env!("OUT_DIR"), "/typed_providers.rs"));

const PROGRAM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/program.cslug"));

/// Runs the fixture program and returns its result as text.
///
/// # Errors
///
/// Returns the first artifact, installation, or runtime failure as text.
pub fn run() -> Result<String, String> {
    let limits = cslug::Limits {
        artifact_bytes: 65_536,
        section_bytes: 65_536,
        modules: 8,
        imports: 8,
        string_bytes: 4_096,
        chunks_per_module: 64,
        entries_per_table: 1_024,
    };
    let artifact = cslug::decode(PROGRAM, limits).map_err(|error| error.to_string())?;
    let host = ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
        .map_err(|error| error.to_string())?;
    let owner: Rc<dyn Any> = Rc::new(());
    let functions = typed_provider_functions(&owner).map_err(|error| error.to_string())?;
    host.install_static_providers([StaticProvider::new(functions)])
        .map_err(|error| format!("{error:?}"))?;
    host.run()
        .map(|value| value.to_string())
        .map_err(|error| error.to_string())
}
