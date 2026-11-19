//! Operations for the native tier's generated code.
//!
//! Native code runs a function without a VM frame of its own, so each
//! operation here takes the running closure, and reads its module through
//! that closure's own globals and slot cache, as the interpreter would with
//! the closure's frame on top. Each gives `None` wherever the interpreter
//! would do more than read: native code then leaves to the interpreter
//! before the instruction, which runs it and reports any error with its
//! trace.

use alloc::vec::Vec;

use crate::{
    Constant, Value, bytecode::GlobalNameId, collections::List, value::UNRESOLVED_GLOBAL,
};

use super::{Vm, operations::negate};

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
        let slot = closure.global_slots.as_ref()?.get(name.index())?.get();
        if slot == UNRESOLVED_GLOBAL {
            return None;
        }
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

    /// `not value`, as `Not` computes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_not(&self, value: &Value) -> Option<Value> {
        Some(Value::Bool(!value.resolve_in(self).ok()?.is_truthy()))
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
        Some(Value::Closure(crate::sync::Code::new(crate::value::Closure {
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
        })))
    }

    /// A list of `values`, as `List` makes it.
    #[doc(hidden)]
    #[must_use]
    pub fn native_list(&self, values: Vec<Value>) -> Value {
        #[cfg(feature = "metrics")]
        self.record_collection_construction(values.len());
        Value::List(List::from_values(values))
    }
}
