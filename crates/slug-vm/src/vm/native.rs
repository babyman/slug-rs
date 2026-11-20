//! Operations for the native tier's generated code.
//!
//! Native code runs a function without a VM frame of its own, so each
//! operation here takes the running closure, and reads its module through
//! that closure's own globals and slot cache, as the interpreter would with
//! the closure's frame on top. Each gives `None` wherever the interpreter
//! would do more than read: native code then leaves to the interpreter
//! before the instruction, which runs it and reports any error with its
//! trace.

use alloc::{string::String, vec::Vec};

use crate::{
    Capture, Constant, MatchPatternId, Op, Value,
    bytecode::{GlobalNameId, GlobalUpdate, StructFieldsId},
    collections::{List, Map, is_map_key},
    native::NativeInvocation,
    tiering::{NativeCell, NativeFailure, NativeLocal},
    value::{UNRESOLVED_GLOBAL, binding_cell},
};

use super::{
    Vm,
    operations::{
        bit_not, construct_struct, copy_value, list_append, list_prepend, matches_pattern, negate,
        slice_value,
    },
    read_binding,
    updates::{copy_cannot_fail, update_cannot_fail},
};

/// Where an index read finds its collection.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum NativeReadSource<'a> {
    /// A value native code holds.
    Value(&'a Value),
    /// The running closure's capture.
    Capture(usize),
    /// A module-level name, as the running closure reads it.
    Global(GlobalNameId),
}

/// The function a builtin or typed native `callee` runs, and the typed
/// native's resource signature.
fn native_callable(
    callee: &Value,
) -> Option<(
    &crate::NativeFunction,
    Option<&crate::ForeignResourceSignature>,
)> {
    match callee {
        Value::Native(function) => Some((function, None)),
        Value::DeclaredNative {
            function,
            declaration,
        } => Some((function, Some(&declaration.resources))),
        _ => None,
    }
}

/// The slot of module-level name `name` in `closure`'s module: the one
/// its slot cache holds, or else the one its name has, which the cache
/// then holds, as the interpreter's read by name caches it. A write by
/// name does not, so code that only writes a variable would otherwise
/// never find its slot.
pub(super) fn native_slot(closure: &crate::value::Closure, name: GlobalNameId) -> Option<u32> {
    let cached = closure.global_slots.as_ref()?.get(name.index())?;
    let slot = cached.get();
    if slot != UNRESOLVED_GLOBAL {
        return Some(slot);
    }
    let slot = closure
        .globals
        .borrow()
        .slot(closure.program.global_name(name)?)?;
    cached.set(slot);
    Some(slot)
}

/// An instruction [`Vm::native_operation`] runs: the part of its [`Op`]
/// that says what it does, which native code keeps.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum NativeOperation {
    ListAppend,
    ListPrepend,
    GetSlice {
        has_start: bool,
        has_end: bool,
        has_step: bool,
    },
    Map(usize),
    Struct(Vec<String>),
    StructPooled(StructFieldsId),
    StructCopy(Vec<String>),
    StructCopyPooled(StructFieldsId),
    CopyGlobalPooled {
        name: GlobalNameId,
        fields: StructFieldsId,
    },
    UpdateCapture {
        slot: usize,
        operator: GlobalUpdate,
    },
}

impl NativeOperation {
    /// The operation `op` is, if it is one.
    #[must_use]
    pub fn of(op: &Op) -> Option<Self> {
        Some(match op {
            Op::ListAppend => Self::ListAppend,
            Op::ListPrepend => Self::ListPrepend,
            Op::GetSlice {
                has_start,
                has_end,
                has_step,
            } => Self::GetSlice {
                has_start: *has_start,
                has_end: *has_end,
                has_step: *has_step,
            },
            Op::Map(count) => Self::Map(*count),
            Op::Struct(names) => Self::Struct(names.clone()),
            Op::StructPooled(id) => Self::StructPooled(*id),
            Op::StructCopy(names) => Self::StructCopy(names.clone()),
            Op::StructCopyPooled(id) => Self::StructCopyPooled(*id),
            Op::CopyGlobalPooled { name, fields } => Self::CopyGlobalPooled {
                name: *name,
                fields: *fields,
            },
            Op::UpdateCapture { slot, operator } => Self::UpdateCapture {
                slot: *slot,
                operator: *operator,
            },
            _ => return None,
        })
    }
}

