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
    Number(u32),
}

#[derive(Clone, Copy)]
enum FlashInstruction {
    GetGlobal(&'static str),
    Constant(u16),
    True,
    False,
    Pop,
    Nil,
    Call(u8),
    Recur,
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

include!(concat!(env!("OUT_DIR"), "/slug_blink_program.rs"));

/// Loads the immutable GPIO blink program from its firmware-resident image.
///
/// The return value is the Program and its validated entry chunk index.
pub fn load_blink_program() -> Result<(Program, usize), FlashLoadError> {
    load(&FROZEN_PROGRAM)
}

fn load(image: &FlashProgram) -> Result<(Program, usize), FlashLoadError> {
    let mut chunk = Chunk::new(image.name, 0);
    for constant in image.constants {
        match constant {
            FlashConstant::Text(value) => {
                chunk.constant(Value::string(*value));
            }
            FlashConstant::Number(value) => {
                chunk.constant(Value::Int(i64::from(*value)));
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
            FlashInstruction::True => {
                chunk.emit(Op::True);
            }
            FlashInstruction::False => {
                chunk.emit(Op::False);
            }
            FlashInstruction::Pop => {
                chunk.emit(Op::Pop);
            }
            FlashInstruction::Nil => {
                chunk.emit(Op::Nil);
            }
            FlashInstruction::Recur => {
                chunk.emit(Op::RecurPositional(0));
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
