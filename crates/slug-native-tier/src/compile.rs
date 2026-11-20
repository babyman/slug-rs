//! Lowering a lifted function to machine code: the baseline tier.
//!
//! Every IR value has a 24-byte slot in the native frame holding a `Value`
//! exactly as the VM lays it out. Arithmetic, division, and comparisons
//! run inline on two integers, and on two numbers of which one is a float
//! (converted as the VM's `numbers` converts them; equality only on two
//! floats); every other case, and every error, calls the interpreter's own
//! operation. Anything that
//! fails, and a thread that needs attention at a `recur`, leaves to the
//! interpreter before the instruction, with the frame from its snapshot.
//!
//! Ownership: a slot whose value may own something (a parameter, or the
//! result of arithmetic, which a slow path can make a string or a list) is
//! `nil` until written, and every such slot is dropped when the function
//! returns or exits and reset when it recurs. Values move into another
//! slot by a clone, so each slot owns what it holds.
//!
//! A call of a module-level name enters the callee's native code directly
//! when it is compiled, with no interpreter frame. If the callee leaves to
//! the interpreter, so does every caller: each leaves its frame as it
//! stands after the call, and the interpreter continues in the callee's.
//!
//! Reads of globals, captures, and collections, constants that are not
//! numbers, and list literals call the VM's `native_*` operations, which
//! read through the running closure; one that cannot answer leaves to the
//! interpreter before the instruction.
//!
//! A function compiles only if it uses constants, locals, arithmetic,
//! comparisons, `not`, negation, branches, those reads, list literals,
//! calls of module-level names, a positional `recur`, and `return` (see
//! [`qualify`]).

use std::sync::atomic::{AtomicU32, Ordering};

use cranelift_codegen::{
    Context,
    ir::{
        AbiParam, Block as CodeBlock, InstBuilder, MemFlags, Signature, StackSlot, StackSlotData,
        StackSlotKind, Value as CodeValue,
        condcodes::{FloatCC, IntCC},
        types,
    },
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use slug_vm::{BinaryOperator, Constant, NativeOperation, Op, ReadSource, Value};

use crate::{
    ir::Snapshot,
    ir::{Block, Edge, Function, Inst, Terminator, ValueDef, ValueId},
    jit::Jit,
    runtime::{
        self, BINARY_OPERATORS, CALL_DECLINED, CALL_RETURNED, Compiled, EXITED, ExitTable,
        NO_CONSTANT, NO_SITE, RETURNED, SOURCE_CAPTURE, SOURCE_CELL, SOURCE_GLOBAL, SOURCE_VALUE,
        STACK_BUDGET, layout, offsets,
    },
};

/// Why a function does not compile, or `Ok` if it can.
///
/// # Errors
///
/// Returns the first thing the baseline tier does not compile yet.
pub fn qualify(function: &Function) -> Result<(), String> {
    let cell = |slot: &usize| function.cells.get(*slot).copied().unwrap_or(true);
    for block in &function.blocks {
        for inst in &block.insts {
            match &inst.op {
                // Only a captured local's reads and writes are instructions.
                Op::GetLocal(slot) | Op::SetLocal(slot) if !cell(slot) => {
                    return Err(format!("local {slot} is read as a cell but is not one"));
                }
                Op::MakeClosure { .. } | Op::MakeClosurePooled { .. }
                    if !function.closures.contains_key(&inst.ip) =>
                {
                    return Err(format!("the closure at instruction {} is missing", inst.ip));
                }
                Op::Constant(index) => {
                    if function.constants.get(*index).is_none() {
                        return Err(format!("constant {index} does not exist"));
                    }
                }
                Op::Nil
                | Op::True
                | Op::False
                | Op::CallGlobalPooled { .. }
                | Op::CallPositional(_)
                | Op::GetGlobalPooled(_)
                | Op::SetGlobalPooled(_)
                | Op::GetCapture(_)
                | Op::SetCapture(_)
                | Op::GetLocal(_)
                | Op::SetLocal(_)
                | Op::MakeClosure { .. }
                | Op::MakeClosurePooled { .. }
                | Op::ListAppend
                | Op::ListPrepend
                | Op::GetSlice { .. }
                | Op::Struct(_)
                | Op::StructPooled(_)
                | Op::StructCopy(_)
                | Op::StructCopyPooled(_)
                | Op::CopyGlobalPooled { .. }
                | Op::UpdateCapture { .. }
                | Op::Map(_)
                | Op::InterpolatePooled(_)
                | Op::MatchCasePooled { .. }
                | Op::TryMatchPooled { .. }
                | Op::JumpIfProvided { .. }
                | Op::Throw
                | Op::MatchFailure
                | Op::NotImplemented
                | Op::Not
                | Op::BitNot
                | Op::Negate
                | Op::List(_)
                | Op::GetIndex
                | Op::GetIndexConstant(_)
                | Op::ReadIndex {
                    source:
                        ReadSource::Local(_) | ReadSource::Capture(_) | ReadSource::GlobalPooled(_),
                }
                | Op::ReadIndexConstant {
                    source:
                        ReadSource::Local(_) | ReadSource::Capture(_) | ReadSource::GlobalPooled(_),
                    ..
                } => {}
                Op::LocalConstantBinary { constant, .. } | Op::ConstantBinary { constant, .. } => {
                    value_constant(function, *constant)?;
                }
                Op::RecurPositional(count)
                    if *count == function.arity && function.exact_positional => {}
                op if binary_operator(op).is_some() => {}
                op => {
                    return Err(format!(
                        "instruction {}, {op:?}, is not compiled yet",
                        inst.ip
                    ));
                }
            }
        }
        match &block.terminator {
            Terminator::Jump(_)
            | Terminator::Branch { .. }
            | Terminator::Return(_)
            | Terminator::Recur(_)
            | Terminator::Choose { .. }
            | Terminator::Stop => {}
        }
    }
    Ok(())
}

/// A constant that is a value rather than a function.
fn value_constant(function: &Function, index: usize) -> Result<&Value, String> {
    match function.constants.get(index) {
        Some(Constant::Value(value)) => Ok(value),
        _ => Err(format!("constant {index} is not a value")),
    }
}

/// A constant the tier writes inline: `nil`, a boolean, or a number.
fn number_constant(function: &Function, index: usize) -> Result<&Value, String> {
    match function.constants.get(index) {
        Some(Constant::Value(
            value @ (Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_)),
        )) => Ok(value),
        _ => Err(format!("constant {index} is not a number, boolean, or nil")),
    }
}

/// The operator an instruction applies to its two operands.
fn binary_operator(op: &Op) -> Option<BinaryOperator> {
    use BinaryOperator as B;
    Some(match op {
        Op::Add => B::Add,
        Op::AddNum => B::AddNum,
        Op::Subtract => B::Subtract,
        Op::SubtractNum => B::SubtractNum,
        Op::Multiply => B::Multiply,
        Op::MultiplyNum => B::MultiplyNum,
        Op::Divide => B::Divide,
        Op::DivideNum => B::DivideNum,
        Op::Modulo => B::Modulo,
        Op::ModuloNum => B::ModuloNum,
        Op::Equal => B::Equal,
        Op::Greater => B::Greater,
        Op::GreaterNum => B::GreaterNum,
        Op::Less => B::Less,
        Op::LessNum => B::LessNum,
        Op::BitAnd => B::BitAnd,
        Op::BitOr => B::BitOr,
        Op::BitXor => B::BitXor,
        Op::ShiftLeft => B::ShiftLeft,
        Op::ShiftRight => B::ShiftRight,
        Op::LocalConstantBinary { operator, .. } | Op::ConstantBinary { operator, .. } => *operator,
        _ => return None,
    })
}

/// Whether `operator`'s result is always a boolean.
fn compares(operator: BinaryOperator) -> bool {
    use BinaryOperator as B;
    matches!(
        operator,
        B::Equal | B::Greater | B::GreaterNum | B::Less | B::LessNum
    )
}

static COMPILED: AtomicU32 = AtomicU32::new(0);

/// Compiles a function that [`qualify`] accepts, printing its machine
/// code on standard error if `dump`.
pub(crate) fn compile(jit: &Jit, function: &Function, dump: bool) -> Result<Compiled, String> {
    qualify(function)?;
    let mut context = Context::new();
    context.set_disasm(dump);
    let pointer = jit.isa().pointer_type();
    let call_conv = jit.isa().default_call_conv();
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(pointer));
    signature.params.push(AbiParam::new(pointer));
    signature.returns.push(AbiParam::new(types::I32));
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    let sites = runtime::call_sites(
        function
            .blocks
            .iter()
            .flat_map(|block| &block.insts)
            .filter(|inst| matches!(inst.op, Op::CallGlobalPooled { .. }))
            .count(),
    );
    let (exits, owning, operations, matches) = {
        let builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        Lowering::new(
            builder,
            function,
            pointer,
            call_conv,
            sites.as_ptr() as usize,
        )
        .lower()?
    };
    let cells = if function.cells.iter().any(|cell| *cell) {
        function.cells.clone()
    } else {
        Vec::new()
    };
    let closures = function.closures.values().cloned().collect();
    let name = format!(
        "slug_{}_{}",
        function.chunk,
        COMPILED.fetch_add(1, Ordering::Relaxed)
    );
    let entry = jit.define(&name, &mut context)?;
    if let Some(code) = context.compiled_code().and_then(|code| code.vcode.as_ref()) {
        eprintln!("{name}:\n{code}");
    }
    Ok(Compiled::new(
        entry,
        function.arity,
        runtime::Tables {
            exits,
            owning,
            cells,
            closures,
            operations,
            matches,
            sites,
        },
    ))
}

