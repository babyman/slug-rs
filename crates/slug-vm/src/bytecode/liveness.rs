//! Last-read analysis for frame locals.
//!
//! A `GetLocal` after which its slot is never read again can move the value
//! out of the frame instead of cloning it. That keeps a collection held only
//! by that local uniquely owned, so `acc :+ x` appends in place instead of
//! copying.
//!
//! Only `GetLocal`, `LocalConstantBinary`, `SetLocal`, `MakeClosure`, and
//! `MatchLocals` touch local values. A closure reads the local slots in its
//! capture list, which keeps them live; once captured, a slot is a shared
//! cell, which the VM clones rather than moves anyway. A closure whose
//! capture list is missing keeps every slot live, and so does `MatchLocals`,
//! which reads the first locals in place. `Recur` assigns every local before it restarts, and
//! `Return`, `Throw`, `MatchFailure`, and `NotImplemented` leave the frame,
//! so none of them has a live successor.

use alloc::{vec, vec::Vec};

use super::{
    chunk::{PackedInstruction, PackedOpcode},
    metadata::Capture,
};

/// Operand `b` of a `GetLocal` that moves its value out of the slot.
pub(crate) const MOVE_LOCAL: u32 = 1;

/// Bit of operand `c` of a `LocalConstantBinary` that moves its local out of
/// the slot. The low bits hold the operator.
pub(crate) const MOVE_LOCAL_OPERAND: u32 = 1 << 31;

/// The most `u64` words of live-slot bitsets the analysis allocates for one
/// chunk, 32 MiB: a bitset of the chunk's locals for each instruction.
const ANALYSIS_WORDS: usize = 4 * 1024 * 1024;

/// Sets `b` to [`MOVE_LOCAL`] on each final `GetLocal` read and clears it on
/// every other `GetLocal`, so the flag never comes from untrusted input.
///
/// A chunk whose bitsets would exceed [`ANALYSIS_WORDS`] keeps every flag
/// clear: each read copies its local, which is always correct.
pub(crate) fn mark_last_local_reads(
    code: &mut [PackedInstruction],
    locals: usize,
    capture_lists: &[Vec<Capture>],
) {
    for instruction in code.iter_mut() {
        match instruction.opcode {
            PackedOpcode::GetLocal => instruction.b = 0,
            PackedOpcode::LocalConstantBinary => instruction.c &= !MOVE_LOCAL_OPERAND,
            _ => {}
        }
    }
    let words = locals.div_ceil(64);
    if code
        .len()
        .checked_add(1)
        .and_then(|rows| rows.checked_mul(words))
        .is_none_or(|total| total > ANALYSIS_WORDS)
    {
        return;
    }
    let mut live_in = vec![vec![0_u64; words]; code.len() + 1];
    // Verified jumps go forward, so this settles in one pass, plus one to
    // confirm it; the fixed point keeps it sound regardless.
    loop {
        let mut changed = false;
        for index in (0..code.len()).rev() {
            let live = live_before(code[index], index, &live_in, locals, capture_lists);
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
    capture_lists: &[Vec<Capture>],
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
        PackedOpcode::JumpIfProvided | PackedOpcode::MatchCase | PackedOpcode::MatchLocals => {
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
        PackedOpcode::MakeClosure => match capture_lists.get(instruction.b as usize) {
            Some(captures) => {
                for capture in captures {
                    if let Capture::Local(slot) = *capture
                        && slot < locals
                    {
                        live[slot / 64] |= 1 << (slot % 64);
                    }
                }
            }
            None => live.fill(u64::MAX),
        },
        // Its pattern, which says how many locals it reads, is not at hand.
        PackedOpcode::MatchLocals => live.fill(u64::MAX),
        _ => {}
    }
    live
}

fn contains(live: &[u64], slot: usize) -> bool {
    live[slot / 64] & (1 << (slot % 64)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(opcode: PackedOpcode, a: u32, b: u32, c: u32) -> PackedInstruction {
        PackedInstruction::from_cslug(opcode, a, b, c, None)
    }

    #[test]
    fn moves_a_local_at_its_last_read() {
        let mut code = [
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
        ];
        mark_last_local_reads(&mut code, 1, &[]);
        assert_eq!(code[0].b, 0);
        assert_eq!(code[1].b, MOVE_LOCAL);
    }

    #[test]
    fn a_failed_match_case_jump_keeps_its_target_reads_live() {
        let mut code = [
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::MatchCase, 0, 3, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
        ];
        mark_last_local_reads(&mut code, 1, &[]);
        assert_eq!(code[0].b, 0);
        assert_eq!(code[3].b, MOVE_LOCAL);
    }

    #[test]
    fn a_closure_keeps_only_its_captured_locals_live() {
        let mut code = [
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::GetLocal, 1, 0, 0),
            instruction(PackedOpcode::MakeClosure, 0, 0, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
        ];
        mark_last_local_reads(&mut code, 2, &[vec![Capture::Local(1)]]);
        assert_eq!(code[0].b, MOVE_LOCAL);
        assert_eq!(code[1].b, 0);
    }

    #[test]
    fn a_closure_without_capture_metadata_keeps_every_local_live() {
        let mut code = [
            instruction(PackedOpcode::GetLocal, 0, 0, 0),
            instruction(PackedOpcode::MakeClosure, 0, 0, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
        ];
        mark_last_local_reads(&mut code, 1, &[]);
        assert_eq!(code[0].b, 0);
    }

    #[test]
    fn copies_every_read_when_the_analysis_exceeds_its_budget() {
        // Supplied move flags on reads that are not last reads.
        let mut code = [
            instruction(PackedOpcode::GetLocal, 0, MOVE_LOCAL, 0),
            instruction(PackedOpcode::LocalConstantBinary, 0, 0, MOVE_LOCAL_OPERAND),
            instruction(PackedOpcode::GetLocal, 0, MOVE_LOCAL, 0),
            instruction(PackedOpcode::Return, 0, 0, 0),
        ];
        let locals = ANALYSIS_WORDS / code.len() * 64 + 64;
        mark_last_local_reads(&mut code, locals, &[]);
        assert_eq!(code[0].b, 0);
        assert_eq!(code[1].c & MOVE_LOCAL_OPERAND, 0);
        assert_eq!(code[2].b, 0);
    }
}
