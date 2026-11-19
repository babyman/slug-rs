//! Lifting a chunk's verified bytecode into the IR.
//!
//! The VM's verifier supplies each instruction's stack height, stack effect,
//! and successors ([`slug_vm::Program::verified_chunk`]), so the lift and the
//! verifier cannot disagree about the stack. Verified bytecode jumps only
//! forward and loops only through `recur`, which restarts the entry, so the
//! blocks in bytecode order are already in a topological order: each block's
//! predecessors are lifted before it, and SSA construction is one pass.

use std::collections::BTreeSet;

use slug_vm::{Capture, Op, Program, ReadSource, VerifiedChunk, VerifiedInstruction};

use crate::ir::{
    Block, BlockId, Edge, Function, Inst, Local, Snapshot, Terminator, ValueDef, ValueId,
};

/// Lifts chunk `chunk` of `program`.
///
/// # Errors
///
/// Returns why the chunk cannot be lifted: it does not verify, or its code
/// reads a value below its own on the operand stack.
pub fn lift(program: &Program, chunk: usize) -> Result<Function, String> {
    let verified = program.verified_chunk(chunk)?;
    let cells = captured_locals(program, &verified)?;
    Lifter::new(chunk, &verified, cells).run()
}

/// Locals a `MakeClosure` captures, which the VM promotes to cells.
fn captured_locals(program: &Program, chunk: &VerifiedChunk) -> Result<Vec<bool>, String> {
    let mut cells = vec![false; chunk.locals];
    for instruction in &chunk.instructions {
        let captures = match &instruction.op {
            Op::MakeClosure { captures, .. } => captures.as_slice(),
            Op::MakeClosurePooled { captures, .. } => program
                .captures(*captures)
                .ok_or("a closure's capture list is missing")?,
            _ => continue,
        };
        for capture in captures {
            if let Capture::Local(slot) = capture {
                *cells
                    .get_mut(*slot)
                    .ok_or_else(|| format!("a closure captures missing local {slot}"))? = true;
            }
        }
    }
    Ok(cells)
}

/// The frame between instructions: what each local and stack slot holds.
#[derive(Clone)]
struct State {
    locals: Vec<Local>,
    stack: Vec<ValueId>,
}

/// An edge whose target's parameters are not known yet.
struct Incoming {
    from: usize,
    edge: usize,
    state: State,
}

struct Lifter<'a> {
    chunk: &'a VerifiedChunk,
    function: Function,
    /// Which instructions start a block.
    leaders: BTreeSet<usize>,
    /// Pending edges into each block-starting instruction.
    incoming: Vec<Vec<Incoming>>,
    /// The block each block-starting instruction became.
    block_at: Vec<Option<BlockId>>,
}

impl<'a> Lifter<'a> {
    fn new(chunk_index: usize, chunk: &'a VerifiedChunk, cells: Vec<bool>) -> Self {
        let length = chunk.instructions.len();
        Self {
            chunk,
            function: Function {
                chunk: chunk_index,
                name: chunk.name.clone(),
                arity: chunk.arity,
                locals: chunk.locals,
                cells,
                exact_positional: chunk.exact_positional,
                constants: chunk.constants.clone(),
                values: Vec::new(),
                blocks: Vec::new(),
            },
            leaders: BTreeSet::new(),
            incoming: (0..length).map(|_| Vec::new()).collect(),
            block_at: vec![None; length],
        }
    }

