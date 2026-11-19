//! Running generated code: the context it runs with, the Rust functions it
//! calls, and entering it from the VM.
//!
//! This module and [`crate::jit`] hold the crate's only `unsafe` code. Each
//! function generated code calls takes raw pointers into the native frame,
//! turns them into references, and calls safe Rust, including `slug-vm`'s
//! own operations.

#![allow(unsafe_code)]

use std::mem::{MaybeUninit, offset_of, size_of};

use slug_vm::{BinaryOperator, GlobalNameId, NativeCode, NativeFrame, NativeOutcome, Value, Vm};

/// Where generated code finds a [`Value`]'s parts. `Value` is `repr(u64)`
/// on 64-bit targets: a word tag, then its payload from the next word.
pub(crate) mod layout {
    pub(crate) const SIZE: i32 = 24;
    pub(crate) const PAYLOAD: i32 = 8;
    pub(crate) const NIL: i64 = 1;
    pub(crate) const BOOL: i64 = 2;
    pub(crate) const INT: i64 = 3;
    pub(crate) const FLOAT: i64 = 4;
    /// Tags from here on may own something to count or drop.
    pub(crate) const FIRST_COUNTED: i64 = 5;
}

/// Whether this build's `Value` has the layout [`layout`] describes. The
/// tier compiles nothing if not.
pub(crate) fn layout_matches() -> bool {
    fn tag(value: &Value) -> u64 {
        // SAFETY: a `Value` is 24 bytes, and a `repr(u64)` enum's tag is
        // its initialized first word.
        unsafe { std::ptr::from_ref(value).cast::<u64>().read() }
    }
    fn word(value: &Value) -> u64 {
        // SAFETY: an `Int` or `Float` payload is the initialized second word.
        unsafe { std::ptr::from_ref(value).cast::<u64>().add(1).read() }
    }
    fn byte(value: &Value) -> u8 {
        // SAFETY: a `Bool` payload is the initialized byte at offset 8.
        unsafe { std::ptr::from_ref(value).cast::<u8>().add(8).read() }
    }
    size_of::<Value>() == layout::SIZE as usize
        && tag(&Value::Nil) == layout::NIL as u64
        && tag(&Value::Bool(true)) == layout::BOOL as u64
        && byte(&Value::Bool(true)) == 1
        && byte(&Value::Bool(false)) == 0
        && tag(&Value::Int(-7)) == layout::INT as u64
        && word(&Value::Int(-7)) == (-7i64).cast_unsigned()
        && tag(&Value::Float(1.5)) == layout::FLOAT as u64
        && word(&Value::Float(1.5)) == 1.5f64.to_bits()
}

/// One frame's state as the interpreter will see it at an exit: each
/// local's and each of the function's own stack values' slot.
#[derive(Clone, Debug)]
pub(crate) struct ExitTable {
    pub(crate) ip: usize,
    pub(crate) locals: Vec<u32>,
    pub(crate) stack: Vec<u32>,
}

/// Native calls deeper than this leave to the interpreter, which has no
/// limit of its own; this bounds the machine stack native code uses.
pub(crate) const MAX_DEPTH: u32 = 256;

/// What every activation of one entry from the interpreter shares.
struct Root {
    /// Native calls in progress.
    depth: u32,
    /// The frames of exiting calls, innermost first.
    frames: Vec<NativeFrame>,
}

/// A frame native code left to the interpreter.
struct Exited {
    ip: usize,
    locals: Vec<Value>,
    stack: Vec<Value>,
}

/// What one activation of generated code runs with. Its address is the
/// code's first argument.
#[repr(C)]
pub(crate) struct Context {
    vm: *mut Vm,
    exits: *const ExitTable,
    exit_count: usize,
    /// The function running, a closure.
    closure: *const Value,
    root: *mut Root,
    /// Where a returning function leaves its value.
    ret: MaybeUninit<Value>,
    /// What an exit leaves for the interpreter.
    exit: Option<Exited>,
}

/// Offsets generated code uses.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // A small struct.
pub(crate) const CONTEXT_RET: i32 = offset_of!(Context, ret) as i32;

/// Status generated code returns.
pub(crate) const RETURNED: u32 = 0;
pub(crate) const EXITED: u32 = 1;

