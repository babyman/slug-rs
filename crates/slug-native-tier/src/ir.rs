//! The native tier's IR: SSA over basic blocks, lifted from a chunk's
//! verified bytecode (see [`crate::lift`]).
//!
//! Every value is defined once. Locals and operand-stack slots are not
//! storage here but names for values: reading a local is using the value it
//! holds, and where paths with different values join, the block takes a
//! parameter. A local that a closure captures is the exception: it lives in
//! the frame as a cell, and reads and writes of it stay instructions.
//!
//! Each instruction records the frame as it stands just before it (a
//! [`Snapshot`]), so native code can hand the frame to the interpreter there
//! and the interpreter runs the instruction itself.
//!
//! Types are not recorded yet: Phase 1 lowers every value as unknown.

use core::fmt;

use slug_vm::Op;

/// A value, defined once.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValueId(pub u32);

/// A basic block. Blocks are numbered in bytecode order, and every jump
/// but `recur` goes to a later block.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockId(pub u32);

impl ValueId {
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl BlockId {
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where a value comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueDef {
    /// A parameter of `block`: an argument for the entry block, a joined
    /// value for any other.
    Param { block: BlockId, index: usize },
    /// Result `index` of instruction `inst` of `block`.
    Result {
        block: BlockId,
        inst: usize,
        index: usize,
    },
    /// What a local holds before the function sets it: `nil`, as the VM
    /// fills a frame's locals past its arguments.
    UnsetLocal,
    /// The `index`th operand-stack value below the function's own, which
    /// the caller left. Lifting rejects code that reads one.
    Below(usize),
}

/// A local as a snapshot sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Local {
    Value(ValueId),
    /// A captured local, which lives in the frame as a cell.
    Cell,
}

/// The frame just before an instruction runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub ip: usize,
    pub locals: Vec<Local>,
    /// The operand stack, bottom first, including the values below the
    /// function's own.
    pub stack: Vec<ValueId>,
}

/// One bytecode instruction that does more than move values between
/// locals and the stack.
#[derive(Clone, Debug)]
pub struct Inst {
    pub ip: usize,
    pub op: Op,
    /// The values it pops, bottom first, then any locals it reads.
    pub args: Vec<ValueId>,
    /// The values it pushes, bottom first, on its fall-through path.
    pub results: Vec<ValueId>,
    pub snapshot: Snapshot,
}

/// A control-flow edge, with the values for the target's parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edge {
    pub block: BlockId,
    pub args: Vec<ValueId>,
}

/// How a block ends.
#[derive(Clone, Debug)]
pub enum Terminator {
    Jump(Edge),
    /// `JumpIfFalse` or `JumpIfFalsePop`: `then` when `cond` is truthy.
    Branch {
        cond: ValueId,
        then: Edge,
        otherwise: Edge,
    },
    /// The block's last instruction chose a path: `MatchCase`,
    /// `MatchLocals`, or `JumpIfProvided`. Its results reach only
    /// `fallthrough`; `target` gets the stack from before it.
    Choose {
        fallthrough: Edge,
        target: Edge,
    },
    Return(ValueId),
    /// The block's last instruction is a `recur` with these arguments,
    /// which restarts the entry block with fresh locals.
    Recur(Vec<ValueId>),
    /// The block's last instruction does not continue: `Throw`,
    /// `MatchFailure`, or `NotImplemented`.
    Stop,
}

impl Terminator {
    /// The edges to later blocks.
    #[must_use]
    pub fn edges(&self) -> Vec<&Edge> {
        match self {
            Self::Jump(edge) => vec![edge],
            Self::Branch {
                then, otherwise, ..
            } => vec![then, otherwise],
            Self::Choose {
                fallthrough,
                target,
            } => vec![fallthrough, target],
            Self::Return(_) | Self::Recur(_) | Self::Stop => Vec::new(),
        }
    }

    fn edges_mut(&mut self) -> Vec<&mut Edge> {
        match self {
            Self::Jump(edge) => vec![edge],
            Self::Branch {
                then, otherwise, ..
            } => vec![then, otherwise],
            Self::Choose {
                fallthrough,
                target,
            } => vec![fallthrough, target],
            Self::Return(_) | Self::Recur(_) | Self::Stop => Vec::new(),
        }
    }

