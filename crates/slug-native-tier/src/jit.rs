//! The Cranelift JIT module that owns generated code.
//!
//! Code is never freed: a compiled chunk lives as long as its program may,
//! and programs are shared with forks on other threads.

#![allow(unsafe_code)]

use std::sync::Mutex;

use cranelift_codegen::{
    Context,
    isa::OwnedTargetIsa,
    settings::{self, Configurable},
};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};

/// The module, behind a lock: any thread may compile a chunk that became
/// hot on it.
pub(crate) struct Jit {
    module: Mutex<Owned>,
    isa: OwnedTargetIsa,
}

struct Owned(JITModule);

// SAFETY: a `JITModule` holds raw pointers to the memory it maps. It is only
// used behind `Jit`'s mutex, so one thread at a time, and the code it
// finalizes is immutable and safe to run from any thread.
unsafe impl Send for Owned {}

impl Jit {
    /// A module for this machine, or why there is none.
    pub(crate) fn new() -> Result<Self, String> {
        let isa = target_isa()?;
        let builder = JITBuilder::with_isa(isa.clone(), default_libcall_names());
        Ok(Self {
            module: Mutex::new(Owned(JITModule::new(builder))),
            isa,
        })
    }

    pub(crate) fn isa(&self) -> &OwnedTargetIsa {
        &self.isa
    }

    /// Compiles the function in `context` and returns its code's address,
    /// which stays mapped for the life of the process.
    pub(crate) fn define(&self, name: &str, context: &mut Context) -> Result<*const u8, String> {
        let mut module = self
            .module
            .lock()
            .map_err(|_| "the JIT module is poisoned".to_string())?;
        let module = &mut module.0;
        let id = module
            .declare_function(name, Linkage::Local, &context.func.signature)
            .map_err(|error| error.to_string())?;
        module
            .define_function(id, context)
            .map_err(|error| format!("{error:?}"))?;
        module
            .finalize_definitions()
            .map_err(|error| error.to_string())?;
        Ok(module.get_finalized_function(id))
    }
}

fn target_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    for (name, value) in [
        ("opt_level", "speed"),
        // Generated code calls runtime functions through their addresses.
        ("use_colocated_libcalls", "false"),
        ("is_pic", "false"),
    ] {
        flags.set(name, value).map_err(|error| error.to_string())?;
    }
    cranelift_native::builder()
        .map_err(ToString::to_string)?
        .finish(settings::Flags::new(flags))
        .map_err(|error| error.to_string())
}
