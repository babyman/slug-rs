//! Firmware-local flash image loading for the ESP32-S3 proof of concept.
//!
//! This is intentionally not a portable Slug bytecode format. The image is a
//! compact, static description compiled into firmware, and this loader turns it
//! into the VM's checked in-process `Program` representation at boot.

use core::fmt;

use slug_vm::{Chunk, Op, Program, Value};

#[derive(Clone, Copy)]
enum FlashConstant {
    Text(&'static str),
}

#[derive(Clone, Copy)]
enum FlashInstruction {
    GetGlobal(&'static str),
    Constant(u16),
    Call(u8),
    Return,
}

struct FlashProgram {
    name: &'static str,
    constants: &'static [FlashConstant],
    instructions: &'static [FlashInstruction],
}

#[derive(Clone, Copy, Debug)]
pub enum FlashLoadError {
    ConstantOutOfBounds { instruction: usize, constant: u16 },
}

impl fmt::Display for FlashLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConstantOutOfBounds {
                instruction,
                constant,
            } => write!(
                f,
                "flash instruction {instruction} references missing constant {constant}"
            ),
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/slug_frozen_program.rs"));

/// Loads the immutable proof program from its firmware-resident image.
///
/// The return value is the Program and its validated entry chunk index.
pub fn load_entry_program() -> Result<(Program, usize), FlashLoadError> {
    load(&FROZEN_PROGRAM)
}

fn load(image: &FlashProgram) -> Result<(Program, usize), FlashLoadError> {
    let mut chunk = Chunk::new(image.name, 0);
    for constant in image.constants {
        match constant {
            FlashConstant::Text(value) => {
                chunk.constant(Value::string(*value));
            }
        }
    }
    for (instruction_index, instruction) in image.instructions.iter().enumerate() {
        match instruction {
            FlashInstruction::GetGlobal(name) => {
                chunk.emit(Op::GetGlobal((*name).into()));
            }
            FlashInstruction::Constant(index) => {
                if usize::from(*index) >= image.constants.len() {
                    return Err(FlashLoadError::ConstantOutOfBounds {
                        instruction: instruction_index,
                        constant: *index,
                    });
                }
                chunk.emit(Op::Constant(usize::from(*index)));
            }
            FlashInstruction::Call(arguments) => {
                chunk.emit(Op::Call(usize::from(*arguments)));
            }
            FlashInstruction::Return => {
                chunk.emit(Op::Return);
            }
        };
    }
    let mut program = Program::new();
    let entry = program.add_chunk(chunk);
    Ok((program, entry))
}
