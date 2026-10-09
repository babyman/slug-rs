//! Firmware-local private frozen program loading for the ESP32-S3 proof.
//!
//! The generated tables are compiled into this exact firmware image. They are
//! not a portable Slug format and are validated before VM installation.

use slug_vm::{
    Program,
    frozen::{FrozenChunk, FrozenConstant, FrozenInstruction, FrozenLoadError, FrozenProgram},
};

include!(concat!(env!("OUT_DIR"), "/slug_blink_program.rs"));

/// Loads the immutable blink program from its firmware-resident image.
pub fn load_blink_program() -> Result<(Program, usize), FrozenLoadError> {
    slug_vm::frozen::load(&FROZEN_PROGRAM)
}
