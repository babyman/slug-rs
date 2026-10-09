//! Private table image support for firmware builds.
//!
//! This module is deliberately limited to generated Rust included in one
//! firmware build. It is neither a byte-stream codec nor a compatible public
//! representation; portable program persistence belongs to `.cslug`.

use alloc::string::String;
use core::fmt;

use crate::{Chunk, Op, Program, Value};

/// A constant embedded in a private firmware image.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum FrozenConstant {
    Text(&'static str),
    Integer(i64),
}

/// An instruction embedded in a private firmware image.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum FrozenInstruction {
    GetGlobal(&'static str),
    Constant(u32),
    True,
    False,
    Pop,
    Nil,
    Call(u32),
    RecurPositional(u32),
    Return,
}

/// One private frozen chunk compiled into a firmware binary.
#[doc(hidden)]
pub struct FrozenChunk {
    pub name: &'static str,
    pub constants: &'static [FrozenConstant],
    pub instructions: &'static [FrozenInstruction],
}

/// A private, build-local firmware program image.
#[doc(hidden)]
pub struct FrozenProgram {
    pub chunks: &'static [FrozenChunk],
    pub entry: u32,
}

/// Checked failure while reconstructing a private frozen program image.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrozenLoadError {
    EntryOutOfBounds {
        entry: u32,
    },
    TableTooLarge {
        table: &'static str,
    },
    ConstantOutOfBounds {
        chunk: u32,
        instruction: u32,
        constant: u32,
    },
}

impl fmt::Display for FrozenLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryOutOfBounds { entry } => {
                write!(f, "frozen program entry {entry} does not name a chunk")
            }
            Self::TableTooLarge { table } => {
                write!(f, "frozen {table} table exceeds the supported limit")
            }
            Self::ConstantOutOfBounds {
                chunk,
                instruction,
                constant,
            } => write!(
                f,
                "frozen chunk {chunk} instruction {instruction} references missing constant {constant}"
            ),
        }
    }
}

/// Reconstructs a checked in-process program from a firmware-local table image.
///
/// The VM performs its complete executable-invariant validation when the
/// returned program is installed. This loader checks every table reference it
/// consumes first, so malformed generated input cannot cause host indexing.
pub fn load(image: &FrozenProgram) -> Result<(Program, usize), FrozenLoadError> {
    let entry = image.entry as usize;
    if entry >= image.chunks.len() {
        return Err(FrozenLoadError::EntryOutOfBounds { entry: image.entry });
    }

    let mut program = Program::new();
    for (chunk_index, image_chunk) in image.chunks.iter().enumerate() {
        let chunk_id = u32::try_from(chunk_index)
            .map_err(|_| FrozenLoadError::TableTooLarge { table: "chunk" })?;
        let mut chunk = Chunk::new(image_chunk.name, 0);
        for constant in image_chunk.constants {
            match constant {
                FrozenConstant::Text(value) => {
                    chunk.constant(Value::string(*value));
                }
                FrozenConstant::Integer(value) => {
                    chunk.constant(Value::Int(*value));
                }
            }
        }
        for (instruction_index, instruction) in image_chunk.instructions.iter().enumerate() {
            let instruction_id =
                u32::try_from(instruction_index).map_err(|_| FrozenLoadError::TableTooLarge {
                    table: "instruction",
                })?;
            let op = match instruction {
                FrozenInstruction::GetGlobal(name) => Op::GetGlobal(String::from(*name)),
                FrozenInstruction::Constant(index) => {
                    if *index as usize >= image_chunk.constants.len() {
                        return Err(FrozenLoadError::ConstantOutOfBounds {
                            chunk: chunk_id,
                            instruction: instruction_id,
                            constant: *index,
                        });
                    }
                    Op::Constant(*index as usize)
                }
                FrozenInstruction::True => Op::True,
                FrozenInstruction::False => Op::False,
                FrozenInstruction::Pop => Op::Pop,
                FrozenInstruction::Nil => Op::Nil,
                FrozenInstruction::Call(arguments) => Op::Call(*arguments as usize),
                FrozenInstruction::RecurPositional(arguments) => {
                    Op::RecurPositional(*arguments as usize)
                }
                FrozenInstruction::Return => Op::Return,
            };
            chunk.emit(op);
        }
        program.add_chunk(chunk);
    }
    Ok((program, entry))
}

#[cfg(test)]
mod tests {
    use super::{
        FrozenChunk, FrozenConstant, FrozenInstruction, FrozenLoadError, FrozenProgram, load,
    };

    #[test]
    fn reconstructs_a_single_chunk_program() {
        const CONSTANTS: &[FrozenConstant] = &[FrozenConstant::Text("hello")];
        const INSTRUCTIONS: &[FrozenInstruction] =
            &[FrozenInstruction::Constant(0), FrozenInstruction::Return];
        const CHUNKS: &[FrozenChunk] = &[FrozenChunk {
            name: "main",
            constants: CONSTANTS,
            instructions: INSTRUCTIONS,
        }];
        let (program, entry) = load(&FrozenProgram {
            chunks: CHUNKS,
            entry: 0,
        })
        .expect("valid image loads");
        assert_eq!(entry, 0);
        assert_eq!(program.chunk_count(), 1);
    }

    #[test]
    fn rejects_invalid_table_references() {
        const BAD_INSTRUCTIONS: &[FrozenInstruction] = &[FrozenInstruction::Constant(1)];
        const BAD_CHUNKS: &[FrozenChunk] = &[FrozenChunk {
            name: "main",
            constants: &[],
            instructions: BAD_INSTRUCTIONS,
        }];
        assert!(matches!(
            load(&FrozenProgram {
                chunks: BAD_CHUNKS,
                entry: 0
            }),
            Err(FrozenLoadError::ConstantOutOfBounds {
                chunk: 0,
                instruction: 0,
                constant: 1,
            })
        ));
        assert!(matches!(
            load(&FrozenProgram {
                chunks: BAD_CHUNKS,
                entry: 1
            }),
            Err(FrozenLoadError::EntryOutOfBounds { entry: 1 })
        ));
    }
}