    fn run(mut self) -> Result<Function, String> {
        if self.chunk.locals < self.chunk.arity {
            return Err(format!(
                "function `{}` has fewer locals than parameters",
                self.chunk.name
            ));
        }
        self.find_leaders();
        let mut state = Some(self.entry_state());
        for ip in 0..self.chunk.instructions.len() {
            let instruction = &self.chunk.instructions[ip];
            let Some(height) = instruction.height else {
                continue;
            };
            if ip > 0 && self.leaders.contains(&ip) {
                if let Some(state) = state.take() {
                    // The previous block falls through into this one.
                    let from = self.current_block();
                    self.end_block(Terminator::Jump(Self::edge_to(ip)));
                    self.incoming[ip].push(Incoming {
                        from,
                        edge: 0,
                        state,
                    });
                }
                state = Some(self.start_block(ip)?);
            }
            let current = state
                .as_mut()
                .ok_or_else(|| format!("instruction {ip} is reached but starts no block"))?;
            if current.stack.len() != height {
                return Err(format!(
                    "instruction {ip} has {} stack values, verification says {height}",
                    current.stack.len()
                ));
            }
            if self.lift_instruction(ip, instruction, current)? {
                state = None;
            }
        }
        if state.is_some() {
            return Err(format!("function `{}` falls off its end", self.chunk.name));
        }
        self.resolve_edges()?;
        Ok(self.function)
    }

    /// Edges name their target instruction until every block exists; this
    /// replaces each with its block.
    fn resolve_edges(&mut self) -> Result<(), String> {
        let block_at = &self.block_at;
        for block in &mut self.function.blocks {
            let count = block.terminator.edges().len();
            for index in 0..count {
                let edge = block.terminator.edge_mut(index);
                edge.block = block_at[edge.block.index()].ok_or_else(|| {
                    format!(
                        "an edge goes to instruction {}, which starts no block",
                        edge.block.0
                    )
                })?;
            }
        }
        Ok(())
    }

    /// Block starts: the entry, every jump target, and the instruction after
    /// any instruction that does not simply fall through.
    fn find_leaders(&mut self) {
        self.leaders.insert(0);
        for (ip, instruction) in self.chunk.instructions.iter().enumerate() {
            if instruction.height.is_none() {
                continue;
            }
            let falls_through = instruction.successors.len() == 1
                && instruction.successors[0].0 == ip + 1
                && !instruction.recurs;
            if !falls_through {
                for (target, _) in &instruction.successors {
                    self.leaders.insert(*target);
                }
                if ip + 1 < self.chunk.instructions.len() {
                    self.leaders.insert(ip + 1);
                }
            }
        }
    }

    /// The entry block: its parameters are the arguments; other locals are
    /// unset; the values below the function's own are the caller's.
    fn entry_state(&mut self) -> State {
        let block = self.push_block(0);
        let mut locals = Vec::with_capacity(self.chunk.locals);
        for index in 0..self.chunk.locals {
            if index < self.chunk.arity {
                let param = self.new_value(ValueDef::Param { block, index });
                self.function.blocks[block.index()].params.push(param);
                locals.push(self.local_holding(index, param));
            } else {
                let unset = self.new_value(ValueDef::UnsetLocal);
                locals.push(self.local_holding(index, unset));
            }
        }
        let stack = (0..self.chunk.initial_height)
            .map(|index| self.new_value(ValueDef::Below(index)))
            .collect();
        self.block_at[0] = Some(block);
        State { locals, stack }
    }

    fn local_holding(&self, index: usize, value: ValueId) -> Local {
        if self.function.cells[index] {
            Local::Cell
        } else {
            Local::Value(value)
        }
    }

