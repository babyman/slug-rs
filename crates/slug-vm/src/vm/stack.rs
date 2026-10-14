//! Operand-stack and frame-local access helpers.
//!
//! These helpers keep underflow, unresolved binding, and missing-local faults
//! tied to the active source span instead of exposing unchecked indexing.

use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;

use crate::{
    Value,
    value::{BindingCell, binding_cell},
};

use super::{RuntimeErrorKind, Vm, VmResult, frames::LocalSlot, numbers, operations::compare_num};

impl Vm {
    #[inline]
    pub(super) fn pop(&mut self) -> VmResult<Value> {
        match self.stack.pop() {
            Some(value) if !matches!(value, Value::Binding { .. }) => Ok(value),
            value => self.pop_slow(value),
        }
    }

    /// Resolves a popped module binding, or reports an empty stack.
    #[cold]
    #[inline(never)]
    fn pop_slow(&self, value: Option<Value>) -> VmResult<Value> {
        value
            .ok_or_else(|| {
                self.error_at(RuntimeErrorKind::InvalidBytecode, "stack underflow".into())
            })?
            .into_resolved()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message))
    }

    #[inline]
    pub(super) fn pop_unresolved(&mut self) -> VmResult<Value> {
        self.stack.pop().ok_or_else(|| self.stack_underflow())
    }

    #[cold]
    #[inline(never)]
    fn stack_underflow(&self) -> super::RuntimeError {
        self.error_at(RuntimeErrorKind::InvalidBytecode, "stack underflow".into())
    }

    pub(super) fn pop_values(&mut self, count: usize) -> VmResult<Vec<Value>> {
        if self.stack.len() < count {
            return Err(self.error_at(RuntimeErrorKind::InvalidBytecode, "stack underflow".into()));
        }
        self.stack
            .split_off(self.stack.len() - count)
            .into_iter()
            .map(|value| {
                value
                    .into_resolved()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message))
            })
            .collect()
    }

    #[inline]
    pub(super) fn peek(&self) -> VmResult<&Value> {
        self.stack.last().ok_or_else(|| self.stack_underflow())
    }

    #[inline]
    pub(super) fn local_value(&self, slot: usize) -> VmResult<Value> {
        match self.frames.last().and_then(|frame| frame.locals.get(slot)) {
            Some(LocalSlot::Direct(value)) => Ok(value.clone()),
            Some(LocalSlot::Captured(cell)) => Ok(cell.borrow().clone()),
            None => Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
            )),
        }
    }

    /// Moves a direct local out after its last read, leaving `nil` behind.
    /// A captured slot is a cell shared with closures, so it is cloned.
    #[inline]
    pub(super) fn take_local(&mut self, slot: usize) -> VmResult<Value> {
        match self
            .frames
            .last_mut()
            .and_then(|frame| frame.locals.get_mut(slot))
        {
            Some(LocalSlot::Direct(value)) => Ok(core::mem::replace(value, Value::Nil)),
            Some(LocalSlot::Captured(cell)) => Ok(cell.borrow().clone()),
            None => Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
            )),
        }
    }

    pub(super) fn promote_local(&mut self, slot: usize) -> VmResult<BindingCell> {
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
            ));
        }
        let local = self
            .frames
            .last_mut()
            .and_then(|frame| frame.locals.get_mut(slot))
            .expect("local slot was checked");
        match local {
            LocalSlot::Direct(value) => {
                let cell = binding_cell(value.clone());
                *local = LocalSlot::Captured(cell.clone());
                #[cfg(feature = "metrics")]
                self.record_local_cell();
                Ok(cell)
            }
            LocalSlot::Captured(cell) => Ok(cell.clone()),
        }
    }

    pub(super) fn set_local(&mut self, slot: usize, value: Value) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
            ));
        }
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
            ));
        }
        match &mut self
            .frames
            .last_mut()
            .expect("active frame was checked")
            .locals[slot]
        {
            LocalSlot::Direct(local) => *local = value,
            LocalSlot::Captured(cell) => *cell.borrow_mut() = value,
        }
        Ok(())
    }

    #[inline]
    pub(super) fn pop_pair(&mut self) -> VmResult<(Value, Value)> {
        let right = self.pop()?;
        let left = self.pop()?;
        Ok((left, right))
    }

    pub(super) fn binary(
        &mut self,
        operation: fn(Value, Value) -> Result<Value, (RuntimeErrorKind, String)>,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair()?;
        self.stack
            .push(operation(left, right).map_err(|(kind, message)| self.error_at(kind, message))?);
        Ok(())
    }

    pub(super) fn compare(&mut self, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair()?;
        let result = self.compare_values(left, right, expected)?;
        self.stack.push(Value::Bool(result));
        Ok(())
    }

    pub(super) fn compare_values(
        &self,
        left: Value,
        right: Value,
        expected: Ordering,
    ) -> VmResult<bool> {
        if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            return Ok(left.cmp(right) == expected);
        }
        let (left, right) = numbers(left, right)
            .map_err(|message| self.error_at(RuntimeErrorKind::Type, message))?;
        Ok(left
            .partial_cmp(&right)
            .is_some_and(|ordering| ordering == expected))
    }

    pub(super) fn numeric_compare(&mut self, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair()?;
        let result = self.numeric_compare_values(left, right, expected)?;
        self.stack.push(Value::Bool(result));
        Ok(())
    }

    pub(super) fn numeric_compare_values(
        &self,
        left: Value,
        right: Value,
        expected: Ordering,
    ) -> VmResult<bool> {
        compare_num(left, right, expected)
            .map_err(|message| self.error_at(RuntimeErrorKind::Type, message))
    }

    pub(super) fn guard_compare(&mut self, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair()?;
        let result = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            left.cmp(right) == expected
        } else {
            numbers(left, right)
                .ok()
                .and_then(|(left, right)| left.partial_cmp(&right))
                .is_some_and(|ordering| ordering == expected)
        };
        self.stack.push(Value::Bool(result));
        Ok(())
    }
}