impl Vm {
    /// Module-level name `name` as code in `caller` reads it: `None` when
    /// `caller` has not resolved the name yet or it is uninitialized, which
    /// the interpreter resolves or reports.
    #[doc(hidden)]
    #[must_use]
    pub fn native_global(&self, caller: &Value, name: GlobalNameId) -> Option<Value> {
        self.with_native_global(caller, name, Value::clone)
    }

    /// Calls `read` with module-level name `name` as code in `caller` reads
    /// it, in place where it can be, as `global_value` reads it otherwise.
    fn with_native_global<R>(
        &self,
        caller: &Value,
        name: GlobalNameId,
        read: impl FnOnce(&Value) -> R,
    ) -> Option<R> {
        let Value::Closure(closure) = caller else {
            return None;
        };
        let slot = native_slot(closure, name)?;
        let globals = closure.globals.borrow();
        match globals.value(slot) {
            Value::Binding { cell, name } => {
                // A task with its own view of the variables reads one
                // through it, as `global_value` does.
                #[cfg(feature = "concurrency")]
                if self.view.is_some()
                    && let Some(value) = self.viewed_binding(cell, name).ok()?
                {
                    return Some(read(&value));
                }
                #[cfg(not(feature = "concurrency"))]
                let _ = name;
                let value = cell.borrow();
                if matches!(*value, Value::Uninitialized) {
                    return None;
                }
                Some(read(&value))
            }
            value => Some(read(value)),
        }
    }

    /// Sets module variable `name` to a clone of `value`, as code in
    /// `caller` sets it, and returns whether it did. It does not when the
    /// task has its own view of the variables, which decides what a write
    /// means, or `caller` has not resolved the name.
    #[doc(hidden)]
    #[must_use]
    pub fn native_set_global(&self, caller: &Value, name: GlobalNameId, value: &Value) -> bool {
        #[cfg(feature = "concurrency")]
        if self.view.is_some() {
            return false;
        }
        let Value::Closure(closure) = caller else {
            return false;
        };
        let Some(slot) = native_slot(closure, name) else {
            return false;
        };
        let globals = closure.globals.borrow();
        globals.value(slot).replace_binding(value.clone())
    }