    /// Starts the block at `ip`, joining the states its edges bring: a slot
    /// every edge agrees on keeps its value, and any other becomes a
    /// parameter. The stack is as high as the lowest edge brings it, as in
    /// verification; values above that are dead.
    fn start_block(&mut self, ip: usize) -> Result<State, String> {
        let incoming = std::mem::take(&mut self.incoming[ip]);
        let height = self.chunk.instructions[ip]
            .height
            .ok_or_else(|| format!("block at instruction {ip} is unreachable"))?;
        let block = self.push_block(ip);
        self.block_at[ip] = Some(block);
        let Some(first) = incoming.first() else {
            return Err(format!("instruction {ip} starts a block nothing jumps to"));
        };
        let mut joined = State {
            locals: first.state.locals.clone(),
            stack: first
                .state
                .stack
                .get(..height)
                .map(<[_]>::to_vec)
                .ok_or_else(|| {
                    format!("an edge brings fewer than {height} stack values to instruction {ip}")
                })?,
        };
        // Each parameter, with the slot it stands for.
        let mut params: Vec<Slot> = Vec::new();
        for (index, local) in joined.locals.iter_mut().enumerate() {
            let Local::Value(value) = *local else {
                continue;
            };
            if incoming
                .iter()
                .any(|edge| edge.state.locals[index] != Local::Value(value))
            {
                let param = self.new_param(block);
                *local = Local::Value(param);
                params.push(Slot::Local(index));
            }
        }
        for (index, slot) in joined.stack.iter_mut().enumerate() {
            let value = *slot;
            if incoming
                .iter()
                .any(|edge| edge.state.stack.get(index) != Some(&value))
            {
                if matches!(self.function.def(value), ValueDef::Below(_)) {
                    // Every path leaves the caller's values alone.
                    return Err(format!(
                        "paths join with different values below the frame at instruction {ip}"
                    ));
                }
                *slot = self.new_param(block);
                params.push(Slot::Stack(index));
            }
        }
        for edge in &incoming {
            let args = params
                .iter()
                .map(|slot| match slot {
                    Slot::Local(index) => match edge.state.locals[*index] {
                        Local::Value(value) => Ok(value),
                        Local::Cell => Err(format!("local {index} is a cell on one path only")),
                    },
                    Slot::Stack(index) => edge.state.stack.get(*index).copied().ok_or_else(|| {
                        format!("an edge to instruction {ip} is missing stack value {index}")
                    }),
                })
                .collect::<Result<Vec<_>, String>>()?;
            self.function.blocks[edge.from]
                .terminator
                .edge_mut(edge.edge)
                .args = args;
        }
        Ok(joined)
    }