/// The Rust functions generated code calls, by signature.
struct Runtime {
    binary: cranelift_codegen::ir::SigRef,
    attention: cranelift_codegen::ir::SigRef,
    clone: cranelift_codegen::ir::SigRef,
    drop: cranelift_codegen::ir::SigRef,
    release: cranelift_codegen::ir::SigRef,
    exit: cranelift_codegen::ir::SigRef,
    call: cranelift_codegen::ir::SigRef,
    /// `(context, operand, out) -> status`: globals, captures, constants.
    operand: cranelift_codegen::ir::SigRef,
    /// `(context, value, out) -> status`: `Negate` and `Not`.
    unary: cranelift_codegen::ir::SigRef,
    index: cranelift_codegen::ir::SigRef,
    list: cranelift_codegen::ir::SigRef,
    /// `(context, site, base, out) -> status`: `native_operation`.
    operation: cranelift_codegen::ir::SigRef,
    /// `(context, arguments) -> status`: compiled code's entry.
    entry: cranelift_codegen::ir::SigRef,
    /// `(context, callee context)`: `native_call_exited`.
    exited: cranelift_codegen::ir::SigRef,
}

impl Runtime {
    fn new(
        builder: &mut FunctionBuilder<'_>,
        pointer: types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Self {
        let mut signature = |params: &[types::Type], returns: &[types::Type]| {
            let mut signature = Signature::new(call_conv);
            signature
                .params
                .extend(params.iter().map(|kind| AbiParam::new(*kind)));
            signature
                .returns
                .extend(returns.iter().map(|kind| AbiParam::new(*kind)));
            builder.import_signature(signature)
        };
        Runtime {
            binary: signature(
                &[pointer, types::I32, pointer, pointer, pointer],
                &[types::I32],
            ),
            attention: signature(&[pointer], &[types::I32]),
            clone: signature(&[pointer, pointer], &[]),
            drop: signature(&[pointer], &[]),
            release: signature(&[pointer, pointer], &[]),
            exit: signature(&[pointer, types::I32, pointer], &[]),
            call: signature(
                &[
                    pointer,
                    types::I32,
                    types::I32,
                    types::I32,
                    pointer,
                    pointer,
                    types::I64,
                    pointer,
                ],
                &[types::I32],
            ),
            entry: signature(&[pointer, pointer], &[types::I32]),
            exited: signature(&[pointer, pointer], &[]),
            operand: signature(&[pointer, types::I32, pointer], &[types::I32]),
            list: signature(&[pointer, types::I32, pointer, pointer], &[]),
            operation: signature(&[pointer, types::I32, pointer, pointer], &[types::I32]),
            unary: signature(&[pointer, pointer, pointer], &[types::I32]),
            index: signature(
                &[
                    pointer,
                    types::I32,
                    types::I32,
                    pointer,
                    pointer,
                    types::I32,
                    pointer,
                ],
                &[types::I32],
            ),
        }
    }
}

struct Lowering<'a> {
    builder: FunctionBuilder<'a>,
    function: &'a Function,
    pointer: types::Type,
    runtime: Runtime,
    /// The native frame: one slot per IR value, then scratch slots for a
    /// `recur`'s arguments, one for a constant operand, and the arguments
    /// of a call.
    frame: StackSlot,
    blocks: Vec<CodeBlock>,
    /// Ends the function: drops every owning slot and returns its status.
    cleanup: CodeBlock,
    context: CodeValue,
    /// The native frame's address.
    base: CodeValue,
    /// Each comparison result's value as an `i8`, where it was computed.
    booleans: Vec<Option<CodeValue>>,
    /// The slot an operation reads a captured local into.
    cell_operand: usize,
    owning: Vec<bool>,
    /// The values live out of each block.
    /// Computed when a value first may move.
    live_out: Option<Vec<Vec<u64>>>,
    exits: Vec<ExitTable>,
    operations: Vec<runtime::Operation>,
    matches: Vec<runtime::MatchSite>,
    /// How the block being lowered chooses its path, when it ends in a
    /// choice.
    choice: Option<Choice>,
    /// Blocks made while another was being filled, filled at the end.
    pending: Vec<Pending>,
    /// The address of the function's call-site records, and the next
    /// module-level call's site.
    sites: usize,
    next_site: usize,
    /// Where a direct call builds its callee's context.
    callee_context: StackSlot,
}

/// A block whose body is lowered after the function's own blocks.
enum Pending {
    /// Takes `Edge`.
    Edge(CodeBlock, Edge),
    /// Leaves to the interpreter through this exit.
    Exit(CodeBlock, u32),
}

