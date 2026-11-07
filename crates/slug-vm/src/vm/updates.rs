//! Assignments to module-level names that update a collection in place.
//!
//! `players = players + {[id]: player}` reads `players`, so while the merge
//! runs the binding and the operand share one map, and the merge copies it.
//! A local's last read moves the value out of its slot instead (see
//! `bytecode::liveness`); a module-level binding stays readable by every
//! function in the module, so its reference is dropped only here, within the
//! one instruction that also assigns the result to it. Nothing can observe
//! the binding meanwhile, and the old value would be overwritten at once
//! anyway, so it is dropped early only when the operation cannot fail: a
//! failure leaves the binding as it was.

use alloc::string::String;

use crate::{
    Value,
    bytecode::{GlobalNameId, GlobalUpdate, StructFieldsId},
    value::UNRESOLVED_GLOBAL,
};

use super::{
    Program, RuntimeErrorKind, Vm, VmResult,
    operations::{add, copy_value, is_map_key, list_append, subtract},
};

impl Vm {
    /// Applies `operator` to the two values on the stack, assigns the result
    /// to the module-level `id`, and pushes it.
    #[inline(never)]
    pub(super) fn update_global(
        &mut self,
        program: &Program,
        id: GlobalNameId,
        operator: GlobalUpdate,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair()?;
        let infallible = match (operator, &left, &right) {
            (GlobalUpdate::Add, Value::Map(_), Value::Map(_))
            | (GlobalUpdate::Add, Value::List(_), Value::List(_))
            | (GlobalUpdate::Add, Value::Str(_), _)
            | (GlobalUpdate::Append, Value::List(_), _) => true,
            (GlobalUpdate::Subtract, Value::Map(_), key) => is_map_key(key),
            _ => false,
        };
        if infallible {
            self.release_global(id);
        }
        let value = match operator {
            GlobalUpdate::Add => {
                #[cfg(feature = "metrics")]
                self.record_add(&left, &right);
                add(left, right).map_err(|(kind, message)| self.error_at(kind, message))?
            }
            GlobalUpdate::Subtract => {
                #[cfg(feature = "metrics")]
                self.record_subtract(&left);
                subtract(left, right).map_err(|(kind, message)| self.error_at(kind, message))?
            }
            GlobalUpdate::Append => {
                #[cfg(feature = "metrics")]
                self.record_list_append(&left);
                list_append(left, right)
                    .map_err(|message| self.error_at(RuntimeErrorKind::Type, message))?
            }
        };
        self.assign_global(program, id, value)
    }

    /// Copies the struct or map under the stack's `fields` replacements with
    /// them, assigns the copy to the module-level `id`, and pushes it.
    #[inline(never)]
    pub(super) fn copy_global(
        &mut self,
        program: &Program,
        id: GlobalNameId,
        fields: StructFieldsId,
    ) -> VmResult<()> {
        let fields = program
            .struct_fields(fields)
            .expect("validated struct field metadata");
        let replacements = self.pop_values(fields.len())?;
        let value = self.pop()?;
        if copy_cannot_fail(&value, fields) {
            self.release_global(id);
        }
        #[cfg(feature = "metrics")]
        self.record_copy(&value);
        let value = copy_value(value, fields, &replacements)
            .map_err(|message| self.error_at(RuntimeErrorKind::Type, message))?;
        self.assign_global(program, id, value)
    }

    /// Drops the value of the module-level `id`, which the instruction is
    /// about to replace, so that an operand it shared with is no longer
    /// shared. A binding the active closure has not resolved yet keeps its
    /// value: the instruction then only assigns it.
    fn release_global(&self, id: GlobalNameId) {
        // Dropped once no binding is borrowed.
        drop(self.replace_cached_global(id, Value::Nil));
    }

    /// Assigns `value` to the module-level `id` and pushes it, through the
    /// binding the active closure has resolved, or else by name.
    fn assign_global(&mut self, program: &Program, id: GlobalNameId, value: Value) -> VmResult<()> {
        match self.replace_cached_global(id, value.clone()) {
            Ok(old) => drop(old),
            Err(value) => {
                self.stack.push(value);
                self.set_global(program, id)?;
            }
        }
        self.stack.push(value);
        Ok(())
    }

    /// Replaces the value of the module-level binding `id` when the active
    /// closure has resolved its slot, returning the old value; otherwise
    /// hands `value` back.
    fn replace_cached_global(&self, id: GlobalNameId, value: Value) -> Result<Value, Value> {
        let slot = self
            .frames
            .last()
            .and_then(|frame| frame.closure.global_slots.as_ref())
            .and_then(|slots| slots.get(id.index()))
            .map_or(UNRESOLVED_GLOBAL, core::cell::Cell::get);
        if slot == UNRESOLVED_GLOBAL {
            return Err(value);
        }
        match self.globals.borrow().value(slot) {
            Value::Binding { cell, .. } => Ok(cell.replace(value)),
            _ => Err(value),
        }
    }
}

/// Whether copying `value` with `names` replaced always succeeds: the names
/// are distinct and, for a struct, all its fields.
fn copy_cannot_fail(value: &Value, names: &[String]) -> bool {
    let distinct = names
        .iter()
        .enumerate()
        .all(|(index, name)| !names[..index].contains(name));
    distinct
        && match value {
            Value::Map(_) => true,
            Value::Struct(value) => names.iter().all(|name| {
                value
                    .schema
                    .fields
                    .iter()
                    .any(|field| field.name.as_ref() == name)
            }),
            _ => false,
        }
}
