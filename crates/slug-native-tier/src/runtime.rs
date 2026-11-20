//! Running generated code: the context it runs with, the Rust functions it
//! calls, and entering it from the VM.
//!
//! This module and [`crate::jit`] hold the crate's only `unsafe` code. Each
//! function generated code calls takes raw pointers into the native frame,
//! turns them into references, and calls safe Rust, including `slug-vm`'s
//! own operations.

#![allow(unsafe_code)]

use std::mem::{MaybeUninit, offset_of, size_of};

use slug_vm::{
    BinaryOperator, Capture, GlobalNameId, MatchPatternId, NativeCode, NativeFailure, NativeFrame,
    NativeLocal, NativeOperation, NativeOutcome, NativeReadSource, Value, Vm,
};

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

/// An exit table's entry for a captured local, which the context holds
/// rather than a slot.
pub(crate) const CELL_LOCAL: u32 = u32::MAX;

/// One frame's state as the interpreter will see it at an exit: each
/// local's slot, or [`CELL_LOCAL`], and each of the function's own stack
/// values' slot.
#[derive(Clone, Debug)]
pub(crate) struct ExitTable {
    pub(crate) ip: usize,
    pub(crate) locals: Vec<u32>,
    pub(crate) stack: Vec<u32>,
}

/// The machine stack native calls may use from where the interpreter
/// entered native code. A call past it leaves to the interpreter, which
/// keeps its frames on the heap and has no limit of its own. Frames differ
/// in size, so the bound is in bytes rather than calls; it leaves most of
/// a fork thread's 2 MiB stack to everything else.
pub(crate) const STACK_BUDGET: usize = 256 * 1024;

/// Counts for `SLUG_NATIVE_TRACE`'s summary, kept only while tracing.
pub(crate) mod stats {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    pub(crate) static TRACING: AtomicBool = AtomicBool::new(false);
    /// Entries into native code from the interpreter.
    pub(crate) static ENTRIES: AtomicU64 = AtomicU64::new(0);
    /// Activations that left to the interpreter, callers included.
    pub(crate) static EXITS: AtomicU64 = AtomicU64::new(0);
    /// Calls native code could not make because the callee had no native
    /// code (or was not a closure an exact call enters).
    pub(crate) static UNCOMPILED_CALLS: AtomicU64 = AtomicU64::new(0);