impl<'a> Lowering<'a> {
    fn new(
        mut builder: FunctionBuilder<'a>,
        function: &'a Function,
        pointer: types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
        sites: usize,
    ) -> Self {
        let runtime = Runtime::new(&mut builder, pointer, call_conv);
        let call_arguments = function
            .blocks
            .iter()
            .flat_map(|block| &block.insts)
            .filter_map(|inst| match inst.op {
                Op::CallGlobalPooled { count, .. }
                | Op::CallPositional(count)
                | Op::List(count) => Some(count),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        // The last slot holds a captured local an operation reads.
        let cell_operand = function.values.len() + function.arity + 1 + call_arguments;
        let slots = cell_operand + 1;
        let frame = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            u32::try_from(slots * layout::SIZE as usize).expect("frame fits"),
            3,
        ));
        let blocks = function
            .blocks
            .iter()
            .map(|_| builder.create_block())
            .collect();
        let cleanup = builder.create_block();
        builder.append_block_param(cleanup, types::I32);
        let callee_context = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            offsets::CONTEXT_SIZE,
            3,
        ));
        let mut owning = owning_values(function);
        owning.resize(cell_operand, false);
        owning.push(function.cells.iter().any(|cell| *cell));
        Self {
            builder,
            function,
            pointer,
            runtime,
            frame,
            blocks,
            cleanup,
            context: CodeValue::from_u32(0),
            base: CodeValue::from_u32(0),
            booleans: vec![None; function.values.len()],
            cell_operand,
            owning,
            live_out: None,
            exits: Vec::new(),
            operations: Vec::new(),
            matches: Vec::new(),
            choice: None,
            pending: Vec::new(),
            sites,
            next_site: 0,
            callee_context,
        }
    }

    #[allow(clippy::type_complexity)] // The tables `Compiled` takes.
    fn lower(
        mut self,
    ) -> Result<
        (
            Vec<ExitTable>,
            Vec<u32>,
            Vec<runtime::Operation>,
            Vec<runtime::MatchSite>,
        ),
        String,
    > {
        let entry = self.builder.create_block();
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);
        self.context = self.builder.block_params(entry)[0];
        let arguments = self.builder.block_params(entry)[1];
        self.base = self.builder.ins().stack_addr(self.pointer, self.frame, 0);
        let function = self.function;
        // Owning slots and unset locals start `nil`.
        for index in 0..self.owning.len() {
            let unset = matches!(function.values.get(index), Some(ValueDef::UnsetLocal));
            if self.owning[index] || unset {
                let slot = self.slot(index);
                self.store_tag(slot, layout::NIL);
            }
        }
        // The arguments move into the entry block's parameter slots.
        for (index, param) in function.blocks[0].params.iter().enumerate() {
            let offset = i32::try_from(index).expect("arity fits") * layout::SIZE;
            let source = Place::At(self.builder.ins().iadd_imm(arguments, i64::from(offset)));
            let target = self.slot(param.index());
            self.copy_words(source, target);
        }
        let first = self.blocks[0];
        self.builder.ins().jump(first, &[]);

        for (index, block) in function.blocks.iter().enumerate() {
            self.builder.switch_to_block(self.blocks[index]);
            for (position, inst) in block.insts.iter().enumerate() {
                self.lower_inst(inst, (index, position))?;
            }
            self.lower_terminator(block)?;
        }

        while let Some(pending) = self.pending.pop() {
            match pending {
                Pending::Edge(block, edge) => {
                    self.builder.switch_to_block(block);
                    self.lower_edge(&edge);
                }
                Pending::Exit(block, id) => {
                    self.builder.switch_to_block(block);
                    let exit = self.builder.ins().iconst(types::I32, i64::from(id));
                    let base = self.slot(0);
                    let base = self.address(base);
                    self.call(
                        self.runtime.exit,
                        runtime::native_exit as *const () as usize,
                        &[self.context, exit, base],
                    );
                    let status = self.builder.ins().iconst(types::I32, i64::from(EXITED));
                    self.builder.ins().jump(self.cleanup, &[status]);
                }
            }
        }

        self.builder.switch_to_block(self.cleanup);
        let status = self.builder.block_params(self.cleanup)[0];
        self.drop_owning();
        self.builder.ins().return_(&[status]);
        self.builder.seal_all_blocks();
        self.builder.finalize();
        let owning = (0..self.owning.len())
            .filter(|index| self.owning[*index])
            .map(|index| u32::try_from(index).expect("slot index fits"))
            .collect();
        Ok((self.exits, owning, self.operations, self.matches))
    }

    /// Lowers `inst`, instruction `position.1` of block `position.0`.
    fn lower_inst(&mut self, inst: &Inst, position: (usize, usize)) -> Result<(), String> {
        match &inst.op {
            Op::Constant(index) => {
                let slot = self.slot(inst.results[0].index());
                if let Ok(value) = number_constant(self.function, *index) {
                    let value = value.clone();
                    self.store_constant(slot, &value);
                } else {
                    self.lower_operand_op(
                        inst,
                        runtime::native_constant as *const () as usize,
                        *index,
                    );
                }
            }
            Op::GetGlobalPooled(name) => {
                self.lower_operand_op(
                    inst,
                    runtime::native_global as *const () as usize,
                    name.get() as usize,
                );
            }
            Op::GetCapture(index) => {
                self.lower_operand_op(inst, runtime::native_capture as *const () as usize, *index);
            }
            Op::GetLocal(slot) => {
                self.lower_operand_op(inst, runtime::native_get_cell as *const () as usize, *slot);
            }
            Op::MakeClosure { .. } | Op::MakeClosurePooled { .. } => {
                let site = self
                    .function
                    .closures
                    .keys()
                    .position(|ip| *ip == inst.ip)
                    .ok_or("a closure is missing")?;
                self.lower_operand_op(inst, runtime::native_closure as *const () as usize, site);
            }
            Op::SetGlobalPooled(name) => {
                self.lower_write(
                    inst,
                    runtime::native_set_global as *const () as usize,
                    name.get() as usize,
                );
            }
            Op::SetLocal(slot) => {
                self.lower_write(inst, runtime::native_set_cell as *const () as usize, *slot);
            }
            Op::SetCapture(index) => {
                self.lower_write(
                    inst,
                    runtime::native_set_capture as *const () as usize,
                    *index,
                );
            }
            Op::ListAppend
            | Op::ListPrepend
            | Op::GetSlice { .. }
            | Op::Struct(_)
            | Op::StructPooled(_)
            | Op::StructCopy(_)
            | Op::StructCopyPooled(_)
            | Op::CopyGlobalPooled { .. }
            | Op::UpdateCapture { .. }
            | Op::Map(_)
            | Op::InterpolatePooled(_) => self.lower_operation(inst, position)?,
            Op::MatchCasePooled {
                pattern, operands, ..
            } => self.lower_match(inst, *pattern, *operands, false)?,
            Op::TryMatchPooled {
                pattern, operands, ..
            } => self.lower_match(inst, *pattern, *operands, true)?,
            // Native code is entered only with every parameter passed.
            Op::JumpIfProvided { .. } => self.choice = Some(Choice::Target),
            Op::Not => self.lower_not(inst),
            Op::List(_) => self.lower_list(inst),
            Op::Negate => self.lower_negate(inst),
            Op::BitNot => self.lower_bit_not(inst),
            Op::GetIndex
            | Op::GetIndexConstant(_)
            | Op::ReadIndex { .. }
            | Op::ReadIndexConstant { .. } => self.lower_index_op(inst)?,
            Op::Nil => {
                let slot = self.slot(inst.results[0].index());
                self.store_tag(slot, layout::NIL);
            }
            Op::True | Op::False => {
                let slot = self.slot(inst.results[0].index());
                let value = self
                    .builder
                    .ins()
                    .iconst(types::I8, i64::from(matches!(inst.op, Op::True)));
                self.store_bool(slot, value);
            }
            // A `recur` is lowered with its block's terminator, and the
            // others' block ends leave to the interpreter, which runs them.
            Op::RecurPositional(_) | Op::Throw | Op::MatchFailure | Op::NotImplemented => {}
            Op::CallGlobalPooled { name, count } => {
                self.lower_call(inst, Callee::Global(name.get()), *count, position)?;
            }
            Op::CallPositional(count) => self.lower_call(inst, Callee::Value, *count, position)?,
            op => self.lower_operator(inst, op, position)?,
        }
        Ok(())
    }

    /// An index read of any of its forms.
    fn lower_index_op(&mut self, inst: &Inst) -> Result<(), String> {
        match &inst.op {
            Op::GetIndex => {
                self.lower_index(
                    inst,
                    SOURCE_VALUE,
                    0,
                    Some(inst.args[0]),
                    Some(inst.args[1]),
                    None,
                );
            }
            Op::GetIndexConstant(constant) => {
                self.lower_index(
                    inst,
                    SOURCE_VALUE,
                    0,
                    Some(inst.args[0]),
                    None,
                    Some(*constant),
                );
            }
            Op::ReadIndex { source } => {
                let (kind, operand, collection) = self.read_source(inst, source, 1)?;
                self.lower_index(inst, kind, operand, collection, Some(inst.args[0]), None);
            }
            Op::ReadIndexConstant { source, constant } => {
                let (kind, operand, collection) = self.read_source(inst, source, 0)?;
                self.lower_index(inst, kind, operand, collection, None, Some(*constant));
            }
            _ => return Err(format!("instruction {} is not an index read", inst.ip)),
        }
        Ok(())
    }

    /// An arithmetic or comparison instruction, whose left operand may be
    /// a captured local.
    fn lower_operator(
        &mut self,
        inst: &Inst,
        op: &Op,
        position: (usize, usize),
    ) -> Result<(), String> {
        let operator = binary_operator(op)
            .ok_or_else(|| format!("instruction {} is not compiled yet", inst.ip))?;
        let right = match op {
            Op::LocalConstantBinary { constant, .. } | Op::ConstantBinary { constant, .. } => {
                if let Ok(value) = number_constant(self.function, *constant) {
                    Operand::Constant(value.clone())
                } else {
                    value_constant(self.function, *constant)?;
                    Operand::Pooled(*constant)
                }
            }
            _ => Operand::Value(inst.args[1]),
        };
        match op {
            Op::LocalConstantBinary { slot, .. } if self.function.cells[*slot] => {
                // Read into a slot of its own for the operation,
                // and dropped after it.
                let operand = self.cell_operand;
                let place = self.slot(operand);
                let out = self.address(place);
                let slot = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::try_from(*slot).expect("slot fits"));
                self.call(
                    self.runtime.operand,
                    runtime::native_get_cell as *const () as usize,
                    &[self.context, slot, out],
                );
                self.lower_binary(inst, operator, operand, &right, false);
                let out = self.address(place);
                self.call(
                    self.runtime.drop,
                    runtime::native_drop as *const () as usize,
                    &[out],
                );
            }
            _ => {
                // A collection read for the last time is added to or taken
                // from in place, as the interpreter's own `+` and `-` do.
                let moves = matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.moves(inst.args[0], inst, position);
                self.lower_binary(inst, operator, inst.args[0].index(), &right, moves);
            }
        }
        Ok(())
    }

    /// `left operator right` into the instruction's result: inline when
    /// both are integers or both floats and the operator has a fast path
    /// for them, as `numeric_binary` in the VM does; otherwise through the
    /// interpreter's operation, exiting if it fails.
    #[allow(clippy::too_many_lines)] // The integer, float, and slow paths, in order.
    fn lower_binary(
        &mut self,
        inst: &Inst,
        operator: BinaryOperator,
        left: usize,
        right: &Operand,
        moves_left: bool,
    ) {
        use BinaryOperator as B;
        let result = self.slot(inst.results[0].index());
        let left_slot = self.slot(left);
        let left_tag = self.load_tag(left_slot);
        let done = self.builder.create_block();
        let slow = self.builder.create_block();
        // A comparison also hands its result to `done` as an `i8`, so a
        // branch on it need not read it back from the slot.
        let boolean = compares(operator);
        if boolean {
            self.builder.append_block_param(done, types::I8);
        }

        // A constant right operand that is not a number never takes a path,
        // and a float one never takes the integer path.
        let (int_constant, number_constant) = match right {
            Operand::Value(_) => (true, true),
            Operand::Pooled(_) => (false, false),
            Operand::Constant(constant) => (
                matches!(constant, Value::Int(_)),
                matches!(constant, Value::Int(_) | Value::Float(_)),
            ),
        };
        let int_fast = int_constant
            && matches!(
                operator,
                B::Add
                    | B::AddNum
                    | B::Subtract
                    | B::SubtractNum
                    | B::Multiply
                    | B::MultiplyNum
                    | B::Divide
                    | B::DivideNum
                    | B::Equal
                    | B::Greater
                    | B::GreaterNum
                    | B::Less
                    | B::LessNum
                    | B::BitAnd
                    | B::BitOr
                    | B::BitXor
            );
        let float_fast = number_constant
            && matches!(
                operator,
                B::Add
                    | B::AddNum
                    | B::Subtract
                    | B::SubtractNum
                    | B::Multiply
                    | B::MultiplyNum
                    | B::Divide
                    | B::DivideNum
                    | B::Equal
                    | B::Greater
                    | B::GreaterNum
                    | B::Less
                    | B::LessNum
            );
        // Equality of an integer and a float is not numeric, so only
        // two floats take the float path for it.
        let mixed = operator != B::Equal;
        let float_fast =
            float_fast && (mixed || !matches!(right, Operand::Constant(Value::Int(_))));

        // Reached when the operands are not both integers, and by an
        // integer division with a remainder, which gives a float.
        let after_int = self.builder.create_block();
        if int_fast {
            let left_is = self
                .builder
                .ins()
                .icmp_imm(IntCC::Equal, left_tag, layout::INT);
            let is_int = match right {
                Operand::Value(value) => {
                    let right_slot = self.slot(value.index());
                    let right_tag = self.load_tag(right_slot);
                    let right_is =
                        self.builder
                            .ins()
                            .icmp_imm(IntCC::Equal, right_tag, layout::INT);
                    self.builder.ins().band(left_is, right_is)
                }
                Operand::Constant(_) | Operand::Pooled(_) => left_is,
            };
            let int_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(is_int, int_block, &[], after_int, &[]);
            self.builder.switch_to_block(int_block);
            let a = self.load_payload(left_slot, types::I64);
            let b = self.operand_payload(right, types::I64);
            match operator {
                B::Add
                | B::AddNum
                | B::Subtract
                | B::SubtractNum
                | B::Multiply
                | B::MultiplyNum => {
                    let (sum, overflow) = match operator {
                        B::Add | B::AddNum => self.builder.ins().sadd_overflow(a, b),
                        B::Subtract | B::SubtractNum => self.builder.ins().ssub_overflow(a, b),
                        _ => self.builder.ins().smul_overflow(a, b),
                    };
                    let store = self.builder.create_block();
                    self.builder.ins().brif(overflow, slow, &[], store, &[]);
                    self.builder.switch_to_block(store);
                    self.store_int(result, sum);
                    self.builder.ins().jump(done, &[]);
                }
                B::Divide | B::DivideNum => {
                    // A zero divisor and `i64::MIN / -1` are the
                    // interpreter's errors to report.
                    let zero = self.builder.ins().icmp_imm(IntCC::Equal, b, 0);
                    let minus_one = self.builder.ins().icmp_imm(IntCC::Equal, b, -1);
                    let min = self.builder.ins().icmp_imm(IntCC::Equal, a, i64::MIN);
                    let overflow = self.builder.ins().band(minus_one, min);
                    let fault = self.builder.ins().bor(zero, overflow);
                    let divide = self.builder.create_block();
                    self.builder.ins().brif(fault, slow, &[], divide, &[]);
                    self.builder.switch_to_block(divide);
                    let remainder = self.builder.ins().srem(a, b);
                    let exact = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(remainder, after_int, &[], exact, &[]);
                    self.builder.switch_to_block(exact);
                    let quotient = self.builder.ins().sdiv(a, b);
                    self.store_int(result, quotient);
                    self.builder.ins().jump(done, &[]);
                }
                B::BitAnd | B::BitOr | B::BitXor => {
                    let value = match operator {
                        B::BitAnd => self.builder.ins().band(a, b),
                        B::BitOr => self.builder.ins().bor(a, b),
                        _ => self.builder.ins().bxor(a, b),
                    };
                    self.store_int(result, value);
                    self.builder.ins().jump(done, &[]);
                }
                _ => {
                    let condition = match operator {
                        B::Equal => IntCC::Equal,
                        B::Greater | B::GreaterNum => IntCC::SignedGreaterThan,
                        _ => IntCC::SignedLessThan,
                    };
                    let value = self.builder.ins().icmp(condition, a, b);
                    self.store_bool(result, value);
                    self.builder.ins().jump(done, &[value]);
                }
            }
        } else {
            self.builder.ins().jump(after_int, &[]);
        }

        // Two numbers, at least one a float or a quotient with a
        // remainder: both as floats, as `numbers` in the VM converts them.
        self.builder.switch_to_block(after_int);
        if float_fast {
            let a = self.float_of(left_slot, mixed, slow);
            let b = match right {
                Operand::Value(value) => {
                    let right_slot = self.slot(value.index());
                    self.float_of(right_slot, mixed, slow)
                }
                #[allow(clippy::cast_precision_loss)] // As `numbers` converts.
                Operand::Constant(Value::Int(value)) if mixed => {
                    self.builder.ins().f64const(*value as f64)
                }
                Operand::Constant(Value::Float(value)) => self.builder.ins().f64const(*value),
                Operand::Constant(_) | Operand::Pooled(_) => {
                    unreachable!("only number constants reach a fast path")
                }
            };
            match operator {
                B::Add
                | B::AddNum
                | B::Subtract
                | B::SubtractNum
                | B::Multiply
                | B::MultiplyNum
                | B::Divide
                | B::DivideNum => {
                    if matches!(operator, B::Divide | B::DivideNum) {
                        // Division by zero is the interpreter's error.
                        let zero = self.builder.ins().f64const(0.0);
                        let is_zero = self.builder.ins().fcmp(FloatCC::Equal, b, zero);
                        let divide = self.builder.create_block();
                        self.builder.ins().brif(is_zero, slow, &[], divide, &[]);
                        self.builder.switch_to_block(divide);
                    }
                    let value = match operator {
                        B::Add | B::AddNum => self.builder.ins().fadd(a, b),
                        B::Subtract | B::SubtractNum => self.builder.ins().fsub(a, b),
                        B::Multiply | B::MultiplyNum => self.builder.ins().fmul(a, b),
                        _ => self.builder.ins().fdiv(a, b),
                    };
                    // Finite exactly when `value - value` is zero.
                    let difference = self.builder.ins().fsub(value, value);
                    let zero = self.builder.ins().f64const(0.0);
                    let finite = self.builder.ins().fcmp(FloatCC::Equal, difference, zero);
                    let store = self.builder.create_block();
                    self.builder.ins().brif(finite, store, &[], slow, &[]);
                    self.builder.switch_to_block(store);
                    self.store_float(result, value);
                    self.builder.ins().jump(done, &[]);
                }
                _ => {
                    let condition = match operator {
                        B::Equal => FloatCC::Equal,
                        B::Greater | B::GreaterNum => FloatCC::GreaterThan,
                        _ => FloatCC::LessThan,
                    };
                    let value = self.builder.ins().fcmp(condition, a, b);
                    self.store_bool(result, value);
                    self.builder.ins().jump(done, &[value]);
                }
            }
        } else {
            self.builder.ins().jump(slow, &[]);
        }

        // The interpreter's own operation, or an exit if it fails.
        self.builder.switch_to_block(slow);
        let scratch = self.slot(self.function.values.len() + self.function.arity);
        let right_address = match right {
            Operand::Value(value) => self.slot(value.index()),
            Operand::Constant(constant) => {
                self.store_constant(scratch, constant);
                scratch
            }
            Operand::Pooled(index) => {
                // Cloned in for the operation and dropped after it.
                self.store_tag(scratch, layout::NIL);
                let index = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::try_from(*index).expect("index fits"));
                let scratch_address = self.address(scratch);
                self.call(
                    self.runtime.operand,
                    runtime::native_constant as *const () as usize,
                    &[self.context, index, scratch_address],
                );
                scratch
            }
        };
        let index = BINARY_OPERATORS
            .iter()
            .position(|known| *known == operator)
            .expect("every operator has an index");
        let operator_code = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(index).expect("index fits"));
        let left_slot = self.address(left_slot);
        let right_address = self.address(right_address);
        let result_address = self.address(result);
        let status = self.call(
            self.runtime.binary,
            if moves_left {
                runtime::native_binary_moving as *const () as usize
            } else {
                runtime::native_binary as *const () as usize
            },
            &[
                self.context,
                operator_code,
                left_slot,
                right_address,
                result_address,
            ],
        )[0];
        if matches!(right, Operand::Pooled(_)) {
            let scratch_address = self.address(scratch);
            self.call(
                self.runtime.drop,
                runtime::native_drop as *const () as usize,
                &[scratch_address],
            );
        }
        let exit = self.exit_block(inst);
        let result = self.slot(inst.results[0].index());
        let done_arguments = if boolean {
            // Read only when the operation succeeded.
            vec![self.load(types::I8, result, layout::PAYLOAD)]
        } else {
            Vec::new()
        };
        self.builder
            .ins()
            .brif(status, exit, &[], done, &done_arguments);

        self.builder.switch_to_block(done);
        if boolean {
            self.booleans[inst.results[0].index()] = Some(self.builder.block_params(done)[0]);
        }
    }

    /// A list of the instruction's arguments, cloned into the call
    /// scratch slots for the runtime function to take.
    fn lower_list(&mut self, inst: &Inst) {
        let base = self.function.values.len() + self.function.arity + 1;
        for (index, argument) in inst.args.iter().enumerate() {
            let source = self.slot(argument.index());
            let target = self.slot(base + index);
            self.clone_into(source, target);
        }
        let values = self.slot(base);
        let values = self.address(values);
        let count = self.builder.ins().iconst(
            types::I32,
            i64::try_from(inst.args.len()).expect("count fits"),
        );
        let result = self.slot(inst.results[0].index());
        let out = self.address(result);
        self.call(
            self.runtime.list,
            runtime::native_list as *const () as usize,
            &[self.context, count, values, out],
        );
    }

    /// Continues when `status` is zero, and leaves to the interpreter
    /// before `inst` otherwise.
    fn exit_unless_ok(&mut self, inst: &Inst, status: CodeValue) {
        let exit = self.exit_block(inst);
        let next = self.builder.create_block();
        self.builder.ins().brif(status, exit, &[], next, &[]);
        self.builder.switch_to_block(next);
    }

    /// An instruction a runtime function computes from the running
    /// function and one operand, such as a global's name.
    fn lower_operand_op(&mut self, inst: &Inst, function: usize, operand: usize) {
        let result = self.slot(inst.results[0].index());
        let out = self.address(result);
        let operand = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(operand).expect("operand fits"));
        let status = self.call(
            self.runtime.operand,
            function,
            &[self.context, operand, out],
        )[0];
        self.exit_unless_ok(inst, status);
    }

    /// Whether `inst`, instruction `index` of block `block`, may move
    /// `value`, its operand: it reads it once, and nothing after it does.
    fn moves(&mut self, value: ValueId, inst: &Inst, (block, index): (usize, usize)) -> bool {
        if inst.args.iter().filter(|other| **other == value).count() != 1 {
            return false;
        }
        let function = self.function;
        let live_out = self.live_out.get_or_insert_with(|| live_out(function));
        let code = &function.blocks[block];
        let live = live_out[block][value.index() / 64] & (1 << (value.index() % 64)) != 0;
        !(live
            || code.insts[index + 1..]
                .iter()
                .flat_map(inst_uses)
                .any(|other| *other == value)
            || terminator_uses(&code.terminator).contains(&value))
    }

    /// An instruction the VM's `native_operation` runs on its operands,
    /// moving each it reads for the last time: instruction `index` of
    /// block `block`.
    fn lower_operation(
        &mut self,
        inst: &Inst,
        (block, index): (usize, usize),
    ) -> Result<(), String> {
        let operands = inst
            .args
            .iter()
            .map(|value| u32::try_from(value.index()).expect("slot index fits"))
            .collect();
        let moves = inst
            .args
            .iter()
            .map(|value| self.moves(*value, inst, (block, index)))
            .collect();
        let site = self.operations.len();
        self.operations.push(runtime::Operation {
            op: NativeOperation::of(&inst.op).ok_or("not an operation")?,
            operands,
            moves,
        });
        let site = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(site).expect("site fits"));
        let base = self.slot(0);
        let base = self.address(base);
        let result = self.slot(inst.results[0].index());
        let out = self.address(result);
        let status = self.call(
            self.runtime.operation,
            runtime::native_operation as *const () as usize,
            &[self.context, site, base, out],
        )[0];
        self.exit_unless_ok(inst, status);
        Ok(())
    }

    /// A `MatchCasePooled`: the VM's `native_match` writes the value and
    /// its bindings to the results, which follow one another, and the
    /// block's end takes the fall-through when it matched.
    fn lower_match(
        &mut self,
        inst: &Inst,
        pattern: slug_vm::MatchPatternId,
        operands: usize,
        tries: bool,
    ) -> Result<(), String> {
        let consecutive = inst
            .results
            .windows(2)
            .all(|pair| pair[1].index() == pair[0].index() + 1);
        if inst.results.is_empty() || !consecutive || inst.args.len() != operands + 1 {
            return Err(format!("instruction {} is a malformed match", inst.ip));
        }
        let site = self.matches.len();
        self.matches.push(runtime::MatchSite {
            pattern,
            operands: inst
                .args
                .iter()
                .map(|value| u32::try_from(value.index()).expect("slot index fits"))
                .collect(),
            results: inst.results.len(),
            tries,
        });
        let site = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(site).expect("site fits"));
        let base = self.slot(0);
        let base = self.address(base);
        let out = self.slot(inst.results[0].index());
        let out = self.address(out);
        let status = self.call(
            self.runtime.operation,
            runtime::native_match as *const () as usize,
            &[self.context, site, base, out],
        )[0];
        let settled = self.builder.ins().icmp_imm(
            IntCC::UnsignedLessThanOrEqual,
            status,
            i64::from(runtime::NOT_MATCHED),
        );
        let exit = self.exit_block(inst);
        let next = self.builder.create_block();
        self.builder.ins().brif(settled, next, &[], exit, &[]);
        self.builder.switch_to_block(next);
        if !tries {
            let matched =
                self.builder
                    .ins()
                    .icmp_imm(IntCC::Equal, status, i64::from(runtime::MATCHED));
            self.choice = Some(Choice::Fallthrough(matched));
        }
        Ok(())
    }

    /// An instruction that writes the value it pops to the place `operand`
    /// names, through a runtime function that clones it there, leaving to
    /// the interpreter if that cannot.
    fn lower_write(&mut self, inst: &Inst, function: usize, operand: usize) {
        let operand = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(operand).expect("operand fits"));
        let value = self.slot(inst.args[0].index());
        let value = self.address(value);
        let status = self.call(
            self.runtime.operand,
            function,
            &[self.context, operand, value],
        )[0];
        self.exit_unless_ok(inst, status);
    }

    /// Where an index read's collection is: a value native code holds (a
    /// local, after the `pops` popped arguments), a capture, or a global.
    fn read_source(
        &self,
        inst: &Inst,
        source: &ReadSource,
        pops: usize,
    ) -> Result<(u32, u32, Option<ValueId>), String> {
        let operand = |value: usize| u32::try_from(value).map_err(|_| "operand too large");
        Ok(match source {
            ReadSource::Local(slot) if self.function.cells[*slot] => {
                (SOURCE_CELL, operand(*slot)?, None)
            }
            ReadSource::Local(_) => {
                (
                    SOURCE_VALUE,
                    0,
                    Some(*inst.args.get(pops).ok_or_else(|| {
                        format!("instruction {} reads a captured local", inst.ip)
                    })?),
                )
            }
            ReadSource::Capture(index) => (SOURCE_CAPTURE, operand(*index)?, None),
            ReadSource::GlobalPooled(name) => (SOURCE_GLOBAL, name.get(), None),
            ReadSource::Global(_) => {
                return Err(format!("instruction {} reads a global by name", inst.ip));
            }
        })
    }

    /// An index read through the interpreter's own code: the collection is
    /// `collection` or `source`'s `operand`, the key `key` or constant
    /// `constant`.
    fn lower_index(
        &mut self,
        inst: &Inst,
        source: u32,
        operand: u32,
        collection: Option<ValueId>,
        key: Option<ValueId>,
        constant: Option<usize>,
    ) {
        let address_of = |lowering: &mut Self, value: Option<ValueId>| match value {
            Some(value) => {
                let slot = lowering.slot(value.index());
                lowering.address(slot)
            }
            None => lowering.builder.ins().iconst(lowering.pointer, 0),
        };
        let collection = address_of(self, collection);
        let key = address_of(self, key);
        let source = self.builder.ins().iconst(types::I32, i64::from(source));
        let operand = self.builder.ins().iconst(types::I32, i64::from(operand));
        let constant = self.builder.ins().iconst(
            types::I32,
            constant.map_or(i64::from(NO_CONSTANT), |index| {
                i64::try_from(index).expect("constant index fits")
            }),
        );
        let result = self.slot(inst.results[0].index());
        let out = self.address(result);
        let status = self.call(
            self.runtime.index,
            runtime::native_index as *const () as usize,
            &[
                self.context,
                source,
                operand,
                collection,
                key,
                constant,
                out,
            ],
        )[0];
        self.exit_unless_ok(inst, status);
    }

    /// `not value`: inline for `nil`, booleans, and numbers, and through
    /// the interpreter's code for anything else, which may be a module
    /// binding to resolve.
    fn lower_not(&mut self, inst: &Inst) {
        let value = inst.args[0];
        let result_index = inst.results[0].index();
        let result = self.slot(result_index);
        if let Some(boolean) = self.booleans[value.index()] {
            let not = self.builder.ins().icmp_imm(IntCC::Equal, boolean, 0);
            self.store_bool(result, not);
            self.booleans[result_index] = Some(not);
            return;
        }
        let operand = self.slot(value.index());
        let tag = self.load_tag(operand);
        let inline = self
            .builder
            .ins()
            .icmp_imm(IntCC::SignedLessThan, tag, layout::FIRST_COUNTED);
        let fast = self.builder.create_block();
        let slow = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.append_block_param(done, types::I8);
        self.builder.ins().brif(inline, fast, &[], slow, &[]);

        self.builder.switch_to_block(fast);
        let truthy = self.truthy(value);
        let not = self.builder.ins().icmp_imm(IntCC::Equal, truthy, 0);
        self.store_bool(result, not);
        self.builder.ins().jump(done, &[not]);

        self.builder.switch_to_block(slow);
        let (value_address, out) = (self.address(operand), self.address(result));
        let status = self.call(
            self.runtime.unary,
            runtime::native_not as *const () as usize,
            &[self.context, value_address, out],
        )[0];
        self.exit_unless_ok(inst, status);
        let byte = self.load(types::I8, result, layout::PAYLOAD);
        self.builder.ins().jump(done, &[byte]);

        self.builder.switch_to_block(done);
        self.booleans[result_index] = Some(self.builder.block_params(done)[0]);
    }

    /// `~value`: inline for an integer, and through the interpreter's code
    /// for anything else (bytes, or an error it reports).
    fn lower_bit_not(&mut self, inst: &Inst) {
        let operand = self.slot(inst.args[0].index());
        let result = self.slot(inst.results[0].index());
        let tag = self.load_tag(operand);
        let int_block = self.builder.create_block();
        let slow = self.builder.create_block();
        let done = self.builder.create_block();
        let is_int = self.builder.ins().icmp_imm(IntCC::Equal, tag, layout::INT);
        self.builder.ins().brif(is_int, int_block, &[], slow, &[]);

        self.builder.switch_to_block(int_block);
        let integer = self.load_payload(operand, types::I64);
        let inverted = self.builder.ins().bnot(integer);
        self.store_int(result, inverted);
        self.builder.ins().jump(done, &[]);

        self.builder.switch_to_block(slow);
        let (value_address, out) = (self.address(operand), self.address(result));
        let status = self.call(
            self.runtime.unary,
            runtime::native_bit_not as *const () as usize,
            &[self.context, value_address, out],
        )[0];
        self.exit_unless_ok(inst, status);
        self.builder.ins().jump(done, &[]);

        self.builder.switch_to_block(done);
    }

    /// `-value`: inline for an integer other than the smallest and for a
    /// float, and through the interpreter's code for anything else.
    fn lower_negate(&mut self, inst: &Inst) {
        let operand = self.slot(inst.args[0].index());
        let result = self.slot(inst.results[0].index());
        let tag = self.load_tag(operand);
        let int_block = self.builder.create_block();
        let not_int = self.builder.create_block();
        let float_block = self.builder.create_block();
        let slow = self.builder.create_block();
        let done = self.builder.create_block();
        let is_int = self.builder.ins().icmp_imm(IntCC::Equal, tag, layout::INT);
        self.builder
            .ins()
            .brif(is_int, int_block, &[], not_int, &[]);

        self.builder.switch_to_block(int_block);
        let integer = self.load_payload(operand, types::I64);
        // `-i64::MIN` overflows, which the interpreter reports.
        let smallest = self.builder.ins().icmp_imm(IntCC::Equal, integer, i64::MIN);
        let negate = self.builder.create_block();
        self.builder.ins().brif(smallest, slow, &[], negate, &[]);
        self.builder.switch_to_block(negate);
        let negated = self.builder.ins().ineg(integer);
        self.store_int(result, negated);
        self.builder.ins().jump(done, &[]);

        self.builder.switch_to_block(not_int);
        let is_float = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, tag, layout::FLOAT);
        self.builder
            .ins()
            .brif(is_float, float_block, &[], slow, &[]);
        self.builder.switch_to_block(float_block);
        let float = self.load_payload(operand, types::F64);
        let negated = self.builder.ins().fneg(float);
        self.store_float(result, negated);
        self.builder.ins().jump(done, &[]);

        self.builder.switch_to_block(slow);
        let (value_address, out) = (self.address(operand), self.address(result));
        let status = self.call(
            self.runtime.unary,
            runtime::native_negate as *const () as usize,
            &[self.context, value_address, out],
        )[0];
        self.exit_unless_ok(inst, status);
        self.builder.ins().jump(done, &[]);

        self.builder.switch_to_block(done);
    }

    /// A call of module-level name `name`: the callee's native code with
    /// clones of the arguments, or an exit before the call when there is
    /// none, or after it when the callee exited.
    /// A call, instruction `index` of block `block`. An argument the call
    /// reads for the last time is moved to it, not copied, so a collection
    /// the callee updates may be updated in place, as the interpreter's own
    /// calls allow; if the call is declined, it is moved back.
    fn lower_call(
        &mut self,
        inst: &Inst,
        callee: Callee,
        count: usize,
        position: (usize, usize),
    ) -> Result<(), String> {
        // A called value is below its arguments.
        let below = usize::from(matches!(callee, Callee::Value));
        if inst.args.len() != count + below || inst.results.len() != 1 {
            return Err(format!("instruction {} is a malformed call", inst.ip));
        }
        let (name, site, callee) = match callee {
            Callee::Global(name) => {
                let site = self.next_site;
                self.next_site += 1;
                (name, Some(site), self.builder.ins().iconst(self.pointer, 0))
            }
            Callee::Value => {
                let slot = self.slot(inst.args[0].index());
                (0, None, self.address(slot))
            }
        };
        let base = self.function.values.len() + self.function.arity + 1;
        let mut moved = 0_u64;
        for (index, argument) in inst.args[below..].iter().enumerate() {
            let source = self.slot(argument.index());
            let target = self.slot(base + index);
            if index < 64 && self.owning[argument.index()] && self.moves(*argument, inst, position)
            {
                self.copy_words(source, target);
                self.store_tag(source, layout::NIL);
                moved |= 1 << index;
            } else {
                self.clone_into(source, target);
            }
        }
        let arguments = self.slot(base);
        let arguments = self.address(arguments);
        let before = self.exit_block(inst);
        let after = self.exit_after_call(inst);
        let done = self.builder.create_block();
        if let Some(site) = site {
            let slow = self.builder.create_block();
            self.direct_call(site, (arguments, count), inst, (slow, done, after));
            self.builder.switch_to_block(slow);
        }
        #[expect(clippy::cast_possible_wrap, reason = "a bit mask")]
        let moved_mask = self.builder.ins().iconst(types::I64, moved as i64);
        let site = self.builder.ins().iconst(
            types::I32,
            i64::from(site.map_or(NO_SITE, |site| u32::try_from(site).expect("site fits"))),
        );
        let name = self.builder.ins().iconst(types::I32, i64::from(name));
        let count = self
            .builder
            .ins()
            .iconst(types::I32, i64::try_from(count).expect("count fits"));
        let result = self.slot(inst.results[0].index());
        let result = self.address(result);
        let status = self.call(
            self.runtime.call,
            runtime::native_call as *const () as usize,
            &[
                self.context,
                name,
                count,
                site,
                callee,
                arguments,
                moved_mask,
                result,
            ],
        )[0];
        let not_returned = self.builder.create_block();
        let returned = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, status, i64::from(CALL_RETURNED));
        self.builder
            .ins()
            .brif(returned, done, &[], not_returned, &[]);
        self.builder.switch_to_block(not_returned);
        let declined = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, status, i64::from(CALL_DECLINED));
        if moved == 0 {
            self.builder.ins().brif(declined, before, &[], after, &[]);
        } else {
            // Declined: the moved arguments go back where they were.
            let restore = self.builder.create_block();
            self.builder.ins().brif(declined, restore, &[], after, &[]);
            self.builder.switch_to_block(restore);
            for (index, argument) in inst.args[below..].iter().enumerate() {
                if moved & (1 << index) != 0 {
                    let source = self.slot(base + index);
                    let target = self.slot(argument.index());
                    self.copy_words(source, target);
                }
            }
            self.builder.ins().jump(before, &[]);
        }
        self.builder.switch_to_block(done);
        Ok(())
    }

    /// Call site `site`'s record, continuing past checks that it still
    /// holds for the `count` arguments at `arguments` (see
    /// [`Self::direct_call`]), or at `slow`.
    fn direct_call_record(
        &mut self,
        site: usize,
        (arguments, count): (CodeValue, usize),
        slow: CodeBlock,
    ) -> CodeValue {
        let pointer = self.pointer;
        let flags = MemFlags::trusted();
        let address = self.sites + site * size_of::<usize>();
        let address = self
            .builder
            .ins()
            .iconst(pointer, i64::try_from(address).expect("address fits"));
        let record = self.builder.ins().atomic_load(pointer, flags, address);
        let check = |lowering: &mut Self, failed: CodeValue| {
            let next = lowering.builder.create_block();
            lowering.builder.ins().brif(failed, slow, &[], next, &[]);
            lowering.builder.switch_to_block(next);
        };
        let none = self.builder.ins().icmp_imm(IntCC::Equal, record, 0);
        check(self, none);
        let globals =
            self.builder
                .ins()
                .load(pointer, flags, self.context, offsets::CONTEXT_GLOBALS);
        let recorded = self
            .builder
            .ins()
            .load(pointer, flags, record, offsets::RECORD_GLOBALS);
        let other = self.builder.ins().icmp(IntCC::NotEqual, globals, recorded);
        check(self, other);
        let version = self.builder.ins().atomic_load(types::I64, flags, globals);
        let recorded = self
            .builder
            .ins()
            .load(types::I64, flags, record, offsets::RECORD_VERSION);
        let changed = self.builder.ins().icmp(IntCC::NotEqual, version, recorded);
        check(self, changed);
        // The stack used since the entry from the interpreter: the root's
        // address is above this frame's.
        let root = self
            .builder
            .ins()
            .load(pointer, flags, self.context, offsets::CONTEXT_ROOT);
        let top = self
            .builder
            .ins()
            .load(pointer, flags, root, offsets::ROOT_BASE);
        let used = self.builder.ins().isub(top, self.base);
        let full = self.builder.ins().icmp_imm(
            IntCC::UnsignedGreaterThanOrEqual,
            used,
            i64::try_from(STACK_BUDGET).expect("budget fits"),
        );
        check(self, full);
        let attention =
            self.builder
                .ins()
                .load(pointer, flags, self.context, offsets::CONTEXT_ATTENTION);
        let attention = self.builder.ins().uload8(types::I32, flags, attention, 0);
        let attend = self.builder.ins().icmp_imm(IntCC::NotEqual, attention, 0);
        check(self, attend);
        let binding = runtime::binding_tag();
        for index in 0..count {
            let offset = i32::try_from(index).expect("count fits") * layout::SIZE;
            let tag = self
                .builder
                .ins()
                .load(types::I64, flags, arguments, offset);
            let bound = self.builder.ins().icmp_imm(IntCC::Equal, tag, binding);
            check(self, bound);
        }
        record
    }

    /// Calls what call site `site` recorded, directly, with the `count`
    /// arguments at `arguments`, when its record still holds: the caller's
    /// module table has the stamp it had, the native stack has room, the
    /// thread needs no attention, and no argument is a module binding.
    /// Otherwise continues at `slow`, which makes the call through the
    /// runtime. Writes the result to `inst`'s and continues at `done`, or,
    /// when the callee exits, records it and continues at `after`.
    fn direct_call(
        &mut self,
        site: usize,
        (arguments, count): (CodeValue, usize),
        inst: &Inst,
        (slow, done, after): (CodeBlock, CodeBlock, CodeBlock),
    ) {
        let pointer = self.pointer;
        let flags = MemFlags::trusted();
        let record = self.direct_call_record(site, (arguments, count), slow);
        // The callee's context.
        let callee = self
            .builder
            .ins()
            .stack_addr(pointer, self.callee_context, 0);
        let copy = |lowering: &mut Self, from: CodeValue, at: i32, to: i32| {
            let value = lowering.builder.ins().load(pointer, flags, from, at);
            lowering.builder.ins().store(flags, value, callee, to);
        };
        copy(self, self.context, offsets::CONTEXT_VM, offsets::CONTEXT_VM);
        copy(
            self,
            record,
            offsets::RECORD_COMPILED,
            offsets::CONTEXT_COMPILED,
        );
        copy(
            self,
            self.context,
            offsets::CONTEXT_ROOT,
            offsets::CONTEXT_ROOT,
        );
        copy(
            self,
            record,
            offsets::RECORD_CALLEE_GLOBALS,
            offsets::CONTEXT_GLOBALS,
        );
        copy(
            self,
            self.context,
            offsets::CONTEXT_ATTENTION,
            offsets::CONTEXT_ATTENTION,
        );
        let closure = self
            .builder
            .ins()
            .iadd_imm(record, i64::from(offsets::RECORD_CLOSURE));
        self.builder
            .ins()
            .store(flags, closure, callee, offsets::CONTEXT_CLOSURE);
        let null = self.builder.ins().iconst(pointer, 0);
        self.builder
            .ins()
            .store(flags, null, callee, offsets::CONTEXT_CELLS);
        self.builder
            .ins()
            .store(flags, null, callee, offsets::CONTEXT_EXIT);
        let entry = self
            .builder
            .ins()
            .load(pointer, flags, record, offsets::RECORD_ENTRY);
        let call =
            self.builder
                .ins()
                .call_indirect(self.runtime.entry, entry, &[callee, arguments]);
        let status = self.builder.inst_results(call)[0];
        let returned = self.builder.create_block();
        let exited = self.builder.create_block();
        let is_returned = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, status, i64::from(RETURNED));
        self.builder
            .ins()
            .brif(is_returned, returned, &[], exited, &[]);
        self.builder.switch_to_block(returned);
        let value = Place::At(
            self.builder
                .ins()
                .iadd_imm(callee, i64::from(offsets::CONTEXT_RET)),
        );
        let result = self.slot(inst.results[0].index());
        self.copy_words(value, result);
        self.builder.ins().jump(done, &[]);
        self.builder.switch_to_block(exited);
        self.call(
            self.runtime.exited,
            runtime::native_call_exited as *const () as usize,
            &[self.context, callee],
        );
        self.builder.ins().jump(after, &[]);
    }

    fn lower_terminator(&mut self, block: &Block) -> Result<(), String> {
        match &block.terminator {
            Terminator::Jump(edge) => self.lower_edge(edge),
            Terminator::Branch {
                cond,
                then,
                otherwise,
            } => {
                let truthy = self.truthy(*cond);
                let then_block = self.edge_block(then);
                let otherwise_block = self.edge_block(otherwise);
                self.builder
                    .ins()
                    .brif(truthy, then_block, &[], otherwise_block, &[]);
            }
            Terminator::Return(value) => {
                let source = self.slot(value.index());
                let target = Place::At(
                    self.builder
                        .ins()
                        .iadd_imm(self.context, i64::from(offsets::CONTEXT_RET)),
                );
                self.copy_words(source, target);
                if self.owning[value.index()] {
                    // Moved out: the cleanup must not drop it.
                    self.store_tag(source, layout::NIL);
                }
                let status = self.builder.ins().iconst(types::I32, i64::from(RETURNED));
                self.builder.ins().jump(self.cleanup, &[status]);
            }
            Terminator::Recur(arguments) => {
                let inst = block
                    .insts
                    .last()
                    .ok_or("a recur block has no recur instruction")?;
                let status = self.call(
                    self.runtime.attention,
                    runtime::native_attention as *const () as usize,
                    &[self.context],
                )[0];
                let exit = self.exit_block(inst);
                let restart = self.builder.create_block();
                self.builder.ins().brif(status, exit, &[], restart, &[]);
                self.builder.switch_to_block(restart);
                // Clone the arguments aside, drop everything the iteration
                // owned, then move them into the parameters.
                let scratch_base = self.function.values.len();
                for (index, argument) in arguments.iter().enumerate() {
                    let source = self.slot(argument.index());
                    let target = self.slot(scratch_base + index);
                    self.clone_into(source, target);
                }
                self.drop_owning();
                if self.function.cells.iter().any(|cell| *cell) {
                    let arguments = self.slot(scratch_base);
                    let arguments = self.address(arguments);
                    self.call(
                        self.runtime.release,
                        runtime::native_recur_cells as *const () as usize,
                        &[self.context, arguments],
                    );
                }
                let params = self.function.blocks[0].params.clone();
                for (index, param) in params.iter().enumerate() {
                    let source = self.slot(scratch_base + index);
                    let target = self.slot(param.index());
                    self.copy_words(source, target);
                }
                let first = self.blocks[0];
                self.builder.ins().jump(first, &[]);
            }
            Terminator::Choose {
                fallthrough,
                target,
            } => match self.choice.take() {
                Some(Choice::Target) => self.lower_edge(target),
                Some(Choice::Fallthrough(condition)) => {
                    let fallthrough = self.edge_block(fallthrough);
                    let target = self.edge_block(target);
                    self.builder
                        .ins()
                        .brif(condition, fallthrough, &[], target, &[]);
                }
                None => return Err("a choice was not lowered".into()),
            },
            Terminator::Stop => {
                let inst = block
                    .insts
                    .last()
                    .ok_or("a block stops with no instruction")?;
                let exit = self.exit_block(inst);
                self.builder.ins().jump(exit, &[]);
            }
        }
        Ok(())
    }

    /// Jumps along `edge`, cloning its arguments into the target's
    /// parameter slots.
    fn lower_edge(&mut self, edge: &Edge) {
        let function = self.function;
        let params = &function.blocks[edge.block.index()].params;
        for (argument, param) in edge.args.iter().zip(params) {
            let source = self.slot(argument.index());
            let target = self.slot(param.index());
            self.clone_into(source, target);
        }
        let target = self.blocks[edge.block.index()];
        self.builder.ins().jump(target, &[]);
    }

    /// A block that takes `edge`, or its target itself when there is
    /// nothing to pass.
    fn edge_block(&mut self, edge: &Edge) -> CodeBlock {
        if edge.args.is_empty() {
            return self.blocks[edge.block.index()];
        }
        let block = self.builder.create_block();
        self.pending.push(Pending::Edge(block, edge.clone()));
        block
    }

    /// A block that leaves to the interpreter before `inst`.
    fn exit_block(&mut self, inst: &Inst) -> CodeBlock {
        self.exit_with(inst.ip, &inst.snapshot, 0)
    }

    /// A block that leaves to the interpreter during call `inst`, whose
    /// callee left too: past the call, its arguments gone, and the
    /// callee's frame above.
    fn exit_after_call(&mut self, inst: &Inst) -> CodeBlock {
        self.exit_with(inst.ip + 1, &inst.snapshot, inst.args.len())
    }

    /// A block that leaves to the interpreter at `ip` with the frame from
    /// `snapshot`, less the top `popped` stack values.
    fn exit_with(&mut self, ip: usize, snapshot: &Snapshot, popped: usize) -> CodeBlock {
        let id = u32::try_from(self.exits.len()).expect("exit count fits");
        let function = self.function;
        let own = |value: &ValueId| !matches!(function.def(*value), ValueDef::Below(_));
        let stack = &snapshot.stack[..snapshot.stack.len() - popped];
        self.exits.push(ExitTable {
            ip,
            locals: snapshot
                .locals
                .iter()
                .map(|local| match local {
                    crate::ir::Local::Value(value) => value.0,
                    crate::ir::Local::Cell => runtime::CELL_LOCAL,
                })
                .collect(),
            stack: stack
                .iter()
                .filter(|value| own(value))
                .map(|value| value.0)
                .collect(),
        });
        let block = self.builder.create_block();
        self.pending.push(Pending::Exit(block, id));
        block
    }

    /// Whether `value` is truthy: anything but `nil` and `false`.
    fn truthy(&mut self, value: ValueId) -> CodeValue {
        if let Some(boolean) = self.booleans[value.index()] {
            return boolean;
        }
        let slot = self.slot(value.index());
        let tag = self.load_tag(slot);
        let not_nil = self
            .builder
            .ins()
            .icmp_imm(IntCC::NotEqual, tag, layout::NIL);
        let is_bool = self.builder.ins().icmp_imm(IntCC::Equal, tag, layout::BOOL);
        let byte = self.load(types::I8, slot, layout::PAYLOAD);
        let bool_true = self.builder.ins().icmp_imm(IntCC::NotEqual, byte, 0);
        self.builder.ins().select(is_bool, bool_true, not_nil)
    }

    /// Drops every owning slot that holds something, leaving `nil`: with a
    /// check per slot inline when there are few, and otherwise by one test
    /// of all their tags inline, calling the runtime to drop them only when
    /// one holds a counted value; most hold numbers or `nil`.
    fn drop_owning(&mut self) {
        let owning: Vec<usize> = (0..self.owning.len())
            .filter(|index| self.owning[*index])
            .collect();
        if owning.len() > INLINE_TESTS {
            // Too many to test inline: a large function, which nearly
            // always holds something counted.
            let base = self.slot(0);
            let base = self.address(base);
            self.call(
                self.runtime.release,
                runtime::native_release as *const () as usize,
                &[self.context, base],
            );
            return;
        }
        if owning.len() > INLINE_RELEASES {
            let mut any = None;
            for index in owning {
                let slot = self.slot(index);
                let tag = self.load_tag(slot);
                let counted = self.builder.ins().icmp_imm(
                    IntCC::SignedGreaterThanOrEqual,
                    tag,
                    layout::FIRST_COUNTED,
                );
                any = Some(match any {
                    Some(any) => self.builder.ins().bor(any, counted),
                    None => counted,
                });
            }
            let any = any.expect("more than INLINE_RELEASES slots");
            let release = self.builder.create_block();
            let next = self.builder.create_block();
            self.builder.ins().brif(any, release, &[], next, &[]);
            self.builder.switch_to_block(release);
            let base = self.slot(0);
            let base = self.address(base);
            self.call(
                self.runtime.release,
                runtime::native_release as *const () as usize,
                &[self.context, base],
            );
            self.builder.ins().jump(next, &[]);
            self.builder.switch_to_block(next);
            return;
        }
        for index in owning {
            let slot = self.slot(index);
            let tag = self.load_tag(slot);
            let counted = self.builder.ins().icmp_imm(
                IntCC::SignedGreaterThanOrEqual,
                tag,
                layout::FIRST_COUNTED,
            );
            let release = self.builder.create_block();
            let next = self.builder.create_block();
            self.builder.ins().brif(counted, release, &[], next, &[]);
            self.builder.switch_to_block(release);
            let slot = self.address(slot);
            self.call(
                self.runtime.drop,
                runtime::native_drop as *const () as usize,
                &[slot],
            );
            self.builder.ins().jump(next, &[]);
            self.builder.switch_to_block(next);
        }
    }

    /// Copies `source` into the free slot `target`, counting a reference
    /// when the value has one.
    fn clone_into(&mut self, source: Place, target: Place) {
        let tag = self.load_tag(source);
        let counted = self.builder.ins().icmp_imm(
            IntCC::SignedGreaterThanOrEqual,
            tag,
            layout::FIRST_COUNTED,
        );
        let counting = self.builder.create_block();
        let copying = self.builder.create_block();
        let next = self.builder.create_block();
        self.builder
            .ins()
            .brif(counted, counting, &[], copying, &[]);
        self.builder.switch_to_block(counting);
        let (source_address, target_address) = (self.address(source), self.address(target));
        self.call(
            self.runtime.clone,
            runtime::native_clone as *const () as usize,
            &[source_address, target_address],
        );
        self.builder.ins().jump(next, &[]);
        self.builder.switch_to_block(copying);
        self.copy_words(source, target);
        self.builder.ins().jump(next, &[]);
        self.builder.switch_to_block(next);
    }

    fn call(
        &mut self,
        signature: cranelift_codegen::ir::SigRef,
        function: usize,
        arguments: &[CodeValue],
    ) -> Vec<CodeValue> {
        let callee = self
            .builder
            .ins()
            .iconst(self.pointer, runtime::address(function));
        let call = self
            .builder
            .ins()
            .call_indirect(signature, callee, arguments);
        self.builder.inst_results(call).to_vec()
    }

    /// Slot `index` of the native frame.
    #[allow(clippy::unused_self)] // Read beside the other places' helpers.
    fn slot(&self, index: usize) -> Place {
        Place::Slot(index)
    }

    /// The address of `place`, for a Rust function.
    fn address(&mut self, place: Place) -> CodeValue {
        match place {
            Place::Slot(index) => self
                .builder
                .ins()
                .iadd_imm(self.base, i64::from(offset(index, 0))),
            Place::At(address) => address,
        }
    }

    /// The address and offset of byte `within` of `place`. Slots are
    /// reached from the frame's one base address: Cranelift would keep each
    /// slot's own address live in a register.
    fn locate(&self, place: Place, within: i32) -> (CodeValue, i32) {
        match place {
            Place::Slot(index) => (self.base, offset(index, within)),
            Place::At(address) => (address, within),
        }
    }

    fn load(&mut self, kind: types::Type, place: Place, within: i32) -> CodeValue {
        let (address, offset) = self.locate(place, within);
        self.builder
            .ins()
            .load(kind, MemFlags::trusted(), address, offset)
    }

    fn store(&mut self, value: CodeValue, place: Place, within: i32) {
        let (address, offset) = self.locate(place, within);
        self.builder
            .ins()
            .store(MemFlags::trusted(), value, address, offset);
    }

    /// The number in `place` as a float: its payload when it is a float,
    /// or, when `ints` is set, its integer converted as `numbers` in the VM
    /// converts it; anything else branches to `slow`.
    fn float_of(&mut self, place: Place, ints: bool, slow: CodeBlock) -> CodeValue {
        let tag = self.load_tag(place);
        let join = self.builder.create_block();
        self.builder.append_block_param(join, types::F64);
        let float_block = self.builder.create_block();
        let other = if ints {
            self.builder.create_block()
        } else {
            slow
        };
        let is_float = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, tag, layout::FLOAT);
        self.builder
            .ins()
            .brif(is_float, float_block, &[], other, &[]);
        self.builder.switch_to_block(float_block);
        let payload = self.load_payload(place, types::F64);
        self.builder.ins().jump(join, &[payload]);
        if ints {
            self.builder.switch_to_block(other);
            let is_int = self.builder.ins().icmp_imm(IntCC::Equal, tag, layout::INT);
            let int_block = self.builder.create_block();
            self.builder.ins().brif(is_int, int_block, &[], slow, &[]);
            self.builder.switch_to_block(int_block);
            let integer = self.load_payload(place, types::I64);
            let converted = self.builder.ins().fcvt_from_sint(types::F64, integer);
            self.builder.ins().jump(join, &[converted]);
        }
        self.builder.switch_to_block(join);
        self.builder.block_params(join)[0]
    }

    fn load_tag(&mut self, place: Place) -> CodeValue {
        self.load(types::I64, place, 0)
    }

    fn load_payload(&mut self, place: Place, kind: types::Type) -> CodeValue {
        self.load(kind, place, layout::PAYLOAD)
    }

    fn operand_payload(&mut self, operand: &Operand, kind: types::Type) -> CodeValue {
        match operand {
            Operand::Value(value) => {
                let slot = self.slot(value.index());
                self.load_payload(slot, kind)
            }
            Operand::Constant(Value::Int(value)) => self.builder.ins().iconst(types::I64, *value),
            Operand::Constant(Value::Float(value)) => self.builder.ins().f64const(*value),
            Operand::Constant(_) | Operand::Pooled(_) => {
                unreachable!("only number constants reach a fast path")
            }
        }
    }

    fn store_tag(&mut self, place: Place, tag: i64) {
        let tag = self.builder.ins().iconst(types::I64, tag);
        self.store(tag, place, 0);
    }

    fn store_int(&mut self, place: Place, value: CodeValue) {
        self.store_tag(place, layout::INT);
        self.store(value, place, layout::PAYLOAD);
    }

    fn store_float(&mut self, place: Place, value: CodeValue) {
        self.store_tag(place, layout::FLOAT);
        self.store(value, place, layout::PAYLOAD);
    }

    fn store_bool(&mut self, place: Place, value: CodeValue) {
        self.store_tag(place, layout::BOOL);
        // An `i8` store writes the one byte.
        self.store(value, place, layout::PAYLOAD);
    }

    fn store_constant(&mut self, place: Place, value: &Value) {
        match value {
            Value::Bool(value) => {
                let value = self.builder.ins().iconst(types::I8, i64::from(*value));
                self.store_bool(place, value);
            }
            Value::Int(value) => {
                let value = self.builder.ins().iconst(types::I64, *value);
                self.store_int(place, value);
            }
            Value::Float(value) => {
                let value = self.builder.ins().f64const(*value);
                self.store_float(place, value);
            }
            _ => self.store_tag(place, layout::NIL),
        }
    }

    fn copy_words(&mut self, source: Place, target: Place) {
        for within in [0, 8, 16] {
            let word = self.load(types::I64, source, within);
            self.store(word, target, within);
        }
    }
}