    pub(crate) fn edge_mut(&mut self, index: usize) -> &mut Edge {
        self.edges_mut()
            .into_iter()
            .nth(index)
            .expect("terminator has the edge")
    }
}

/// A basic block.
#[derive(Clone, Debug)]
pub struct Block {
    /// Its first instruction.
    pub start: usize,
    pub params: Vec<ValueId>,
    pub insts: Vec<Inst>,
    pub terminator: Terminator,
}

/// One chunk, lifted.
#[derive(Clone, Debug)]
pub struct Function {
    pub chunk: usize,
    pub name: String,
    pub arity: usize,
    /// Local slots, parameters first.
    pub locals: usize,
    /// Which locals are captured, and so live in the frame as cells.
    pub cells: Vec<bool>,
    pub values: Vec<ValueDef>,
    /// The entry block is first; its parameters are the arguments.
    pub blocks: Vec<Block>,
}

impl Function {
    #[must_use]
    pub fn def(&self, value: ValueId) -> ValueDef {
        self.values[value.index()]
    }

    /// Checks the IR's invariants: every value is defined once and used
    /// only where its definition dominates; every edge passes one value per
    /// parameter and goes forward; no code reads below
    /// the function's own stack values; and snapshots have every local.
    ///
    /// # Errors
    ///
    /// Returns the first broken invariant.
    pub fn check(&self) -> Result<(), String> {
        let dominators = self.dominators()?;
        let defined_at = self.definitions()?;
        let available = |value: ValueId, at: (usize, usize)| -> Result<(), String> {
            match self.def(value) {
                ValueDef::UnsetLocal => Ok(()),
                ValueDef::Below(_) => Err(format!("{value:?} is below the function's stack")),
                _ => match defined_at[value.index()] {
                    Some((block, position))
                        if (block == at.0 && position <= at.1)
                            || (block != at.0 && dominators[at.0].contains(&block)) =>
                    {
                        Ok(())
                    }
                    _ => Err(format!(
                        "{value:?} is used in block{} where its definition does not dominate",
                        at.0
                    )),
                },
            }
        };
        let snapshot_available = |snapshot: &Snapshot, at: (usize, usize)| {
            if snapshot.locals.len() != self.locals {
                return Err(format!(
                    "snapshot at instruction {} has {} locals, not {}",
                    snapshot.ip,
                    snapshot.locals.len(),
                    self.locals
                ));
            }
            for local in &snapshot.locals {
                if let Local::Value(value) = local {
                    available(*value, at)?;
                }
            }
            for value in &snapshot.stack {
                if !matches!(self.def(*value), ValueDef::Below(_)) {
                    available(*value, at)?;
                }
            }
            Ok(())
        };
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (inst_index, inst) in block.insts.iter().enumerate() {
                let at = (block_index, inst_index);
                for arg in &inst.args {
                    available(*arg, at)?;
                }
                snapshot_available(&inst.snapshot, at)?;
            }
            let end = (block_index, block.insts.len());
            for edge in block.terminator.edges() {
                let target = &self.blocks[edge.block.index()];
                if edge.block.index() <= block_index {
                    return Err(format!("block{block_index} jumps back to {:?}", edge.block));
                }
                if edge.args.len() != target.params.len() {
                    return Err(format!(
                        "block{block_index} passes {} values to {:?}, which takes {}",
                        edge.args.len(),
                        edge.block,
                        target.params.len()
                    ));
                }
                for arg in &edge.args {
                    available(*arg, end)?;
                }
            }
            match &block.terminator {
                Terminator::Branch { cond, .. } => available(*cond, end)?,
                Terminator::Return(value) => available(*value, end)?,
                Terminator::Recur(arguments) => {
                    for argument in arguments {
                        available(*argument, end)?;
                    }
                }
                Terminator::Jump(_) | Terminator::Choose { .. } | Terminator::Stop => {}
            }
        }
        Ok(())
    }

    /// Where each value is defined: its block, and the position in it after
    /// which it exists (0 for a parameter, `i + 1` for a result of
    /// instruction `i`). Checks each definition matches its [`ValueDef`].
    fn definitions(&self) -> Result<Vec<Option<(usize, usize)>>, String> {
        let mut defined_at = vec![None; self.values.len()];
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (index, param) in block.params.iter().enumerate() {
                let matches = matches!(
                    self.def(*param),
                    ValueDef::Param { block, index: defined }
                        if block.index() == block_index && defined == index
                );
                if !matches {
                    return Err(format!(
                        "{param} is not parameter {index} of block{block_index}"
                    ));
                }
                Self::define(&mut defined_at, *param, (block_index, 0))?;
            }
            for (inst_index, inst) in block.insts.iter().enumerate() {
                for (index, result) in inst.results.iter().enumerate() {
                    let matches = matches!(
                        self.def(*result),
                        ValueDef::Result { block, inst, index: defined }
                            if block.index() == block_index && inst == inst_index && defined == index
                    );
                    if !matches {
                        return Err(format!(
                            "{result} is not result {index} of block{block_index} instruction {inst_index}"
                        ));
                    }
                    Self::define(&mut defined_at, *result, (block_index, inst_index + 1))?;
                }
            }
        }
        Ok(defined_at)
    }

    fn define(
        defined_at: &mut [Option<(usize, usize)>],
        value: ValueId,
        at: (usize, usize),
    ) -> Result<(), String> {
        let slot = &mut defined_at[value.index()];
        if slot.is_some() {
            return Err(format!("{value:?} is defined twice"));
        }
        *slot = Some(at);
        Ok(())
    }

    /// Each block's dominators, including itself. Blocks are in a
    /// topological order (jumps go forward, and `recur` returns to the
    /// entry, which dominates everything), so one pass suffices.
    fn dominators(&self) -> Result<Vec<Vec<usize>>, String> {
        let mut predecessors = vec![Vec::new(); self.blocks.len()];
        for (index, block) in self.blocks.iter().enumerate() {
            for edge in block.terminator.edges() {
                predecessors
                    .get_mut(edge.block.index())
                    .ok_or_else(|| format!("block{index} jumps to missing {:?}", edge.block))?
                    .push(index);
            }
        }
        let mut dominators: Vec<Vec<usize>> = Vec::with_capacity(self.blocks.len());
        for (index, incoming) in predecessors.iter().enumerate() {
            let mut set = if index == 0 {
                Vec::new()
            } else {
                let Some((first, rest)) = incoming.split_first() else {
                    return Err(format!("block{index} has no predecessor"));
                };
                let mut set = dominators[*first].clone();
                for predecessor in rest {
                    set.retain(|block| dominators[*predecessor].contains(block));
                }
                set
            };
            set.push(index);
            dominators.push(set);
        }
        Ok(dominators)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "v{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "block{}", self.0)
    }
}