    /// Lifts one instruction. Returns whether it ended the block.
    #[allow(clippy::too_many_lines)]
    fn lift_instruction(
        &mut self,
        ip: usize,
        instruction: &VerifiedInstruction,
        state: &mut State,
    ) -> Result<bool, String> {
        let before = state.clone();
        let snapshot = Snapshot {
            ip,
            locals: before.locals.clone(),
            stack: before.stack.clone(),
        };
        match &instruction.op {
            Op::Pop => {
                self.pop(state, 1)?;
            }
            Op::Duplicate => {
                let top = *state.stack.last().ok_or("`Duplicate` on an empty stack")?;
                self.check_own(top)?;
                state.stack.push(top);
            }
            Op::GetLocal(slot) => match Self::local(state, *slot)? {
                Local::Value(value) => state.stack.push(value),
                Local::Cell => self.emit(ip, instruction, Vec::new(), state, snapshot)?,
            },
            Op::SetLocal(slot) => match Self::local(state, *slot)? {
                Local::Value(_) => state.locals[*slot] = Local::Value(self.pop(state, 1)?[0]),
                Local::Cell => self.emit(ip, instruction, Vec::new(), state, snapshot)?,
            },
            Op::LocalConstantBinary { slot, .. }
            | Op::ReadIndex {
                source: ReadSource::Local(slot),
            }
            | Op::ReadIndexConstant {
                source: ReadSource::Local(slot),
                ..
            } => {
                let local = match Self::local(state, *slot)? {
                    Local::Value(value) => vec![value],
                    Local::Cell => Vec::new(),
                };
                self.emit(ip, instruction, local, state, snapshot)?;
            }
            Op::MatchLocals { .. } | Op::MatchLocalsPooled { .. } => {
                // The pattern's subjects are the leading parameters.
                let parameters = state.locals[..self.chunk.arity]
                    .iter()
                    .filter_map(|local| match local {
                        Local::Value(value) => Some(*value),
                        Local::Cell => None,
                    })
                    .collect();
                self.emit(ip, instruction, parameters, state, snapshot)?;
                self.end_choice(ip, instruction, &before, state)?;
                return Ok(true);
            }
            Op::MatchCase { .. } | Op::MatchCasePooled { .. } | Op::JumpIfProvided { .. } => {
                self.emit(ip, instruction, Vec::new(), state, snapshot)?;
                self.end_choice(ip, instruction, &before, state)?;
                return Ok(true);
            }
            Op::Return => {
                let value = self.pop(state, 1)?;
                self.end_block(Terminator::Return(value[0]));
                return Ok(true);
            }
            Op::Recur(_) | Op::RecurPooled(_) | Op::RecurPositional(_) => {
                self.emit(ip, instruction, Vec::new(), state, snapshot)?;
                let arguments = self.last_inst().args.clone();
                self.end_block(Terminator::Recur(arguments));
                return Ok(true);
            }
            Op::Throw | Op::MatchFailure | Op::NotImplemented => {
                self.emit(ip, instruction, Vec::new(), state, snapshot)?;
                self.end_block(Terminator::Stop);
                return Ok(true);
            }
            Op::Jump(target) => {
                Self::check_successors(ip, instruction, &[(*target, state.stack.len())])?;
                let from = self.current_block();
                self.end_block(Terminator::Jump(Self::edge_to(*target)));
                self.incoming[*target].push(Incoming {
                    from,
                    edge: 0,
                    state: state.clone(),
                });
                return Ok(true);
            }
            Op::JumpIfFalse(target) | Op::JumpIfFalsePop(target) => {
                let cond = if matches!(instruction.op, Op::JumpIfFalsePop(_)) {
                    self.pop(state, 1)?[0]
                } else {
                    let top = *state
                        .stack
                        .last()
                        .ok_or("`JumpIfFalse` on an empty stack")?;
                    self.check_own(top)?;
                    top
                };
                let height = state.stack.len();
                Self::check_successors(ip, instruction, &[(*target, height), (ip + 1, height)])?;
                let from = self.current_block();
                self.end_block(Terminator::Branch {
                    cond,
                    then: Self::edge_to(ip + 1),
                    otherwise: Self::edge_to(*target),
                });
                for (edge, to) in [(0, ip + 1), (1, *target)] {
                    self.incoming[to].push(Incoming {
                        from,
                        edge,
                        state: state.clone(),
                    });
                }
                return Ok(true);
            }
            _ => self.emit(ip, instruction, Vec::new(), state, snapshot)?,
        }
        let next = state.stack.len();
        if !instruction.successors.is_empty() {
            Self::check_successors(ip, instruction, &[(ip + 1, next)])?;
        }
        Ok(false)
    }

    /// Emits the instruction as an IR instruction: it pops its operands,
    /// reads `extra` (locals it reads), and pushes fresh results.
    fn emit(
        &mut self,
        ip: usize,
        instruction: &VerifiedInstruction,
        extra: Vec<ValueId>,
        state: &mut State,
        snapshot: Snapshot,
    ) -> Result<(), String> {
        let mut args = self.pop(state, instruction.pops)?;
        args.extend(extra);
        let block = self.current_block();
        let inst = self.function.blocks[block].insts.len();
        let block_id = BlockId(u32::try_from(block).map_err(|_| "too many blocks")?);
        let results = (0..instruction.pushes)
            .map(|index| {
                self.new_value(ValueDef::Result {
                    block: block_id,
                    inst,
                    index,
                })
            })
            .collect::<Vec<_>>();
        state.stack.extend(&results);
        self.function.blocks[block].insts.push(Inst {
            ip,
            op: instruction.op.clone(),
            args,
            results,
            snapshot,
        });
        Ok(())
    }