/// A `Value` in memory: a slot of the native frame, which generated code
/// addresses directly, or one at an address.
#[derive(Clone, Copy)]
enum Place {
    Slot(usize),
    At(CodeValue),
}

/// The offset of byte `within` of slot `index` of the native frame.
fn offset(index: usize, within: i32) -> i32 {
    i32::try_from(index).expect("frame fits") * layout::SIZE + within
}

/// The most owning slots a function checks inline when it returns,
/// exits, or recurs.
const INLINE_RELEASES: usize = 6;

/// The most owning slots a function tests at once inline before calling
/// the runtime to release them.
const INLINE_TESTS: usize = 32;

/// How a block that ends in a choice chooses.
#[derive(Clone, Copy)]
enum Choice {
    /// The fall-through when this is set, the target otherwise.
    Fallthrough(CodeValue),
    /// Always the target.
    Target,
}

/// What a call calls.
#[derive(Clone, Copy)]
enum Callee {
    /// A module-level name.
    Global(u32),
    /// The value below the arguments.
    Value,
}

/// An operation's right operand.
enum Operand {
    Value(ValueId),
    /// A number, a boolean, or `nil`, written inline.
    Constant(Value),
    /// Any other value constant of the chunk, by index, cloned in.
    Pooled(usize),
}

