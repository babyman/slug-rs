//! Slug's clean-room bytecode runtime.
//!
//! This crate deliberately keeps bytecode an implementation detail. The public
//! source-language specification makes no promise about opcode values or a
//! serialised format. `Program`, `Chunk`, and `Op` are therefore a compiler-to-VM
//! boundary, designed for clarity and validation rather than persistence.

mod bytecode;
mod source;
mod value;
mod vm;

pub use bytecode::{Chunk, Constant, Instruction, Op, Program, SourceSpan};
pub use source::{SourceError, compile};
pub use value::{Closure, NativeFunction, Value};
pub use vm::{CallFrame, RuntimeError, RuntimeErrorKind, Vm, VmResult};