    /// Ends a block whose last instruction chose between falling through,
    /// with its results, and its target, with the stack from before it.
    fn end_choice(
        &mut self,
        ip: usize,
        instruction: &VerifiedInstruction,
        before: &State,
        after: &State,
    ) -> Result<(), String> {
        let [(target, target_height), (next, next_height)] = instruction.successors[..] else {
            return Err(format!(
                "instruction {ip} should have a target and a fall-through"
            ));
        };
        if next != ip + 1 || next_height != after.stack.len() {
            return Err(format!(
                "instruction {ip} falls through with {} stack values, verification says {next_height}",
                after.stack.len()
            ));
        }
        let target_state = State {
            locals: before.locals.clone(),
            stack: before
                .stack
                .get(..target_height)
                .ok_or_else(|| format!("instruction {ip} jumps with more values than it had"))?
                .to_vec(),
        };
        let from = self.current_block();
        self.end_block(Terminator::Choose {
            fallthrough: Self::edge_to(next),
            target: Self::edge_to(target),
        });
        self.incoming[next].push(Incoming {
            from,
            edge: 0,
            state: after.clone(),
        });
        self.incoming[target].push(Incoming {
            from,
            edge: 1,
            state: target_state,
        });
        Ok(())
    }

    fn check_successors(
        ip: usize,
        instruction: &VerifiedInstruction,
        lifted: &[(usize, usize)],
    ) -> Result<(), String> {
        let mut expected = instruction.successors.clone();
        let mut actual = lifted.to_vec();
        expected.sort_unstable();
        actual.sort_unstable();
        if expected == actual {
            Ok(())
        } else {
            Err(format!(
                "instruction {ip} continues to {actual:?}, verification says {expected:?}"
            ))
        }
    }

    fn pop(&self, state: &mut State, count: usize) -> Result<Vec<ValueId>, String> {
        let start = state
            .stack
            .len()
            .checked_sub(count)
            .ok_or("an instruction pops more values than the stack has")?;
        let values = state.stack.split_off(start);
        for value in &values {
            self.check_own(*value)?;
        }
        Ok(values)
    }

    fn check_own(&self, value: ValueId) -> Result<(), String> {
        if let ValueDef::Below(index) = self.function.def(value) {
            return Err(format!(
                "code reads the caller's stack value {index}, below its own"
            ));
        }
        Ok(())
    }

    fn local(state: &State, slot: usize) -> Result<Local, String> {
        state
            .locals
            .get(slot)
            .copied()
            .ok_or_else(|| format!("missing local {slot}"))
    }

    fn edge_to(ip: usize) -> Edge {
        // The target's block number is filled in when it starts; until
        // then the edge names the instruction.
        Edge {
            block: BlockId(u32::try_from(ip).expect("instruction count fits")),
            args: Vec::new(),
        }
    }

    fn push_block(&mut self, start: usize) -> BlockId {
        let id = BlockId(u32::try_from(self.function.blocks.len()).expect("block count fits"));
        self.function.blocks.push(Block {
            start,
            params: Vec::new(),
            insts: Vec::new(),
            terminator: Terminator::Stop,
        });
        id
    }

    fn current_block(&self) -> usize {
        self.function.blocks.len() - 1
    }

    fn end_block(&mut self, terminator: Terminator) {
        let block = self.current_block();
        self.function.blocks[block].terminator = terminator;
    }

    fn last_inst(&self) -> &Inst {
        self.function.blocks[self.current_block()]
            .insts
            .last()
            .expect("the block has an instruction")
    }

    fn new_value(&mut self, def: ValueDef) -> ValueId {
        let id = ValueId(u32::try_from(self.function.values.len()).expect("value count fits"));
        self.function.values.push(def);
        id
    }

    fn new_param(&mut self, block: BlockId) -> ValueId {
        let index = self.function.blocks[block.index()].params.len();
        let value = self.new_value(ValueDef::Param { block, index });
        self.function.blocks[block.index()].params.push(value);
        value
    }
}

/// A slot a block parameter stands for.
enum Slot {
    Local(usize),
    Stack(usize),
}
