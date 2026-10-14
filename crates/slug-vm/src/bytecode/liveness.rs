//! Last-read analysis for frame locals.
//!
//! A `GetLocal` after which its slot is never read again can move the value
//! out of the frame instead of cloning it. That keeps a collection held only
//! by that local uniquely owned, so `acc :+ x` appends in place instead of
//! copying.
//!
//! Only `GetLocal`, `LocalConstantBinary`, `SetLocal`, and `MakeClosure`
//! touch local values. A
//! closure keeps every slot live because its capture list is pooled
//! metadata; a captured slot is a shared cell, which the VM clones rather
//! than moves anyway. `Recur` assigns every local before it restarts, and
//! `Return`, `Throw`, `MatchFailure`, and `NotImplemented` leave the frame,
//! so none of them has a live successor.

use alloc::{vec, vec::Vec};

use super::chunk::{PackedInstruction, PackedOpcode};

/// Operand `b` of a `GetLocal` that moves its value out of the slot.
pub(crate) const MOVE_LOCAL: u32 = 1;

/// Bit of operand `c` of a `LocalConstantBinary` that moves its local out of
/// the slot. The low bits hold the operator.
pub(crate) const MOVE_LOCAL_OPERAND: u32 = 1 << 31;

/// Sets `b` to [`MOVE_LOCAL`] on each final `GetLocal` read and clears it on
/// every other `GetLocal`, so the flag never comes from untrusted input.
pub(crate) fn mark_last_local_reads(code: &mut [PackedInstruction], locals: usize) {
    let words = locals.div_ceil(64);
    let mut live_in = vec![vec![0_u64; words]; code.len() + 1];
    // Jumps are forward-only in compiled code, so this settles in one or two
    // passes; the fixed point keeps it sound for any verified target.
    loop {
        let mut changed = false;
        for index in (0..code.len()).rev() {
            let live = live_before(code[index], index, &live_in, locals);
            if live != live_in[index] {
                live_in[index] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for index in 0..code.len() {
        let slot = code[index].a as usize;
        let moves = slot < locals && !contains(&live_in[index + 1], slot);
        match code[index].opcode {
            PackedOpcode::GetLocal => code[index].b = if moves { MOVE_LOCAL } else { 0 },
            PackedOpcode::LocalConstantBinary => {
                code[index].c &= !MOVE_LOCAL_OPERAND;
                if moves {
                    code[index].c |= MOVE_LOCAL_OPERAND;
                }
            }
            _ => {}
        }
    }
}

fn live_before(
    instruction: PackedInstruction,
    index: usize,
    live_in: &[Vec<u64>],
    locals: usize,
) -> Vec<u64> {
    let mut live = vec![0_u64; live_in[0].len()];
    let mut join = |target: usize| {
        let successor = &live_in[target.min(live_in.len() - 1)];
        for (word, bits) in live.iter_mut().zip(successor) {
            *word |= bits;
        }
    };
    match instruction.opcode {
        PackedOpcode::Return
        | PackedOpcode::Throw
        | PackedOpcode::MatchFailure
        | PackedOpcode::NotImplemented
        | PackedOpcode::Recur
        | PackedOpcode::RecurPositional => {}
        PackedOpcode::Jump => join(instruction.a as usize),
        PackedOpcode::JumpIfFalse | PackedOpcode::JumpIfFalsePop => {
            join(instruction.a as usize);
            join(index + 1);
        }
        PackedOpcode::JumpIfProvided => {
            join(instruction.b as usize);
            join(index + 1);
        }
        _ => join(index + 1),
    }
    let slot = instruction.a as usize;
    match instruction.opcode {
        PackedOpcode::SetLocal if slot < locals => live[slot / 64] &= !(1 << (slot % 64)),
        PackedOpcode::GetLocal | PackedOpcode::LocalConstantBinary if slot < locals => {
            live[slot / 64] |= 1 << (slot % 64);
        }
        PackedOpcode::MakeClosure => live.fill(u64::MAX),
        _ => {}
    }
    live
}

fn contains(live: &[u64], slot: usize) -> bool {
    live[slot / 64] & (1 << (slot % 64)) != 0
}