    #[inline]
    pub(crate) fn count(counter: &AtomicU64) {
        if TRACING.load(Ordering::Relaxed) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What every activation of one entry from the interpreter shares.
struct Root {
    /// An address near the top of the machine stack at the entry.
    base: usize,
    /// The frames of exiting calls, innermost first.
    frames: Vec<NativeFrame>,
    /// A builtin's failed call, which the innermost frame left past.
    failure: Option<NativeFailure>,
}

/// The machine stack used since `root`'s entry, roughly: native frames
/// and the Rust frames between them.
#[inline]
fn stack_used(root: &Root) -> usize {
    let here = 0u8;
    root.base.abs_diff(std::ptr::from_ref(&here) as usize)
}

/// A frame native code left to the interpreter.
struct Exited {
    ip: usize,
    locals: Vec<NativeLocal>,
    stack: Vec<Value>,
}

/// What one activation of generated code runs with. Its address is the
/// code's first argument.
#[repr(C)]
pub(crate) struct Context {
    vm: *mut Vm,
    compiled: *const Compiled,
    /// The captured locals, by slot, of a function a closure captures
    /// locals of; others' entries are unused, and other functions have
    /// none.
    cells: Vec<NativeLocal>,
    exits: *const ExitTable,
    exit_count: usize,
    /// The frame slots that may own something, for [`native_release`].
    owning: *const u32,
    owning_count: usize,
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
    owning: Vec<u32>,
    /// Which locals a closure captures, if any does.
    cells: Vec<bool>,
    /// The chunk and captures of each closure the function makes.
    closures: Vec<(usize, Vec<Capture>)>,
    operations: Vec<Operation>,
    matches: Vec<MatchSite>,
}

/// A `MatchCasePooled` [`native_match`] runs: its pattern, the slots of
/// the value and the pattern's operands, and how many values it pushes
/// when it matches, the value first.
pub(crate) struct MatchSite {
    pub(crate) pattern: MatchPatternId,
    pub(crate) operands: Vec<u32>,
    pub(crate) results: usize,
    /// A `TryMatchPooled`, which always continues: it pushes the bindings,
    /// or `nil` for each when the pattern fails, then whether it matched.
    pub(crate) tries: bool,
}

/// An instruction [`native_operation`] runs: its operands' slots, and
/// which of them it reads for the last time and so may move.
pub(crate) struct Operation {
    pub(crate) op: NativeOperation,
    pub(crate) operands: Vec<u32>,
    pub(crate) moves: Vec<bool>,
}

/// What [`Compiled`] is built from besides its code.
pub(crate) struct Tables {
    pub(crate) exits: Vec<ExitTable>,
    pub(crate) owning: Vec<u32>,
    pub(crate) cells: Vec<bool>,
    pub(crate) closures: Vec<(usize, Vec<Capture>)>,
    pub(crate) operations: Vec<Operation>,
    pub(crate) matches: Vec<MatchSite>,
}

impl Compiled {
    /// `entry` must be the address of finalized code generated for a
    /// function of `arity` parameters with these tables, which stays
    /// mapped for the life of the process.
    pub(crate) fn new(entry: *const u8, arity: usize, tables: Tables) -> Self {
        Self {
            entry: entry as usize,
            arity,
            exits: tables.exits,
            owning: tables.owning,
            cells: tables.cells,
            closures: tables.closures,
            operations: tables.operations,
            matches: tables.matches,
        }
    }

    /// Fresh cells for an activation: a captured parameter takes its
    /// argument from `arguments`, leaving `nil` there, and any other
    /// captured local starts `nil`.
    ///
    /// # Safety
    ///
    /// `arguments` must point to `arity` initialized values.
    unsafe fn fresh_cells(&self, arguments: *mut Value) -> Vec<NativeLocal> {
        self.cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                NativeLocal::Value(if *cell && index < self.arity {
                    // SAFETY: as the caller promises.
                    unsafe { std::ptr::replace(arguments.add(index), Value::Nil) }
                } else {
                    Value::Nil
                })
            })
            .collect()
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
            compiled: self,
            // SAFETY: as the caller promises.
            cells: unsafe { self.fresh_cells(arguments) },
            exits: self.exits.as_ptr(),
            exit_count: self.exits.len(),
            owning: self.owning.as_ptr(),
            owning_count: self.owning.len(),
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
    fn run(&self, vm: &mut Vm, closure: &Value, locals: &mut Vec<Value>) -> NativeOutcome {
        // The code takes the arguments; the other locals are the frame's
        // unset `nil`s, which it does not need.
        locals.truncate(self.arity);
        if locals.len() != self.arity {
            locals.resize(self.arity, Value::Nil);
        }
        stats::count(&stats::ENTRIES);
        let mut root = Root {
            base: 0,
            frames: Vec::new(),
            failure: None,
        };
        root.base = std::ptr::from_ref(&root) as usize;
        // SAFETY: everything passed lives across the call; the code moves
        // the arguments out of `locals`, so they are forgotten here rather
        // than dropped twice.
        let outcome = unsafe {
            let outcome = self.enter(
                std::ptr::from_mut(vm),
                std::ptr::from_ref(closure),
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
                    failure: root.failure,
                }
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Calls `callee`, or module-level name `name` when `callee` is null, with
/// the `count` arguments at `arguments`, which it takes, when it is a
/// compiled closure an exact positional call enters or a builtin: writes
/// its result to `out` and returns `CALL_RETURNED`. Returns `CALL_EXITED`
/// when the callee left to the interpreter, its frame recorded, or a
/// builtin failed, its failure recorded for the interpreter to report past
/// the call; and `CALL_DECLINED` when there was no such callee, the native
/// calls have used their stack budget, or the thread needs attention (a
/// cancelled fork, say), which the interpreter checks at every call.
/// A declined call drops its arguments but those `moved` marks, by bit,
/// which native code moved from its own slots and moves back.
pub(crate) extern "C" fn native_call(
    context: *mut Context,
    name: u32,
    count: u32,
    callee: *const Value,
    arguments: *mut Value,
    moved: u64,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, null or an
    // initialized callee, `count` initialized values it gives up, and a
    // slot holding no value that needs dropping. The VM is not otherwise
    // borrowed while native code runs.
    unsafe {
        let context = &*context;
        let root = context.root;
        let arguments_slice = std::slice::from_raw_parts_mut(arguments, count as usize);
        let target = {
            let vm = &*context.vm;
            if stack_used(&*root) < STACK_BUDGET
                && !vm.native_needs_attention()
                // An argument the interpreter must resolve first.
                && !arguments_slice
                    .iter()
                    .any(|value| matches!(value, Value::Binding { .. }))
            {
                if callee.is_null() {
                    vm.native_global_callee(&*context.closure, GlobalNameId::new(name))
                } else {
                    vm.native_value_callee(&*callee)
                }
            } else {
                None
            }
        };
        let declined = |arguments: &mut [Value]| {
            for (index, argument) in arguments.iter_mut().enumerate() {
                if index >= 64 || moved & (1 << index) == 0 {
                    std::ptr::drop_in_place(argument);
                }
            }
            CALL_DECLINED
        };
        let Some(target) = target else {
            return declined(arguments_slice);
        };
        if matches!(
            target,
            Value::Native(_) | Value::DeclaredNative { .. } | Value::Builtin(_)
        ) {
            let Some(invoked) = (*context.vm).native_invoke(&target, arguments_slice) else {
                stats::count(&stats::UNCOMPILED_CALLS);
                return declined(arguments_slice);
            };
            std::ptr::drop_in_place(arguments_slice);
            return match invoked {
                Ok(value) => {
                    out.write(value);
                    CALL_RETURNED
                }
                Err(failure) => {
                    (*root).failure = Some(failure);
                    CALL_EXITED
                }
            };
        }
        let Some((closure, code)) = (*context.vm).native_entry(target, count as usize) else {
            stats::count(&stats::UNCOMPILED_CALLS);
            return declined(arguments_slice);
        };
        let Some(compiled) = code.as_any().downcast_ref::<Compiled>() else {
            return declined(arguments_slice);
        };
        let outcome = compiled.enter(context.vm, &raw const closure, root, arguments);
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

/// [`native_binary`] whose left operand, which `left` holds, the
/// instruction reads for the last time: the VM takes it, and adds to or
/// takes from a collection nothing else holds in place.
pub(crate) extern "C" fn native_binary_moving(
    context: *mut Context,
    operator: u32,
    left: *mut Value,
    right: *const Value,
    out: *mut Value,
) -> u32 {
    let Some(operator) = BINARY_OPERATORS.get(operator as usize) else {
        return 1;
    };
    // SAFETY: generated code passes its own context, its own initialized
    // slot for `left`, an initialized `right`, and a free slot.
    unsafe {
        let vm = &*(*context).vm;
        produce(out, vm.native_binary_moving(*operator, &mut *left, &*right))
    }
}

/// Writes `value` to `out` and returns 0, or returns 1 for `None`, so
/// native code leaves before the instruction and the interpreter runs it.
///
/// # Safety
///
/// `out` must be a slot holding no value that needs dropping.
unsafe fn produce(out: *mut Value, value: Option<Value>) -> u32 {
    match value {
        Some(value) => {
            // SAFETY: as the caller promises.
            unsafe { out.write(value) };
            0
        }
        None => 1,
    }
}

/// Module-level name `name` as the running function reads it.
pub(crate) extern "C" fn native_global(context: *mut Context, name: u32, out: *mut Value) -> u32 {
    // SAFETY: generated code passes its own context and a free slot.
    unsafe {
        let context = &*context;
        let value = (*context.vm).native_global(&*context.closure, GlobalNameId::new(name));
        produce(out, value)
    }
}

/// Sets module variable `name` to a clone of `value`: 0, or 1 when the
/// interpreter must.
pub(crate) extern "C" fn native_set_global(
    context: *mut Context,
    name: u32,
    value: *const Value,
) -> u32 {
    // SAFETY: generated code passes its own context and an initialized
    // value.
    unsafe {
        let context = &*context;
        u32::from(!(*context.vm).native_set_global(
            &*context.closure,
            GlobalNameId::new(name),
            &*value,
        ))
    }
}

/// Capture `index` of the running function.
pub(crate) extern "C" fn native_capture(context: *mut Context, index: u32, out: *mut Value) -> u32 {
    // SAFETY: generated code passes its own context and a free slot.
    unsafe {
        let context = &*context;
        let value = (*context.vm).native_capture(&*context.closure, index as usize);
        produce(out, value)
    }
}

/// Where [`native_index`] finds its collection.
pub(crate) const SOURCE_VALUE: u32 = 0;
pub(crate) const SOURCE_CAPTURE: u32 = 1;
pub(crate) const SOURCE_GLOBAL: u32 = 2;
pub(crate) const SOURCE_CELL: u32 = 3;
/// [`native_index`]'s `constant` when the key is a value.
pub(crate) const NO_CONSTANT: u32 = u32::MAX;

/// An index read: the collection is `collection` for `SOURCE_VALUE`, or
/// capture, module-level name, or captured local `operand`; the key is `key`, or value
/// constant `constant` of the running function's chunk.
pub(crate) extern "C" fn native_index(
    context: *mut Context,
    source: u32,
    operand: u32,
    collection: *const Value,
    key: *const Value,
    constant: u32,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, initialized values
    // where `source` and `constant` say there are some, and a free slot.
    unsafe {
        let context = &*context;
        let (key, constant) = if constant == NO_CONSTANT {
            (Some(&*key), None)
        } else {
            (None, Some(constant as usize))
        };
        let read = |source: NativeReadSource<'_>| {
            (*context.vm).native_index(&*context.closure, source, key, constant)
        };
        let value = match source {
            SOURCE_VALUE => read(NativeReadSource::Value(&*collection)),
            SOURCE_CAPTURE => read(NativeReadSource::Capture(operand as usize)),
            SOURCE_CELL => (&context.cells)[operand as usize]
                .read(|collection| read(NativeReadSource::Value(collection))),
            _ => read(NativeReadSource::Global(GlobalNameId::new(operand))),
        };
        produce(out, value)
    }
}

/// `-value`, for a value that is not on the fast path.
pub(crate) extern "C" fn native_negate(
    context: *mut Context,
    value: *const Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, an initialized value,
    // and a free slot.
    unsafe { produce(out, (*(*context).vm).native_negate(&*value)) }
}

/// `~value`, for a value that is not an integer.
pub(crate) extern "C" fn native_bit_not(
    context: *mut Context,
    value: *const Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, an initialized value,
    // and a free slot.
    unsafe { produce(out, (*(*context).vm).native_bit_not(&*value)) }
}

/// `not value`, for a value that is not on the fast path.
pub(crate) extern "C" fn native_not(
    context: *mut Context,
    value: *const Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, an initialized value,
    // and a free slot.
    unsafe { produce(out, (*(*context).vm).native_not(&*value)) }
}

/// Constant `index` of the running function's chunk, for one that is not
/// a number, a boolean, or `nil`.
pub(crate) extern "C" fn native_constant(
    context: *mut Context,
    index: u32,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context and a free slot.
    unsafe {
        let context = &*context;
        let value = (*context.vm).native_constant(&*context.closure, index as usize);
        produce(out, value)
    }
}

/// A list of the `count` values at `values`, which it takes.
pub(crate) extern "C" fn native_list(
    context: *mut Context,
    count: u32,
    values: *mut Value,
    out: *mut Value,
) {
    // SAFETY: generated code passes its own context, `count` initialized
    // values it gives up, and a free slot.
    unsafe {
        let values = std::slice::from_raw_parts(values, count as usize);
        let values = values.iter().map(|value| std::ptr::read(value)).collect();
        out.write((*(*context).vm).native_list(values));
    }
}

/// Captured local `slot`'s value, as `GetLocal` reads it.
pub(crate) extern "C" fn native_get_cell(context: *mut Context, slot: u32, out: *mut Value) -> u32 {
    // SAFETY: generated code passes its own context, a captured local's
    // slot, and a free slot.
    unsafe { produce(out, Some((&(*context).cells)[slot as usize].get())) }
}

/// Sets captured local `slot` to a clone of `value`, as `SetLocal` does.
pub(crate) extern "C" fn native_set_cell(
    context: *mut Context,
    slot: u32,
    value: *const Value,
) -> u32 {
    // SAFETY: generated code passes its own context, a captured local's
    // slot, and an initialized value.
    unsafe { (&mut (*context).cells)[slot as usize].set((*value).clone()) };
    0
}

/// Sets capture `index` of the running function to a clone of `value`:
/// 0, or 1 when the interpreter must.
pub(crate) extern "C" fn native_set_capture(
    context: *mut Context,
    index: u32,
    value: *const Value,
) -> u32 {
    // SAFETY: generated code passes its own context and an initialized
    // value.
    unsafe {
        let context = &*context;
        u32::from(!(*context.vm).native_set_capture(&*context.closure, index as usize, &*value))
    }
}

/// The closure the function's closure `site` makes.
pub(crate) extern "C" fn native_closure(context: *mut Context, site: u32, out: *mut Value) -> u32 {
    // SAFETY: generated code passes its own context, a closure site it was
    // compiled with, and a free slot.
    unsafe {
        let context = &mut *context;
        let compiled = &*context.compiled;
        let (chunk, captures) = &compiled.closures[site as usize];
        let value =
            (*context.vm).native_closure(&*context.closure, *chunk, captures, &mut context.cells);
        produce(out, value)
    }
}

/// Runs the function's operation `site` on its operands in the frame at
/// `base`, moving those it reads for the last time and cloning the rest:
/// writes the result to `out` and returns 0, or returns 1 with each moved
/// operand back in its slot, for the interpreter to run it.
pub(crate) extern "C" fn native_operation(
    context: *mut Context,
    site: u32,
    base: *mut Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, an operation site it
    // was compiled with, its slots, each initialized, and a free slot.
    unsafe {
        let context = &*context;
        let compiled = &*context.compiled;
        let operation = &compiled.operations[site as usize];
        let slots = || operation.operands.iter().zip(&operation.moves);
        let operands = slots()
            .map(|(slot, moves)| {
                let slot = base.add(*slot as usize);
                if *moves {
                    std::ptr::replace(slot, Value::Nil)
                } else {
                    (*slot).clone()
                }
            })
            .collect();
        match (*context.vm).native_operation(&*context.closure, &operation.op, operands) {
            Ok(value) => {
                out.write(value);
                0
            }
            Err(operands) => {
                for ((slot, moves), operand) in slots().zip(operands) {
                    if *moves {
                        *base.add(*slot as usize) = operand;
                    }
                }
                1
            }
        }
    }
}

/// [`native_match`]'s status: the pattern matched, its results written;
/// it did not; or the interpreter must run it.
pub(crate) const MATCHED: u32 = 0;
pub(crate) const NOT_MATCHED: u32 = 1;

/// Runs the function's match `site` on the values in the frame at `base`,
/// writing the value and what the pattern binds to the slots from `out`
/// when it matches.
pub(crate) extern "C" fn native_match(
    context: *mut Context,
    site: u32,
    base: *const Value,
    out: *mut Value,
) -> u32 {
    // SAFETY: generated code passes its own context, a match site it was
    // compiled with, its slots, each initialized, and as many free slots
    // from `out` as the site has results.
    unsafe {
        let context = &*context;
        let compiled = &*context.compiled;
        let site = &compiled.matches[site as usize];
        let Some((value, operands)) = site.operands.split_first() else {
            return 2;
        };
        let value = &*base.add(*value as usize);
        let operands: Vec<Value> = operands
            .iter()
            .map(|slot| (*base.add(*slot as usize)).clone())
            .collect();
        let mut bindings = Vec::new();
        let matched = (*context.vm).native_match(
            &*context.closure,
            site.pattern,
            value,
            &operands,
            &mut bindings,
        );
        if site.tries {
            let Some(matched) = matched else {
                return 2;
            };
            let count = site.results - 1;
            if matched && bindings.len() != count {
                return 2;
            }
            bindings.resize(count, Value::Nil);
            for (index, binding) in bindings.into_iter().enumerate() {
                out.add(index).write(binding);
            }
            out.add(count).write(Value::Bool(matched));
            return MATCHED;
        }
        match matched {
            Some(true) if bindings.len() + 1 == site.results => {
                out.write(value.clone());
                for (index, binding) in bindings.into_iter().enumerate() {
                    out.add(index + 1).write(binding);
                }
                MATCHED
            }
            Some(false) => NOT_MATCHED,
            _ => 2,
        }
    }
}

/// Fresh cells for a `recur`, whose arguments are at `arguments`.
pub(crate) extern "C" fn native_recur_cells(context: *mut Context, arguments: *mut Value) {
    // SAFETY: generated code passes its own context and its `recur`'s
    // arguments, each initialized.
    unsafe {
        let context = &mut *context;
        context.cells = (*context.compiled).fresh_cells(arguments);
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

/// Drops what every owning slot of the frame at `base` holds, leaving
/// `nil`: one call in place of a check per slot, for a function with many.
pub(crate) extern "C" fn native_release(context: *mut Context, base: *mut Value) {
    // SAFETY: generated code passes its own context and its slots, each
    // initialized; the owning slots are its own.
    unsafe {
        let context = &*context;
        for slot in std::slice::from_raw_parts(context.owning, context.owning_count) {
            let slot = base.add(*slot as usize);
            // Most slots hold `nil` or a number, which need no drop; the
            // tag is the slot's first word (see `layout`).
            if slot.cast::<i64>().read() >= layout::FIRST_COUNTED {
                drop(std::ptr::replace(slot, Value::Nil));
            }
        }
    }
}

/// Records exit `exit`: clones the frame its table describes from the
/// slots at `base` into the context. Generated code then drops its slots
/// and returns `EXITED`.
pub(crate) extern "C" fn native_exit(context: *mut Context, exit: u32, base: *const Value) {
    // SAFETY: generated code passes its own context, an exit id it was
    // compiled with, and its slots, each initialized.
    stats::count(&stats::EXITS);
    unsafe {
        let context = &mut *context;
        let tables = std::slice::from_raw_parts(context.exits, context.exit_count);
        let table = &tables[exit as usize];
        let read = |slot: &u32| (*base.add(*slot as usize)).clone();
        let locals = table
            .locals
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                if *slot == CELL_LOCAL {
                    // The activation ends here: its cells move out.
                    std::mem::replace(&mut context.cells[index], NativeLocal::Value(Value::Nil))
                } else {
                    NativeLocal::Value(read(slot))
                }
            })
            .collect();
        context.exit = Some(Exited {
            ip: table.ip,
            locals,
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