fn list(values: &[ValueId]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Edge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}({})", self.block, list(&self.args))
    }
}

impl fmt::Display for Function {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "function {} (chunk {}, {} parameters, {} locals)",
            self.name, self.chunk, self.arity, self.locals
        )?;
        let cells = self
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| **cell)
            .map(|(index, _)| index.to_string())
            .collect::<Vec<_>>();
        if !cells.is_empty() {
            writeln!(formatter, "  cells: {}", cells.join(", "))?;
        }
        for (index, block) in self.blocks.iter().enumerate() {
            writeln!(
                formatter,
                "block{index}({}):  ; instruction {}",
                list(&block.params),
                block.start
            )?;
            for inst in &block.insts {
                let results = if inst.results.is_empty() {
                    String::new()
                } else {
                    format!("{} = ", list(&inst.results))
                };
                writeln!(
                    formatter,
                    "  {results}{:?}({})  ; instruction {}",
                    inst.op,
                    list(&inst.args),
                    inst.ip
                )?;
            }
            match &block.terminator {
                Terminator::Jump(edge) => writeln!(formatter, "  jump {edge}")?,
                Terminator::Branch {
                    cond,
                    then,
                    otherwise,
                } => writeln!(formatter, "  branch {cond}, {then}, {otherwise}")?,
                Terminator::Choose {
                    fallthrough,
                    target,
                } => writeln!(formatter, "  choose {fallthrough}, {target}")?,
                Terminator::Return(value) => writeln!(formatter, "  return {value}")?,
                Terminator::Recur(arguments) => {
                    writeln!(formatter, "  recur({})", list(arguments))?;
                }
                Terminator::Stop => writeln!(formatter, "  stop")?,
            }
        }
        Ok(())
    }
}
