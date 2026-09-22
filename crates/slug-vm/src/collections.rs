//! Private immutable collection operations.
//!
//! `Value` still owns the compact reference-counted storage during the
//! collection redesign. This module is the only place that needs to know the
//! concrete backing containers for ordinary collection transformations.

use std::{ops::Deref, rc::Rc};

use crate::Value;

/// Canonical, hashable map-key representation.
///
/// Equal Slug map keys always produce the same variant and payload. This is
/// deliberately private until an indexed map consumes it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum MapKey {
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(Rc<str>),
    Bytes(Rc<Vec<u8>>),
}

impl MapKey {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "the range and integral-value checks make this conversion exact"
    )]
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Int(value) => Some(Self::Int(*value)),
            Value::Float(value) if value.is_nan() => None,
            Value::Float(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= i64::MIN as f64
                    && *value < -(i64::MIN as f64) =>
            {
                Some(Self::Int(*value as i64))
            }
            Value::Float(value) => Some(Self::Float(value.to_bits())),
            Value::Str(value) => Some(Self::Str(value.clone())),
            Value::Bytes(value) => Some(Self::Bytes(value.clone())),
            _ => None,
        }
    }
}

/// An immutable logical list backed by shared storage.
///
/// A list may expose a contiguous range of its backing vector.  The range is
/// deliberately private: callers can only observe it through logical list
/// operations, and every value-producing operation materializes an independent
/// result before it can be changed.
#[doc(hidden)]
#[derive(Clone)]
pub struct List {
    values: Rc<Vec<Value>>,
    start: usize,
    end: usize,
}

impl List {
    pub(crate) fn from_values(values: Vec<Value>) -> Self {
        let end = values.len();
        Self {
            values: Rc::new(values),
            start: 0,
            end,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.end - self.start
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Value> {
        self.values
            .get(self.start + index)
            .filter(|_| index < self.len())
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.values[self.start..self.end].iter()
    }

    pub(crate) fn view(&self, start: usize, end: usize) -> Self {
        assert!(start <= end && end <= self.len(), "list view is in bounds");
        Self {
            values: self.values.clone(),
            start: self.start + start,
            end: self.start + end,
        }
    }

    pub(crate) fn is_uniquely_owned(&self) -> bool {
        Rc::strong_count(&self.values) == 1
    }

    pub(crate) fn is_view(&self) -> bool {
        self.start != 0 || self.end != self.values.len()
    }

    pub(crate) fn concat(self, other: &Self) -> Self {
        let mut values = self.into_values();
        values.extend(other.iter().cloned());
        Self::from_values(values)
    }

    pub(crate) fn append(self, value: Value) -> Self {
        let mut values = self.into_values();
        values.push(value);
        Self::from_values(values)
    }

    pub(crate) fn prepend(self, value: Value) -> Self {
        let values = self.into_values();
        let mut result = Vec::with_capacity(values.len() + 1);
        result.push(value);
        result.extend(values);
        Self::from_values(result)
    }

    pub(crate) fn slice(&self, indexes: impl Iterator<Item = usize>) -> Self {
        Self::from_values(
            indexes
                .map(|index| self.get(index).unwrap().clone())
                .collect(),
        )
    }

    fn into_values(self) -> Vec<Value> {
        if self.start == 0 && self.end == self.values.len() {
            Rc::try_unwrap(self.values).unwrap_or_else(|values| (*values).clone())
        } else {
            self.iter().cloned().collect()
        }
    }
}

impl Deref for List {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        &self.values[self.start..self.end]
    }
}

impl PartialEq for List {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
mod list_tests {
    use super::List;
    use crate::Value;