/// Status [`native_call`] returns: the callee returned, it was not called
/// (the caller leaves before the call), or it exited (the caller leaves
/// after it).
pub(crate) const CALL_RETURNED: u32 = 0;
pub(crate) const CALL_DECLINED: u32 = 1;
pub(crate) const CALL_EXITED: u32 = 2;

type Entry = unsafe extern "C" fn(*mut Context, *mut Value) -> u32;

/// A chunk compiled to native code.
pub(crate) struct Compiled {
    entry: usize,
    arity: usize,
    exits: Vec<ExitTable>,
}

impl Compiled {
    /// `entry` must be the address of finalized code generated for a
    /// function of `arity` parameters with these exit tables, which stays
    /// mapped for the life of the process.
    pub(crate) fn new(entry: *const u8, arity: usize, exits: Vec<ExitTable>) -> Self {
        Self {
            entry: entry as usize,
            arity,
            exits,
        }
    }

    /// Runs the code with the `arity` arguments at `arguments`, which it
    /// moves out.
    ///
    /// # Safety
    ///
    /// `vm` and `root` must be valid for the call, `closure` must be the
    /// function this code was compiled from, and `arguments` must point to
    /// `arity` initialized values the caller then forgets.
    unsafe fn enter(
        &self,
        vm: *mut Vm,
        closure: *const Value,
        root: *mut Root,
        arguments: *mut Value,
    ) -> Result<Value, Exited> {
        let mut context = Context {
            vm,
            exits: self.exits.as_ptr(),
            exit_count: self.exits.len(),
            closure,
            root,
            ret: MaybeUninit::uninit(),
            exit: None,
        };
        // SAFETY: `entry` is finalized code with this signature (see
        // `Compiled::new`). It returns `RETURNED` only after writing `ret`,
        // and `EXITED` only after `native_exit` set `exit`.
        unsafe {
            let entry: Entry = std::mem::transmute::<usize, Entry>(self.entry);
            if entry(&raw mut context, arguments) == RETURNED {
                Ok(context.ret.assume_init())
            } else {
                Err(context
                    .exit
                    .take()
                    .expect("native code exited through native_exit"))
            }
        }
    }
}

