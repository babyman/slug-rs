//! Private immutable collection operations.
//!
//! `Value` still owns the compact reference-counted storage during the
//! collection redesign. This module is the only place that needs to know the
//! concrete backing containers for ordinary collection transformations.

use std::rc::Rc;

use crate::Value;

#[derive(Clone)]
pub(crate) struct List(Rc<Vec<Value>>);

pub(crate) struct ListView<'a>(&'a [Value]);

impl<'a> ListView<'a> {
    pub(crate) fn new(values: &'a [Value]) -> Self {
        Self(values)
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&'a Value> {
        self.0.get(index)
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &'a Value> {
        self.0.iter()
    }
}

impl List {
    pub(crate) fn from_shared(values: Rc<Vec<Value>>) -> Self {
        Self(values)
    }

    pub(crate) fn into_shared(self) -> Rc<Vec<Value>> {
        self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Value> {
        self.0.get(index)
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.0.iter()
    }

    pub(crate) fn concat(self, other: &Self) -> Self {
        let mut values = (*self.0).clone();
        values.extend(other.iter().cloned());
        Self(Rc::new(values))
    }

    pub(crate) fn append(self, value: Value) -> Self {
        let mut values = (*self.0).clone();
        values.push(value);
        Self(Rc::new(values))
    }

    pub(crate) fn prepend(self, value: Value) -> Self {
        let mut values = Vec::with_capacity(self.len() + 1);
        values.push(value);
        values.extend(self.iter().cloned());
        Self(Rc::new(values))
    }

    pub(crate) fn slice(&self, indexes: impl Iterator<Item = usize>) -> Self {
        Self(Rc::new(
            indexes
                .map(|index| self.0[index].clone())
                .collect::<Vec<_>>(),
        ))
    }
}

#[derive(Clone)]
pub(crate) struct Bytes(Rc<[u8]>);

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
        Self(values.into())
    }

    pub(crate) fn from_shared(values: Rc<[u8]>) -> Self {
        Self(values)
    }

    pub(crate) fn into_shared(self) -> Rc<[u8]> {
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
        let mut values = self.0.to_vec();
        values.extend(other.iter());
        Self(values.into())
    }

    pub(crate) fn append(self, value: u8) -> Self {
        let mut values = self.0.to_vec();
        values.push(value);
        Self(values.into())
    }

    pub(crate) fn prepend(self, value: u8) -> Self {
        let mut values = Vec::with_capacity(self.len() + 1);
        values.push(value);
        values.extend(self.iter());
        Self(values.into())
    }

    pub(crate) fn slice(&self, indexes: impl Iterator<Item = usize>) -> Self {
        Self(
            indexes
                .map(|index| self.0[index])
                .collect::<Vec<_>>()
                .into(),
        )
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
        let mut entries = (*self.0).clone();
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
        Self(Rc::new(
            self.0
                .iter()
                .filter(|(candidate, _)| candidate != key)
                .cloned()
                .collect(),
        ))
    }

    pub(crate) fn copy_string_fields(
        self,
        names: &[String],
        replacements: &[Value],
    ) -> Result<Self, String> {
        let mut entries = (*self.0).clone();
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
}
