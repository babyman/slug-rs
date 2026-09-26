//! Immutable bytecode ownership after VM installation-time validation.

use std::rc::Rc;

use crate::Program;

/// Immutable, checked bytecode installed by a VM.
///
/// A program is installed for one root entry. The VM validates that entry and
/// takes ownership of the mutable [`Program`] before this value is created.
/// Reusing an `InstalledProgram` therefore does not clone or revalidate its
/// bytecode. It is an in-process execution object, not a portable artifact.
#[derive(Clone, Debug)]
pub struct InstalledProgram {
    pub(super) program: Rc<Program>,
    pub(super) entry: usize,
}

impl InstalledProgram {
    /// Returns the immutable bytecode retained by this installed program.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Returns the root chunk validated for this installed program.
    #[must_use]
    pub const fn entry(&self) -> usize {
        self.entry
    }
}
