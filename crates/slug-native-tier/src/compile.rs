//! Lowering a lifted function to machine code: the baseline tier.
//!
//! Every IR value has a 24-byte slot in the native frame holding a `Value`
//! exactly as the VM lays it out. Integer and float arithmetic and
//! comparisons run inline when both operands have the fast path's type;
//! every other case calls the interpreter's own operation. Anything that
//! fails, and a thread that needs attention at a `recur`, leaves to the
//! interpreter before the instruction, with the frame from its snapshot.
//!
//! Ownership: a slot whose value may own something (a parameter, or the
//! result of arithmetic, which a slow path can make a string or a list) is
//! `nil` until written, and every such slot is dropped when the function
//! returns or exits and reset when it recurs. Values move into another
//! slot by a clone, so each slot owns what it holds.
//!
//! Phase 1, step 3a: a function compiles only if it uses constants,
//! locals, arithmetic, comparisons, branches, a positional `recur`, and
//! `return` (see [`qualify`]).

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
use slug_vm::{BinaryOperator, Constant, Op, Value};

use crate::{
    ir::{Block, Edge, Function, Inst, Terminator, ValueDef, ValueId},
    jit::Jit,
    runtime::{self, BINARY_OPERATORS, CONTEXT_RET, Compiled, EXITED, ExitTable, RETURNED, layout},
};