    /// Capture `index` of `caller`, as `GetCapture` reads it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_capture(&self, caller: &Value, index: usize) -> Option<Value> {
        let Value::Closure(closure) = caller else {
            return None;
        };
        Some(closure.captures.get(index)?.borrow().clone())
    }

    /// `collection[key]` for `GetIndex`, `GetIndexConstant`, `ReadIndex`,
    /// and `ReadIndexConstant` in `caller`: the key is `key`, or else value
    /// constant `constant` of `caller`'s chunk, read through its index
    /// hint as the interpreter reads it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_index(
        &self,
        caller: &Value,
        source: NativeReadSource<'_>,
        key: Option<&Value>,
        constant: Option<usize>,
    ) -> Option<Value> {
        let resolved;
        let (key, hint) = match (key, constant) {
            (Some(key), None) => {
                // A popped key is resolved, as `pop` resolves it.
                if matches!(key, Value::Binding { .. }) {
                    resolved = key.resolve_in(self).ok()?;
                    (&resolved, None)
                } else {
                    (key, None)
                }
            }
            (None, Some(constant)) => {
                let Value::Closure(closure) = caller else {
                    return None;
                };
                let chunk = closure.program.chunk(closure.chunk)?;
                let Some(Constant::Value(key)) = chunk.constants.get(constant) else {
                    return None;
                };
                (key, chunk.index_hints.get(constant))
            }
            _ => return None,
        };
        let read = |collection: &Value| self.index_read(collection, key, hint).ok();
        match source {
            NativeReadSource::Value(collection) => read(collection),
            NativeReadSource::Capture(index) => {
                let Value::Closure(closure) = caller else {
                    return None;
                };
                read(&closure.captures.get(index)?.borrow())
            }
            NativeReadSource::Global(name) => self.with_native_global(caller, name, read)?,
        }
    }

    /// `-value`, as `Negate` computes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_negate(&self, value: &Value) -> Option<Value> {
        negate(value.resolve_in(self).ok()?).ok()
    }

    /// `~value`, as `BitNot` computes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_bit_not(&self, value: &Value) -> Option<Value> {
        bit_not(&value.resolve_in(self).ok()?).ok()
    }

    /// `not value`, as `Not` computes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_not(&self, value: &Value) -> Option<Value> {
        Some(Value::Bool(!value.resolve_in(self).ok()?.is_truthy()))
    }

    /// Sets capture `index` of `caller` to a clone of `value`, as
    /// `SetCapture` does, and returns whether it did. It does not when the
    /// task has its own view of the variables, which decides whether the
    /// write is allowed.
    #[doc(hidden)]
    #[must_use]
    pub fn native_set_capture(&self, caller: &Value, index: usize, value: &Value) -> bool {
        #[cfg(feature = "concurrency")]
        if self.view.is_some() {
            return false;
        }
        let Value::Closure(closure) = caller else {
            return false;
        };
        let Some(capture) = closure.captures.get(index) else {
            return false;
        };
        *capture.borrow_mut() = value.clone();
        true
    }

    /// A closure of chunk `chunk` capturing `captures` from `caller`, whose
    /// frame's locals are `locals`, as `MakeClosure` makes it: a captured
    /// local becomes a cell its closures share, if it is not one already.
    #[doc(hidden)]
    #[must_use]
    pub fn native_closure(
        &self,
        caller: &Value,
        chunk: usize,
        captures: &[Capture],
        locals: &mut [NativeLocal],
    ) -> Option<Value> {
        let Value::Closure(closure) = caller else {
            return None;
        };
        closure.program.chunk(chunk)?;
        let cells = captures
            .iter()
            .map(|capture| match capture {
                Capture::Local(slot) => {
                    let local = locals.get_mut(*slot)?;
                    Some(match local {
                        NativeLocal::Value(value) => {
                            let cell = binding_cell(core::mem::replace(value, Value::Nil));
                            #[cfg(feature = "metrics")]
                            self.record_local_cell();
                            *local = NativeLocal::Cell(NativeCell(cell.clone()));
                            cell
                        }
                        NativeLocal::Cell(cell) => cell.0.clone(),
                    })
                }
                Capture::Capture(slot) => closure.captures.get(*slot).cloned(),
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Value::Closure(crate::sync::Code::new(
            crate::value::Closure {
                chunk,
                captures: cells,
                program: closure.program.clone(),
                globals: closure.globals.clone(),
                global_slots: Some(
                    closure
                        .global_slots
                        .clone()
                        .unwrap_or_else(|| crate::value::global_slots(&closure.program)),
                ),
                #[cfg(feature = "concurrency")]
                capture_sources: captures.to_vec(),
                #[cfg(feature = "concurrency")]
                domain: self.domain(),
            },
        )))
    }

    /// Constant `index` of `caller`'s chunk, as `Constant` pushes it: a
    /// function constant becomes a closure with no captures in `caller`'s
    /// module.
    #[doc(hidden)]
    #[must_use]
    pub fn native_constant(&self, caller: &Value, index: usize) -> Option<Value> {
        let Value::Closure(closure) = caller else {
            return None;
        };
        let chunk = match closure.program.chunk(closure.chunk)?.constants.get(index)? {
            Constant::Value(value) => return Some(value.clone()),
            Constant::Function(chunk) => *chunk,
        };
        Some(Value::Closure(crate::sync::Code::new(
            crate::value::Closure {
                chunk,
                captures: Vec::new(),
                program: closure.program.clone(),
                globals: closure.globals.clone(),
                global_slots: Some(
                    closure
                        .global_slots
                        .clone()
                        .unwrap_or_else(|| crate::value::global_slots(&closure.program)),
                ),
                #[cfg(feature = "concurrency")]
                capture_sources: Vec::new(),
                #[cfg(feature = "concurrency")]
                domain: self.domain(),
            },
        )))
    }

    /// A list of `values`, as `List` makes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_list(&self, values: Vec<Value>) -> Value {
        #[cfg(feature = "metrics")]
        self.record_collection_construction(values.len());
        Value::List(List::from_values(values))
    }

    /// An instruction that builds a value from its operands, popped bottom
    /// first, as the interpreter runs it: `ListAppend`, `ListPrepend`,
    /// `GetSlice`, `Struct`, `StructCopy`, `CopyGlobal`, and
    /// `UpdateCapture`, for code in `caller`. Gives the operands back,
    /// untouched, wherever the interpreter would fail or do more, so that
    /// native code leaves before the instruction with them.
    ///
    /// The operands are its own: an operand nothing else holds is updated
    /// in place where the interpreter would update it, and a copy or update
    /// that cannot fail lets go of the variable's value first, as the
    /// interpreter does, so that the operand is all that holds it.
    #[doc(hidden)]
    pub fn native_operation(
        &self,
        caller: &Value,
        op: &NativeOperation,
        operands: Vec<Value>,
    ) -> Result<Value, Vec<Value>> {
        use NativeOperation as Op;
        let Value::Closure(closure) = caller else {
            return Err(operands);
        };
        // A popped binding is resolved first, which the interpreter does.
        if operands
            .iter()
            .any(|operand| matches!(operand, Value::Binding { .. }))
        {
            return Err(operands);
        }
        let program = &closure.program;
        let fields = |id| program.struct_fields(id);
        match op {
            Op::ListAppend | Op::ListPrepend => {
                self.native_append(matches!(op, Op::ListAppend), operands)
            }
            Op::GetSlice {
                has_start,
                has_end,
                has_step,
            } => self.native_slice([*has_start, *has_end, *has_step], operands),
            Op::Map(count) => self.native_map(*count, operands),
            Op::Struct(_) | Op::StructPooled(_) => {
                let names = match op {
                    Op::Struct(names) => Some(names.as_slice()),
                    Op::StructPooled(id) => fields(*id),
                    _ => None,
                };
                let Some(names) = names else {
                    return Err(operands);
                };
                let built = construct_struct(operands[0].clone(), names, &operands[1..]);
                #[cfg(feature = "metrics")]
                if built.is_ok() {
                    self.record_collection_construction(operands.len() - 1);
                }
                built.map_err(|_| operands)
            }
            Op::StructCopy(_) | Op::StructCopyPooled(_) => {
                let names = match op {
                    Op::StructCopy(names) => Some(names.as_slice()),
                    Op::StructCopyPooled(id) => fields(*id),
                    _ => None,
                };
                let Some(names) = names else {
                    return Err(operands);
                };
                if operands.is_empty() {
                    return Err(operands);
                }
                #[cfg(feature = "metrics")]
                self.record_copy(&operands[0]);
                Self::native_copy(names, operands)
            }
            Op::CopyGlobalPooled { name, fields: id } => {
                let (Some(cell), Some(names)) = (self.native_variable(caller, *name), fields(*id))
                else {
                    return Err(operands);
                };
                if operands.is_empty() {
                    return Err(operands);
                }
                if copy_cannot_fail(&operands[0], names) {
                    // Dropped once the cell is no longer borrowed.
                    drop(cell.replace(Value::Nil));
                }
                #[cfg(feature = "metrics")]
                self.record_copy(&operands[0]);
                let value = Self::native_copy(names, operands)?;
                drop(cell.replace(value.clone()));
                Ok(value)
            }
            Op::UpdateCapture { slot, operator } => {
                #[cfg(feature = "concurrency")]
                if self.view.is_some() {
                    return Err(operands);
                }
                let Some(cell) = closure.captures.get(*slot).cloned() else {
                    return Err(operands);
                };
                let [left, right] = <[Value; 2]>::try_from(operands)?;
                let value = if update_cannot_fail(*operator, &left, &right) {
                    drop(cell.replace(Value::Nil));
                    // It cannot fail, as checked.
                    self.apply_update(*operator, left, right)
                        .map_err(|_| Vec::new())?
                } else {
                    match self.apply_update(*operator, left.clone(), right.clone()) {
                        Ok(value) => value,
                        Err(_) => return Err(Vec::from([left, right])),
                    }
                };
                drop(cell.replace(value.clone()));
                Ok(value)
            }
        }
    }

    /// `list :+ value` or, unless `append`, `value +: list`, of `operands`
    /// in that order, unless it can fail.
    fn native_append(&self, append: bool, operands: Vec<Value>) -> Result<Value, Vec<Value>> {
        let [first, second] = <[Value; 2]>::try_from(operands)?;
        let (list, value) = if append {
            (&first, &second)
        } else {
            (&second, &first)
        };
        if !update_cannot_fail(GlobalUpdate::Append, list, value) {
            return Err(Vec::from([first, second]));
        }
        #[cfg(feature = "metrics")]
        if append {
            self.record_list_append(list);
        } else if let Value::List(values) = list {
            self.record_list_view_materialization(values);
            self.record_collection_update(values.len(), values.is_uniquely_owned());
        }
        let result = if append {
            list_append(first, second)
        } else {
            list_prepend(first, second)
        };
        // It cannot fail, as checked.
        result.map_err(|_| Vec::new())
    }

    /// A map of the `count` key and value pairs in `operands`, as `Map`
    /// makes it, unless a key is not one or repeats.
    fn native_map(&self, count: usize, operands: Vec<Value>) -> Result<Value, Vec<Value>> {
        if operands.len() != count.saturating_mul(2)
            || operands.chunks_exact(2).any(|pair| !is_map_key(&pair[0]))
        {
            return Err(operands);
        }
        let entries = operands
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();
        match Map::from_literal(entries) {
            Ok(map) => {
                #[cfg(feature = "metrics")]
                self.record_collection_construction(count);
                Ok(Value::Map(map))
            }
            Err(_) => Err(operands),
        }
    }

    /// Matches `value` against pattern `pattern` of `caller`'s program,
    /// with `operands`, as `MatchCase` does, pushing what it binds onto
    /// `bindings`: whether it matched, or `None` where the interpreter
    /// must run it, which reports any error.
    #[doc(hidden)]
    #[must_use]
    pub fn native_match(
        &self,
        caller: &Value,
        pattern: MatchPatternId,
        value: &Value,
        operands: &[Value],
        bindings: &mut Vec<Value>,
    ) -> Option<bool> {
        let Value::Closure(closure) = caller else {
            return None;
        };
        // A popped binding is resolved first, which the interpreter does.
        if core::iter::once(value)
            .chain(operands)
            .any(|value| matches!(value, Value::Binding { .. }))
        {
            return None;
        }
        let pattern = closure.program.match_pattern(pattern)?;
        let mut list_views_created = 0;
        #[cfg(feature = "concurrency")]
        let view = self.view.as_deref();
        #[cfg(not(feature = "concurrency"))]
        let view = None;
        let matched = matches_pattern(
            pattern,
            value,
            operands,
            bindings,
            &mut list_views_created,
            &|cell, name| read_binding(view, cell, name),
        )
        .ok()?;
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().list_views_created += list_views_created;
        }
        if !matched {
            bindings.clear();
        }
        Some(matched)
    }

    /// `operands[0]` sliced by the start, end, and step that follow it,
    /// those `present`.
    fn native_slice(&self, present: [bool; 3], operands: Vec<Value>) -> Result<Value, Vec<Value>> {
        if operands.is_empty() {
            return Err(operands);
        }
        let mut rest = operands[1..].iter();
        let [start, end, step] = present.map(|present| if present { rest.next() } else { None });
        let sliced = slice_value(operands[0].clone(), start, end, step);
        #[cfg(feature = "metrics")]
        if sliced.is_ok() {
            self.record_collection_slice();
        }
        sliced.map_err(|_| operands)
    }

    /// `operands[0]`, which there is, copied with `names` replaced by the
    /// rest, as `StructCopy` copies it: in place when nothing else holds it
    /// and the copy cannot fail.
    fn native_copy(names: &[String], mut operands: Vec<Value>) -> Result<Value, Vec<Value>> {
        if copy_cannot_fail(&operands[0], names) {
            let base = core::mem::replace(&mut operands[0], Value::Nil);
            // It cannot fail, as checked.
            return copy_value(base, names, &operands[1..]).map_err(|_| Vec::new());
        }
        match copy_value(operands[0].clone(), names, &operands[1..]) {
            Ok(value) => Ok(value),
            Err(_) => Err(operands),
        }
    }

    /// The cell of module variable `name` as code in `caller` resolved it,
    /// outside a task with its own view of the variables.
    fn native_variable(
        &self,
        caller: &Value,
        name: GlobalNameId,
    ) -> Option<crate::value::BindingCell> {
        #[cfg(feature = "concurrency")]
        if self.view.is_some() {
            return None;
        }
        let Value::Closure(closure) = caller else {
            return None;
        };
        let slot = native_slot(closure, name)?;
        match closure.globals.borrow().value(slot) {
            Value::Binding { cell, .. } => Some(cell.clone()),
            _ => None,
        }
    }

    /// Calls `callee` with `arguments` when it is a builtin or a typed
    /// native, which native code may call itself: its result, or the
    /// failure the interpreter is to report with [`Vm::native_raise`], once
    /// native code has left with the caller's frame past the call. `None`
    /// when the interpreter must make the call, before anything ran:
    /// `callee` is neither, a fork may not call it, or the arguments are
    /// not the resources its declaration names, which the interpreter
    /// reports.
    #[doc(hidden)]
    pub fn native_invoke(
        &mut self,
        callee: &Value,
        arguments: &[Value],
    ) -> Option<Result<Value, NativeFailure>> {
        // `int` has no effects, so one the interpreter must report it
        // makes again.
        if let Value::Builtin(crate::value::Builtin::Int) = callee {
            return match arguments {
                [Value::Int(value)] => Some(Ok(Value::Int(*value))),
                [Value::Float(value)] => {
                    super::float_to_int(*value).map(|value| Ok(Value::Int(value)))
                }
                _ => None,
            };
        }
        let (function, signature) = native_callable(callee)?;
        #[cfg(feature = "concurrency")]
        if self.view.is_some() && self.check_fork_native(function).is_err() {
            return None;
        }
        if let Some(signature) = signature
            && self
                .validate_foreign_resource_arguments(function, signature, arguments, None)
                .is_err()
        {
            return None;
        }
        let failure = |invocation| NativeFailure {
            callee: callee.clone(),
            invocation,
        };
        Some(match self.start_native(function, arguments) {
            // A result the interpreter would accept: anything else fails
            // the same way when it is reported.
            NativeInvocation::Result(value, resources)
                if !value.contains_non_finite_number()
                    && signature.is_none_or(|signature| {
                        self.validate_foreign_resource_result(function, signature, &value, None)
                            .is_ok()
                    }) =>
            {
                self.settle_native(
                    function,
                    NativeInvocation::Result(value, resources),
                    signature,
                    None,
                )
                .map_err(|_| {
                    failure(NativeInvocation::ContractViolation(alloc::format!(
                        "native `{}` could not be settled",
                        function.qualified_name()
                    )))
                })
            }
            invocation => Err(failure(invocation)),
        })
    }

    /// The error a builtin's failed call reports, in the frame on top,
    /// which native code left past the call.
    pub(super) fn native_raise(&mut self, failure: NativeFailure) -> crate::RuntimeError {
        let NativeFailure { callee, invocation } = failure;
        let settled = match native_callable(&callee) {
            Some((function, signature)) => {
                self.settle_native(function, invocation, signature, None)
            }
            None => Ok(Value::Nil),
        };
        match settled {
            Err(error) => error,
            Ok(_) => self.error_at(
                crate::RuntimeErrorKind::InvalidBytecode,
                "native code reported a builtin's result as its failure".into(),
            ),
        }
    }
}