    #[test]
    fn ranged_view_exposes_only_its_logical_elements() {
        let list = List::from_values(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let view = list.view(1, 3);

        assert_eq!(view.len(), 2);
        assert_eq!(view.get(0), Some(&Value::Int(2)));
        assert_eq!(view.get(2), None);
        assert_eq!(
            view.iter().cloned().collect::<Vec<_>>(),
            vec![Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn ranged_view_keeps_its_backing_storage_alive() {
        let list = List::from_values(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let view = list.view(1, 3);

        assert!(!list.is_uniquely_owned());
        assert!(!view.is_uniquely_owned());
    }
}

#[derive(Clone)]
pub(crate) struct Bytes(Rc<Vec<u8>>);

pub(crate) struct BytesView<'a>(&'a [u8]);

impl<'a> BytesView<'a> {
    pub(crate) fn new(values: &'a [u8]) -> Self {
        Self(values)
    }

    pub(crate) fn as_slice(&self) -> &'a [u8] {
        self.0
    }
}

impl Bytes {
    pub(crate) fn from_values(values: Vec<u8>) -> Self {
        Self(Rc::new(values))
    }

    pub(crate) fn from_shared(values: Rc<Vec<u8>>) -> Self {
        Self(values)
    }

    pub(crate) fn into_shared(self) -> Rc<Vec<u8>> {
        self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<u8> {
        self.0.get(index).copied()
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = u8> + '_ {
        self.0.iter().copied()
    }

    pub(crate) fn concat(self, other: &Self) -> Self {
        let mut values = self.into_values();
        values.extend(other.iter());
        Self(Rc::new(values))
    }

    pub(crate) fn append(self, value: u8) -> Self {
        let mut values = self.into_values();
        values.push(value);
        Self(Rc::new(values))
    }

    pub(crate) fn prepend(self, value: u8) -> Self {
        let values = self.into_values();
        let mut result = Vec::with_capacity(values.len() + 1);
        result.push(value);
        result.extend(values);
        Self(Rc::new(result))
    }

    pub(crate) fn slice(&self, indexes: impl Iterator<Item = usize>) -> Self {
        Self(Rc::new(indexes.map(|index| self.0[index]).collect()))
    }

    fn into_values(self) -> Vec<u8> {
        Rc::try_unwrap(self.0).unwrap_or_else(|values| (*values).clone())
    }
}

#[derive(Clone)]
pub(crate) struct Map(Rc<Vec<(Value, Value)>>);

pub(crate) struct MapView<'a>(&'a [(Value, Value)]);

impl<'a> MapView<'a> {
    pub(crate) fn new(entries: &'a [(Value, Value)]) -> Self {
        Self(entries)
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<(&'a Value, &'a Value)> {
        self.0.get(index).map(|(key, value)| (key, value))
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &'a (Value, Value)> {
        self.0.iter()
    }
}

impl Map {
    pub(crate) fn new(entries: Vec<(Value, Value)>) -> Self {
        Self(Rc::new(entries))
    }

    pub(crate) fn from_shared(entries: Rc<Vec<(Value, Value)>>) -> Self {
        Self(entries)
    }

    pub(crate) fn into_shared(self) -> Rc<Vec<(Value, Value)>> {
        self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &(Value, Value)> {
        self.0.iter()
    }

    pub(crate) fn get(&self, key: &Value) -> Option<&Value> {
        self.0
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value)
    }

    pub(crate) fn merge(self, other: &Self) -> Self {
        let mut entries = self.into_entries();
        for (key, value) in other.iter() {
            if let Some((_, existing)) = entries.iter_mut().find(|(candidate, _)| candidate == key)
            {
                *existing = value.clone();
            } else {
                entries.push((key.clone(), value.clone()));
            }
        }
        Self(Rc::new(entries))
    }

    pub(crate) fn remove(self, key: &Value) -> Self {
        let mut entries = self.into_entries();
        entries.retain(|(candidate, _)| candidate != key);
        Self(Rc::new(entries))
    }

    pub(crate) fn copy_string_fields(
        self,
        names: &[String],
        replacements: &[Value],
    ) -> Result<Self, String> {
        let mut entries = self.into_entries();
        for (index, (name, replacement)) in names.iter().zip(replacements).enumerate() {
            if names[..index].contains(name) {
                return Err(format!("duplicate map key '{name}'"));
            }
            let key = Value::string(name.clone());
            if let Some((_, existing)) = entries.iter_mut().find(|(candidate, _)| candidate == &key)
            {
                *existing = replacement.clone();
            } else {
                entries.push((key, replacement.clone()));
            }
        }
        Ok(Self(Rc::new(entries)))
    }

    pub(crate) fn equals(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key).is_some_and(|other| other == value))
    }

    fn into_entries(self) -> Vec<(Value, Value)> {
        Rc::try_unwrap(self.0).unwrap_or_else(|entries| (*entries).clone())
    }
}