impl NativeCode for Compiled {
    fn run(&self, vm: &mut Vm, closure: Value, mut locals: Vec<Value>) -> NativeOutcome {
        // The code takes the arguments; the other locals are the frame's
        // unset `nil`s, which it does not need.
        locals.truncate(self.arity);
        if locals.len() != self.arity {
            locals.resize(self.arity, Value::Nil);
        }
        let mut root = Root {
            depth: 0,
            frames: Vec::new(),
        };
        // SAFETY: everything passed lives across the call; the code moves
        // the arguments out of `locals`, so they are forgotten here rather
        // than dropped twice.
        let outcome = unsafe {
            let outcome = self.enter(
                std::ptr::from_mut(vm),
                &raw const closure,
                &raw mut root,
                locals.as_mut_ptr(),
            );
            locals.set_len(0);
            outcome
        };
        match outcome {
            Ok(value) => NativeOutcome::Return(value),
            Err(Exited { ip, locals, stack }) => {
                root.frames.reverse();
                NativeOutcome::Exit {
                    ip,
                    locals,
                    stack,
                    frames: root.frames,
                }
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Calls module-level name `name` with the `count` arguments at
/// `arguments`, which it takes, when it is a compiled closure an exact
/// positional call enters: writes its result to `out` and returns
/// `CALL_RETURNED`. Returns `CALL_EXITED` when the callee left to the
/// interpreter, its frame recorded, and `CALL_DECLINED` when there was no
/// such callee, or the native calls are too deep.
pub(crate) extern "C" fn native_call(
    context: *mut Context,
    name: u32,
    count: u32,
    arguments: *mut Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, `count` initialized
    // values it gives up, and a slot holding no value that needs dropping.
    unsafe {
        let context = &*context;
        let root = context.root;
        let arguments_slice = std::slice::from_raw_parts_mut(arguments, count as usize);
        let callee = if (*root).depth < MAX_DEPTH
            // An argument the interpreter must resolve first.
            && !arguments_slice
                .iter()
                .any(|value| matches!(value, Value::Binding { .. }))
        {
            (*context.vm).native_callee(&*context.closure, GlobalNameId::new(name), count as usize)
        } else {
            None
        };
        let Some((closure, code)) = callee else {
            std::ptr::drop_in_place(arguments_slice);
            return CALL_DECLINED;
        };
        let Some(compiled) = code.as_any().downcast_ref::<Compiled>() else {
            std::ptr::drop_in_place(arguments_slice);
            return CALL_DECLINED;
        };
        (*root).depth += 1;
        let outcome = compiled.enter(context.vm, &raw const closure, root, arguments);
        (*root).depth -= 1;
        match outcome {
            Ok(value) => {
                out.write(value);
                CALL_RETURNED
            }
            Err(Exited { ip, locals, stack }) => {
                (*root).frames.push(NativeFrame {
                    closure,
                    ip,
                    locals,
                    stack,
                });
                CALL_EXITED
            }
        }
    }
}

/// `operator` applied by the interpreter's own code, for an operand that is
/// not on a fast path. Writes the result to `out` and returns 0, or returns
/// 1 when the operation fails, so native code exits and the interpreter
/// fails with the error.
pub(crate) extern "C" fn native_binary(
    context: *mut Context,
    operator: u32,
    left: *const Value,
    right: *const Value,
    out: *mut Value,
) -> u32 {
    let Some(operator) = BINARY_OPERATORS.get(operator as usize) else {
        return 1;
    };
    // SAFETY: generated code passes its own context, and pointers to
    // initialized values in its frame; `out` is a slot holding no value
    // that needs dropping.
    unsafe {
        let vm = &*(*context).vm;
        match vm.native_binary(*operator, &*left, &*right) {
            Some(value) => {
                out.write(value);
                0
            }
            None => 1,
        }
    }
}

/// Whether the thread needs attention (a cancelled fork, or forks' host
/// calls to serve): native code then exits before its `recur`, and the
/// interpreter's own check deals with it.
pub(crate) extern "C" fn native_attention(context: *mut Context) -> u32 {
    // SAFETY: generated code passes its own context.
    unsafe { u32::from((*(*context).vm).native_needs_attention()) }
}

/// Clones `source` into `target`, a slot holding no value that needs
/// dropping.
pub(crate) extern "C" fn native_clone(source: *const Value, target: *mut Value) {
    // SAFETY: generated code passes initialized values and a free slot.
    unsafe { target.write((*source).clone()) }
}

/// Drops the value in `slot` and leaves `nil` there.
pub(crate) extern "C" fn native_drop(slot: *mut Value) {
    // SAFETY: generated code passes an initialized slot it owns.
    unsafe {
        std::ptr::drop_in_place(slot);
        slot.write(Value::Nil);
    }
}

/// Records exit `exit`: clones the frame its table describes from the
/// slots at `base` into the context. Generated code then drops its slots
/// and returns `EXITED`.
pub(crate) extern "C" fn native_exit(context: *mut Context, exit: u32, base: *const Value) {
    // SAFETY: generated code passes its own context, an exit id it was
    // compiled with, and its slots, each initialized.
    unsafe {
        let context = &mut *context;
        let tables = std::slice::from_raw_parts(context.exits, context.exit_count);
        let table = &tables[exit as usize];
        let read = |slot: &u32| (*base.add(*slot as usize)).clone();
        context.exit = Some(Exited {
            ip: table.ip,
            locals: table.locals.iter().map(read).collect(),
            stack: table.stack.iter().map(read).collect(),
        });
    }
}

/// The operators generated code names by index.
pub(crate) const BINARY_OPERATORS: [BinaryOperator; 20] = [
    BinaryOperator::Add,
    BinaryOperator::AddNum,
    BinaryOperator::Subtract,
    BinaryOperator::SubtractNum,
    BinaryOperator::Multiply,
    BinaryOperator::MultiplyNum,
    BinaryOperator::Divide,
    BinaryOperator::DivideNum,
    BinaryOperator::Modulo,
    BinaryOperator::ModuloNum,
    BinaryOperator::Equal,
    BinaryOperator::Greater,
    BinaryOperator::GreaterNum,
    BinaryOperator::Less,
    BinaryOperator::LessNum,
    BinaryOperator::BitAnd,
    BinaryOperator::BitOr,
    BinaryOperator::BitXor,
    BinaryOperator::ShiftLeft,
    BinaryOperator::ShiftRight,
];

/// The address of a function generated code calls.
pub(crate) fn address(function: usize) -> i64 {
    i64::try_from(function).expect("addresses fit in 63 bits")
}

#[cfg(test)]
mod tests {
    #[test]
    fn values_have_the_layout_generated_code_assumes() {
        assert!(super::layout_matches());
    }
}