/// Why a function does not compile, or `Ok` if it can.
///
/// # Errors
///
/// Returns the first thing the baseline tier does not compile yet.
pub fn qualify(function: &Function) -> Result<(), String> {
    if function.cells.iter().any(|cell| *cell) {
        return Err("a closure captures one of its locals".into());
    }
    for block in &function.blocks {
        for inst in &block.insts {
            match &inst.op {
                Op::Constant(index) => {
                    number_constant(function, *index)?;
                }
                Op::Nil | Op::True | Op::False => {}
                Op::LocalConstantBinary { constant, .. } | Op::ConstantBinary { constant, .. } => {
                    number_constant(function, *constant)?;
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
            | Terminator::Recur(_) => {}
            Terminator::Choose { .. } | Terminator::Stop => {
                return Err(format!(
                    "the block at instruction {} ends in a way not compiled yet",
                    block.start
                ));
            }
        }
    }
    Ok(())
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

/// Compiles a function that [`qualify`] accepts.
pub(crate) fn compile(jit: &Jit, function: &Function) -> Result<Compiled, String> {
    qualify(function)?;
    let mut context = Context::new();
    let pointer = jit.isa().pointer_type();
    let call_conv = jit.isa().default_call_conv();
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(pointer));
    signature.params.push(AbiParam::new(pointer));
    signature.returns.push(AbiParam::new(types::I32));
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    let exits = {
        let builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        Lowering::new(builder, function, pointer, call_conv).lower()?
    };
    let name = format!(
        "slug_{}_{}",
        function.chunk,
        COMPILED.fetch_add(1, Ordering::Relaxed)
    );
    let entry = jit.define(&name, &mut context)?;
    Ok(Compiled::new(entry, function.arity, exits))
}

/// The Rust functions generated code calls, by signature.
struct Runtime {
    binary: cranelift_codegen::ir::SigRef,
    attention: cranelift_codegen::ir::SigRef,
    clone: cranelift_codegen::ir::SigRef,
    drop: cranelift_codegen::ir::SigRef,
    exit: cranelift_codegen::ir::SigRef,
}

struct Lowering<'a> {
    builder: FunctionBuilder<'a>,
    function: &'a Function,
    pointer: types::Type,
    runtime: Runtime,
    /// The native frame: one slot per IR value, then scratch slots for a
    /// `recur`'s arguments, then one for a constant operand.
    frame: StackSlot,
    blocks: Vec<CodeBlock>,
    /// Ends the function: drops every owning slot and returns its status.
    cleanup: CodeBlock,
    context: CodeValue,
    owning: Vec<bool>,
    exits: Vec<ExitTable>,
    /// Blocks made while another was being filled, filled at the end.
    pending: Vec<Pending>,
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
        let runtime = Runtime {
            binary: signature(
                &[pointer, types::I32, pointer, pointer, pointer],
                &[types::I32],
            ),
            attention: signature(&[pointer], &[types::I32]),
            clone: signature(&[pointer, pointer], &[]),
            drop: signature(&[pointer], &[]),
            exit: signature(&[pointer, types::I32, pointer], &[]),
        };
        let slots = function.values.len() + function.arity + 1;
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
        let owning = owning_values(function);
        Self {
            builder,
            function,
            pointer,
            runtime,
            frame,
            blocks,
            cleanup,
            context: CodeValue::from_u32(0),
            owning,
            exits: Vec::new(),
            pending: Vec::new(),
        }
    }

    fn lower(mut self) -> Result<Vec<ExitTable>, String> {
        let entry = self.builder.create_block();
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);
        self.context = self.builder.block_params(entry)[0];
        let arguments = self.builder.block_params(entry)[1];
        let function = self.function;
        // Owning slots and unset locals start `nil`.
        for (index, def) in function.values.iter().enumerate() {
            if self.owning[index] || matches!(def, ValueDef::UnsetLocal) {
                let slot = self.slot(index);
                self.store_tag(slot, layout::NIL);
            }
        }
        // The arguments move into the entry block's parameter slots.
        for (index, param) in function.blocks[0].params.iter().enumerate() {
            let offset = i32::try_from(index).expect("arity fits") * layout::SIZE;
            let source = self.builder.ins().iadd_imm(arguments, i64::from(offset));
            let target = self.slot(param.index());
            self.copy_words(source, target);
        }
        let first = self.blocks[0];
        self.builder.ins().jump(first, &[]);

        for (index, block) in function.blocks.iter().enumerate() {
            self.builder.switch_to_block(self.blocks[index]);
            for inst in &block.insts {
                self.lower_inst(inst)?;
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
        Ok(self.exits)
    }

    fn lower_inst(&mut self, inst: &Inst) -> Result<(), String> {
        match &inst.op {
            Op::Constant(index) => {
                let value = number_constant(self.function, *index)?.clone();
                let slot = self.slot(inst.results[0].index());
                self.store_constant(slot, &value);
            }
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
            // The `recur` itself is lowered with its block's terminator.
            Op::RecurPositional(_) => {}
            op => {
                let operator = binary_operator(op)
                    .ok_or_else(|| format!("instruction {} is not compiled yet", inst.ip))?;
                let right = match op {
                    Op::LocalConstantBinary { constant, .. }
                    | Op::ConstantBinary { constant, .. } => {
                        Operand::Constant(number_constant(self.function, *constant)?.clone())
                    }
                    _ => Operand::Value(inst.args[1]),
                };
                self.lower_binary(inst, operator, inst.args[0], &right);
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
        left: ValueId,
        right: &Operand,
    ) {
        use BinaryOperator as B;
        let result = self.slot(inst.results[0].index());
        let left_slot = self.slot(left.index());
        let left_tag = self.load_tag(left_slot);
        let done = self.builder.create_block();
        let slow = self.builder.create_block();

        // A constant right operand of the other type never takes a path.
        let (int_constant, float_constant) = match right {
            Operand::Value(_) => (true, true),
            Operand::Constant(constant) => (
                matches!(constant, Value::Int(_)),
                matches!(constant, Value::Float(_)),
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
                    | B::Equal
                    | B::Greater
                    | B::GreaterNum
                    | B::Less
                    | B::LessNum
                    | B::BitAnd
                    | B::BitOr
                    | B::BitXor
            );
        let float_fast = float_constant
            && matches!(
                operator,
                B::Add
                    | B::AddNum
                    | B::Subtract
                    | B::SubtractNum
                    | B::Multiply
                    | B::MultiplyNum
                    | B::Equal
                    | B::Greater
                    | B::GreaterNum
                    | B::Less
                    | B::LessNum
            );
        // Whether both operands have `tag`, and the right one's payload.
        let both = |lowering: &mut Self, tag: i64| -> CodeValue {
            let left_is = lowering.builder.ins().icmp_imm(IntCC::Equal, left_tag, tag);
            match right {
                Operand::Value(value) => {
                    let right_slot = lowering.slot(value.index());
                    let right_tag = lowering.load_tag(right_slot);
                    let right_is = lowering
                        .builder
                        .ins()
                        .icmp_imm(IntCC::Equal, right_tag, tag);
                    lowering.builder.ins().band(left_is, right_is)
                }
                Operand::Constant(constant) => {
                    let matches = match constant {
                        Value::Int(_) => tag == layout::INT,
                        Value::Float(_) => tag == layout::FLOAT,
                        _ => false,
                    };
                    if matches {
                        left_is
                    } else {
                        lowering.builder.ins().iconst(types::I8, 0)
                    }
                }
            }
        };

        let after_int = self.builder.create_block();
        if int_fast {
            let is_int = both(self, layout::INT);
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
                }
                B::BitAnd | B::BitOr | B::BitXor => {
                    let value = match operator {
                        B::BitAnd => self.builder.ins().band(a, b),
                        B::BitOr => self.builder.ins().bor(a, b),
                        _ => self.builder.ins().bxor(a, b),
                    };
                    self.store_int(result, value);
                }
                _ => {
                    let condition = match operator {
                        B::Equal => IntCC::Equal,
                        B::Greater | B::GreaterNum => IntCC::SignedGreaterThan,
                        _ => IntCC::SignedLessThan,
                    };
                    let value = self.builder.ins().icmp(condition, a, b);
                    self.store_bool(result, value);
                }
            }
            self.builder.ins().jump(done, &[]);
        } else {
            self.builder.ins().jump(after_int, &[]);
        }

        self.builder.switch_to_block(after_int);
        if float_fast {
            let is_float = both(self, layout::FLOAT);
            let float_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(is_float, float_block, &[], slow, &[]);
            self.builder.switch_to_block(float_block);
            let a = self.load_payload(left_slot, types::F64);
            let b = self.operand_payload(right, types::F64);
            match operator {
                B::Add
                | B::AddNum
                | B::Subtract
                | B::SubtractNum
                | B::Multiply
                | B::MultiplyNum => {
                    let value = match operator {
                        B::Add | B::AddNum => self.builder.ins().fadd(a, b),
                        B::Subtract | B::SubtractNum => self.builder.ins().fsub(a, b),
                        _ => self.builder.ins().fmul(a, b),
                    };
                    // Finite exactly when `value - value` is zero.
                    let difference = self.builder.ins().fsub(value, value);
                    let zero = self.builder.ins().f64const(0.0);
                    let finite = self.builder.ins().fcmp(FloatCC::Equal, difference, zero);
                    let store = self.builder.create_block();
                    self.builder.ins().brif(finite, store, &[], slow, &[]);
                    self.builder.switch_to_block(store);
                    self.store_float(result, value);
                }
                _ => {
                    let condition = match operator {
                        B::Equal => FloatCC::Equal,
                        B::Greater | B::GreaterNum => FloatCC::GreaterThan,
                        _ => FloatCC::LessThan,
                    };
                    let value = self.builder.ins().fcmp(condition, a, b);
                    self.store_bool(result, value);
                }
            }
            self.builder.ins().jump(done, &[]);
        } else {
            self.builder.ins().jump(slow, &[]);
        }

        // The interpreter's own operation, or an exit if it fails.
        self.builder.switch_to_block(slow);
        let right_address = match right {
            Operand::Value(value) => self.slot(value.index()),
            Operand::Constant(constant) => {
                let scratch = self.slot(self.function.values.len() + self.function.arity);
                self.store_constant(scratch, constant);
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
        let status = self.call(
            self.runtime.binary,
            runtime::native_binary as *const () as usize,
            &[
                self.context,
                operator_code,
                left_slot,
                right_address,
                result,
            ],
        )[0];
        let exit = self.exit_block(inst);
        self.builder.ins().brif(status, exit, &[], done, &[]);

        self.builder.switch_to_block(done);
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
                let target = self
                    .builder
                    .ins()
                    .iadd_imm(self.context, i64::from(CONTEXT_RET));
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
                let params = self.function.blocks[0].params.clone();
                for (index, param) in params.iter().enumerate() {
                    let source = self.slot(scratch_base + index);
                    let target = self.slot(param.index());
                    self.copy_words(source, target);
                }
                let first = self.blocks[0];
                self.builder.ins().jump(first, &[]);
            }
            Terminator::Choose { .. } | Terminator::Stop => {
                return Err("a block ends in a way not compiled yet".into());
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
        let id = u32::try_from(self.exits.len()).expect("exit count fits");
        let function = self.function;
        let own = |value: &ValueId| !matches!(function.def(*value), ValueDef::Below(_));
        self.exits.push(ExitTable {
            ip: inst.ip,
            locals: inst
                .snapshot
                .locals
                .iter()
                .map(|local| match local {
                    crate::ir::Local::Value(value) => value.0,
                    crate::ir::Local::Cell => unreachable!("qualified functions have no cells"),
                })
                .collect(),
            stack: inst
                .snapshot
                .stack
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
        let slot = self.slot(value.index());
        let tag = self.load_tag(slot);
        let not_nil = self
            .builder
            .ins()
            .icmp_imm(IntCC::NotEqual, tag, layout::NIL);
        let is_bool = self.builder.ins().icmp_imm(IntCC::Equal, tag, layout::BOOL);
        let byte =
            self.builder
                .ins()
                .uload8(types::I32, MemFlags::trusted(), slot, layout::PAYLOAD);
        let bool_true = self.builder.ins().icmp_imm(IntCC::NotEqual, byte, 0);
        self.builder.ins().select(is_bool, bool_true, not_nil)
    }

    /// Drops every owning slot that holds something, leaving `nil`.
    fn drop_owning(&mut self) {
        for index in 0..self.owning.len() {
            if !self.owning[index] {
                continue;
            }
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
    fn clone_into(&mut self, source: CodeValue, target: CodeValue) {
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
        self.call(
            self.runtime.clone,
            runtime::native_clone as *const () as usize,
            &[source, target],
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

    /// The address of slot `index` of the native frame.
    fn slot(&mut self, index: usize) -> CodeValue {
        let offset = i32::try_from(index).expect("frame fits") * layout::SIZE;
        self.builder
            .ins()
            .stack_addr(self.pointer, self.frame, offset)
    }

    fn load_tag(&mut self, slot: CodeValue) -> CodeValue {
        self.builder
            .ins()
            .load(types::I64, MemFlags::trusted(), slot, 0)
    }

    fn load_payload(&mut self, slot: CodeValue, kind: types::Type) -> CodeValue {
        self.builder
            .ins()
            .load(kind, MemFlags::trusted(), slot, layout::PAYLOAD)
    }

    fn operand_payload(&mut self, operand: &Operand, kind: types::Type) -> CodeValue {
        match operand {
            Operand::Value(value) => {
                let slot = self.slot(value.index());
                self.load_payload(slot, kind)
            }
            Operand::Constant(Value::Int(value)) => self.builder.ins().iconst(types::I64, *value),
            Operand::Constant(Value::Float(value)) => self.builder.ins().f64const(*value),
            Operand::Constant(_) => unreachable!("only number constants reach a fast path"),
        }
    }

    fn store_tag(&mut self, slot: CodeValue, tag: i64) {
        let tag = self.builder.ins().iconst(types::I64, tag);
        self.builder.ins().store(MemFlags::trusted(), tag, slot, 0);
    }

    fn store_int(&mut self, slot: CodeValue, value: CodeValue) {
        self.store_tag(slot, layout::INT);
        self.builder
            .ins()
            .store(MemFlags::trusted(), value, slot, layout::PAYLOAD);
    }

    fn store_float(&mut self, slot: CodeValue, value: CodeValue) {
        self.store_tag(slot, layout::FLOAT);
        self.builder
            .ins()
            .store(MemFlags::trusted(), value, slot, layout::PAYLOAD);
    }

    fn store_bool(&mut self, slot: CodeValue, value: CodeValue) {
        self.store_tag(slot, layout::BOOL);
        // An `i8` store writes the one byte.
        self.builder
            .ins()
            .store(MemFlags::trusted(), value, slot, layout::PAYLOAD);
    }

    fn store_constant(&mut self, slot: CodeValue, value: &Value) {
        match value {
            Value::Bool(value) => {
                let value = self.builder.ins().iconst(types::I8, i64::from(*value));
                self.store_bool(slot, value);
            }
            Value::Int(value) => {
                let value = self.builder.ins().iconst(types::I64, *value);
                self.store_int(slot, value);
            }
            Value::Float(value) => {
                let value = self.builder.ins().f64const(*value);
                self.store_float(slot, value);
            }
            _ => self.store_tag(slot, layout::NIL),
        }
    }

    fn copy_words(&mut self, source: CodeValue, target: CodeValue) {
        for offset in [0, 8, 16] {
            let word = self
                .builder
                .ins()
                .load(types::I64, MemFlags::trusted(), source, offset);
            self.builder
                .ins()
                .store(MemFlags::trusted(), word, target, offset);
        }
    }
}

/// An operation's right operand.
enum Operand {
    Value(ValueId),
    Constant(Value),
}

/// Which values' slots may own something: parameters, and the results of
/// arithmetic, which a slow path may make a string or a list.
fn owning_values(function: &Function) -> Vec<bool> {
    let mut owning = vec![false; function.values.len()];
    for block in &function.blocks {
        for param in &block.params {
            owning[param.index()] = true;
        }
        for inst in &block.insts {
            if let Some(operator) = binary_operator(&inst.op)
                && !compares(operator)
            {
                for result in &inst.results {
                    owning[result.index()] = true;
                }
            }
        }
    }
    owning
}