/// The values a block's terminator uses: a branch's condition, a
/// returned value, and an edge's or `recur`'s arguments.
fn terminator_uses(terminator: &Terminator) -> Vec<ValueId> {
    match terminator {
        Terminator::Jump(edge) => edge.args.clone(),
        Terminator::Branch {
            cond,
            then,
            otherwise,
        } => std::iter::once(*cond)
            .chain(then.args.iter().copied())
            .chain(otherwise.args.iter().copied())
            .collect(),
        Terminator::Choose {
            fallthrough,
            target,
        } => fallthrough
            .args
            .iter()
            .chain(&target.args)
            .copied()
            .collect(),
        Terminator::Return(value) => vec![*value],
        Terminator::Recur(arguments) => arguments.clone(),
        Terminator::Stop => Vec::new(),
    }
}

/// The values each instruction uses: its operands, and the operand stack
/// of its snapshot, which an exit there hands to the interpreter. A local
/// that holds a value is not a use: code that reads it later uses the
/// value there.
fn inst_uses(inst: &Inst) -> impl Iterator<Item = &ValueId> {
    inst.args.iter().chain(&inst.snapshot.stack)
}

/// The values live out of each block, as bit sets: used in a block it
/// can reach. Only a `recur`, which starts afresh, goes back, so the
/// blocks in reverse order see every successor first.
fn live_out(function: &Function) -> Vec<Vec<u64>> {
    let words = function.values.len().div_ceil(64);
    let mut live_in: Vec<Vec<u64>> = vec![Vec::new(); function.blocks.len()];
    let mut live_out = vec![Vec::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate().rev() {
        let mut out = vec![0u64; words];
        for edge in block.terminator.edges() {
            for (word, live) in out.iter_mut().zip(&live_in[edge.block.index()]) {
                *word |= live;
            }
        }
        let mut entering = out.clone();
        let terminator = terminator_uses(&block.terminator);
        for value in block.insts.iter().flat_map(inst_uses).chain(&terminator) {
            entering[value.index() / 64] |= 1 << (value.index() % 64);
        }
        live_in[index] = entering;
        live_out[index] = out;
    }
    live_out
}

/// Which values' slots may own something: parameters, the results of
/// arithmetic, which a slow path may make a string or a list, and the
/// results of calls.
fn owning_values(function: &Function) -> Vec<bool> {
    let mut owning = vec![false; function.values.len()];
    for block in &function.blocks {
        for param in &block.params {
            owning[param.index()] = true;
        }
        for inst in &block.insts {
            let owns = match binary_operator(&inst.op) {
                Some(operator) => !compares(operator),
                None => match &inst.op {
                    Op::Constant(index) => number_constant(function, *index).is_err(),
                    Op::CallGlobalPooled { .. }
                    | Op::CallPositional(_)
                    | Op::GetGlobalPooled(_)
                    | Op::GetCapture(_)
                    | Op::GetLocal(_)
                    | Op::MakeClosure { .. }
                    | Op::MakeClosurePooled { .. }
                    | Op::ListAppend
                    | Op::ListPrepend
                    | Op::GetSlice { .. }
                    | Op::Struct(_)
                    | Op::StructPooled(_)
                    | Op::StructCopy(_)
                    | Op::StructCopyPooled(_)
                    | Op::CopyGlobalPooled { .. }
                    | Op::UpdateCapture { .. }
                    | Op::Map(_)
                    | Op::InterpolatePooled(_)
                    | Op::MatchCasePooled { .. }
                    | Op::TryMatchPooled { .. }
                    // `~` on bytes makes bytes.
                    | Op::BitNot
                    | Op::List(_)
                    | Op::GetIndex
                    | Op::GetIndexConstant(_)
                    | Op::ReadIndex { .. }
                    | Op::ReadIndexConstant { .. } => true,
                    _ => false,
                },
            };
            if owns {
                for result in &inst.results {
                    owning[result.index()] = true;
                }
            }
        }
    }
    owning
}
