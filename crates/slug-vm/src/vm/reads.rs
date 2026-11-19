//! Index reads that look inside a variable's value where it is.
//!
//! `players[id]` reads `players`, pushing a new reference to its map, indexes
//! it, and drops that reference again. Each reference is an atomic count on
//! the map, so forks on other cores reading the same map, as every page's
//! drawing reads the world, take turns at the cache line that holds it, and
//! more forks run slower. `ReadIndex` and `ReadIndexConstant` index the
//! value in its local, capture, or module binding without that reference;
//! only the element read gains one.

use crate::{
    Constant, Value,
    bytecode::{GlobalNameId, MOVE_LOCAL_OPERAND, PackedInstruction, PackedOpcode},
    value::UNRESOLVED_GLOBAL,
};

#[cfg(feature = "concurrency")]
use crate::variables::{BindingKind, VarKey};

use super::{Program, RuntimeErrorKind, Vm, VmResult, frames::LocalSlot, operations::index_value};

impl Vm {
    /// Runs a `ReadIndex` or `ReadIndexConstant`: operand `b` says whether
    /// the collection is local, capture, or module binding `a`, and `c` is
    /// the constant key of a `ReadIndexConstant`.
    #[inline(never)]
    pub(super) fn read_index(
        &mut self,
        program: &Program,
        instruction: &PackedInstruction,
    ) -> VmResult<()> {
        let popped;
        let chunk = self.current_chunk(program)?;
        let (key, hint) = if instruction.opcode == PackedOpcode::ReadIndexConstant {
            let constant = instruction.c as usize;
            let Some(Constant::Value(key)) = chunk.constants.get(constant) else {
                return Err(self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    alloc::format!("index constant {constant} does not exist"),
                ));
            };
            (key, chunk.index_hints.get(constant))
        } else {
            popped = self.pop()?;
            (&popped, None)
        };
        // At a local's last read the value moves out of its slot, as
        // `GetLocal` moves it, so that what it held is no longer shared.
        if instruction.b == MOVE_LOCAL_OPERAND {
            let collection = self.take_local(instruction.a as usize)?;
            let value = self.index_read(&collection, key, hint);
            drop(collection);
            let value = value.map_err(|(kind, message)| self.error_at(kind, message))?;
            self.stack.push(value);
            return Ok(());
        }
        let value = self
            .with_read_source(program, instruction.b, instruction.a, |collection| {
                self.index_read(collection, key, hint)
            })?
            .map_err(|(kind, message)| self.error_at(kind, message))?;
        self.stack.push(value);
        Ok(())
    }

    /// `collection[key]`, through the entry position `hint` for a constant
    /// key into a map. A variable can hold a module binding, as a
    /// destructured import holds the exporter's live one; the binding's
    /// value is what is indexed, as it is when popped from the stack.
    fn index_read(
        &self,
        collection: &Value,
        key: &Value,
        hint: Option<&crate::sync::Hint>,
    ) -> Result<Value, (RuntimeErrorKind, alloc::string::String)> {
        if matches!(collection, Value::Binding { .. }) {
            let resolved = collection
                .resolve_in(self)
                .map_err(|message| (RuntimeErrorKind::Name, message))?;
            return self.index_read(&resolved, key, hint);
        }
        if let (Value::Map(map), Some(hint)) = (collection, hint) {
            #[cfg(feature = "metrics")]
            {
                self.metrics.borrow_mut().collection_lookups += 1;
            }
            return Ok(map.get_hinted(key, hint).cloned().unwrap_or(Value::Nil));
        }
        index_value(
            collection,
            key,
            #[cfg(feature = "metrics")]
            &self.metrics,
        )
    }

    /// Calls `read` with the value of local, capture, or module binding `a`
    /// as `kind` 0, 1, or 2 says, without taking a reference to it where it
    /// can be reached in place.
    fn with_read_source<R>(
        &self,
        program: &Program,
        kind: u32,
        a: u32,
        read: impl FnOnce(&Value) -> R,
    ) -> VmResult<R> {
        let slot = a as usize;
        match kind {
            0 => match self.frames.last().and_then(|frame| frame.locals.get(slot)) {
                Some(LocalSlot::Direct(value)) => Ok(read(value)),
                Some(LocalSlot::Captured(cell)) => Ok(read(&cell.borrow())),
                None => Err(self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    alloc::format!("local {slot} does not exist"),
                )),
            },
            1 => {
                let cell = self
                    .frames
                    .last()
                    .and_then(|frame| frame.closure.captures.get(slot))
                    .ok_or_else(|| {
                        self.error_at(
                            RuntimeErrorKind::InvalidBytecode,
                            alloc::format!("capture {slot} does not exist"),
                        )
                    })?;
                Ok(read(&cell.borrow()))
            }
            _ => self.with_global(program, GlobalNameId::new(a), read),
        }
    }

    /// Calls `read` with the value of module binding `id` as `global_value`
    /// would give it, in place when it is a `val`, a variable's own cell, or
    /// a fork's snapshot of a variable, and otherwise as `global_value`
    /// gives it.
    fn with_global<R>(
        &self,
        program: &Program,
        id: GlobalNameId,
        read: impl FnOnce(&Value) -> R,
    ) -> VmResult<R> {
        let slot = self
            .frames
            .last()
            .and_then(|frame| frame.closure.global_slots.as_ref())
            .and_then(|slots| slots.get(id.index()))
            .map_or(UNRESOLVED_GLOBAL, crate::sync::Hint::get);
        if slot != UNRESOLVED_GLOBAL {
            let globals = self.globals.borrow();
            match globals.value(slot) {
                Value::Binding { cell, name } => {
                    #[cfg(feature = "concurrency")]
                    let own = match self.view.as_deref() {
                        None => true,
                        Some(view) => match name.kind() {
                            BindingKind::Var if view.in_fork() => {
                                if let Some(value) = view.snapshot_var(VarKey::cell(cell)) {
                                    return Ok(read(value));
                                }
                                false
                            }
                            BindingKind::Val | BindingKind::Var => true,
                            BindingKind::TaskVar => false,
                        },
                    };
                    #[cfg(not(feature = "concurrency"))]
                    let own = {
                        let _ = name;
                        true
                    };
                    if own {
                        let value = cell.borrow();
                        if !matches!(*value, Value::Uninitialized) {
                            return Ok(read(&value));
                        }
                    }
                }
                value => return Ok(read(value)),
            }
        }
        let value = self.global_value(program, id)?;
        Ok(read(&value))
    }
}
