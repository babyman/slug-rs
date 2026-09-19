//! Operand-stack and frame-local access helpers.
//!
//! These helpers keep underflow, unresolved binding, and missing-local faults
//! tied to the active source span instead of exposing unchecked indexing.

use std::cmp::Ordering;

use crate::{
    SourceSpan, Value,
    value::{BindingCell, binding_cell},
};

use super::{RuntimeErrorKind, Vm, VmResult, frames::LocalSlot, numbers};

impl Vm {
    pub(super) fn pop_at(&mut self, span: Option<&SourceSpan>) -> VmResult<Value> {
        self.pop_unresolved_at(span)?
            .resolve()
            .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
    }

    pub(super) fn pop_unresolved_at(&mut self, span: Option<&SourceSpan>) -> VmResult<Value> {
        self.stack.pop().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }

    pub(super) fn pop_values_at(
        &mut self,
        count: usize,
        span: Option<&SourceSpan>,
    ) -> VmResult<Vec<Value>> {
        if self.stack.len() < count {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            ));
        }
        self.stack
            .split_off(self.stack.len() - count)
            .into_iter()
            .map(|value| {
                value
                    .resolve()
                    .map_err(|message| self.error_at(RuntimeErrorKind::Name, message, span))
            })
            .collect()
    }

    pub(super) fn peek_at(&self, span: Option<&SourceSpan>) -> VmResult<&Value> {
        self.stack.last().ok_or_else(|| {
            self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }

    pub(super) fn local_value(&self, slot: usize, span: Option<&SourceSpan>) -> VmResult<Value> {
        match self.frames.last().and_then(|frame| frame.locals.get(slot)) {
            Some(LocalSlot::Direct(value)) => Ok(value.clone()),
            Some(LocalSlot::Captured(cell)) => Ok(cell.borrow().clone()),
            None => Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
                span,
            )),
        }
    }

    pub(super) fn promote_local_at(
        &mut self,
        slot: usize,
        span: Option<&SourceSpan>,
    ) -> VmResult<BindingCell> {
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
                span,
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

    pub(super) fn set_local_at(
        &mut self,
        slot: usize,
        value: Value,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
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
                span,
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

    pub(super) fn pop_pair_at(&mut self, span: Option<&SourceSpan>) -> VmResult<(Value, Value)> {
        let right = self.pop_at(span)?;
        let left = self.pop_at(span)?;
        Ok((left, right))
    }

    pub(super) fn binary_at(
        &mut self,
        span: Option<&SourceSpan>,
        operation: fn(Value, Value) -> Result<Value, (RuntimeErrorKind, String)>,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair_at(span)?;
        self.stack.push(
            operation(left, right).map_err(|(kind, message)| self.error_at(kind, message, span))?,
        );
        Ok(())
    }

    pub(super) fn compare_at(
        &mut self,
        span: Option<&SourceSpan>,
        expected: Ordering,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair_at(span)?;
        let result = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            left.cmp(right) == expected
        } else {
            let (left, right) = numbers(left, right)
                .map_err(|message| self.error_at(RuntimeErrorKind::Type, message, span))?;
            left.partial_cmp(&right)
                .is_some_and(|ordering| ordering == expected)
        };
        self.stack.push(Value::Bool(result));
        Ok(())
    }

    pub(super) fn guard_compare_at(
        &mut self,
        span: Option<&SourceSpan>,
        expected: Ordering,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair_at(span)?;
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
