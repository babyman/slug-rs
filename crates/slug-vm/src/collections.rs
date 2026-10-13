//! Private immutable collection operations.
//!
//! `Value` still owns the compact reference-counted storage during the
//! collection redesign. This module is the only place that needs to know the
//! concrete backing containers for ordinary collection transformations.

use alloc::{rc::Rc, string::String, vec, vec::Vec};
use core::hash::{BuildHasher, Hash, Hasher};

use crate::platform::MapHasher;
use core::ops::Deref;

use crate::Value;

/// Canonical, hashable map-key representation.
///
/// Equal Slug map keys always produce the same variant and payload, so equal
/// keys hash identically in a map's index.
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
        clippy::float_cmp,
        reason = "the range and integral-value checks make this conversion exact"
    )]
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Int(value) => Some(Self::Int(*value)),
            Value::Float(value)
                if value.is_finite()
                    && *value >= i64::MIN as f64
                    && *value < -(i64::MIN as f64) =>
            {
                let integer = *value as i64;
                ((integer as f64) == *value).then_some(Self::Int(integer))
            }
            Value::Float(value) if value.is_finite() => Some(Self::Float(value.to_bits())),
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
    // `u32` bounds keep `Value` at 24 bytes. A list cannot reach 2^32
    // elements: at 24 bytes each its allocation would fail first.
    start: u32,
    end: u32,
}

impl List {
    pub(crate) fn from_values(values: Vec<Value>) -> Self {
        let end = u32::try_from(values.len()).expect("list length fits in u32");
        Self {
            values: Rc::new(values),
            start: 0,
            end,
        }
    }

    fn range(&self) -> core::ops::Range<usize> {
        self.start as usize..self.end as usize
    }

    fn spans_storage(&self) -> bool {
        self.start == 0 && self.end as usize == self.values.len()
    }

    pub(crate) fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Value> {
        self.values
            .get(self.start as usize + index)
            .filter(|_| index < self.len())
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.values[self.range()].iter()
    }

    pub(crate) fn view(&self, start: usize, end: usize) -> Self {
        assert!(start <= end && end <= self.len(), "list view is in bounds");
        // Both offsets are bounded by this list's `u32` length.
        let offset = |index: usize| self.start + u32::try_from(index).expect("bounded by len");
        Self {
            values: self.values.clone(),
            start: offset(start),
            end: offset(end),
        }
    }

    #[cfg(any(feature = "metrics", test))]
    pub(crate) fn is_uniquely_owned(&self) -> bool {
        Rc::strong_count(&self.values) == 1
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn is_view(&self) -> bool {
        !self.spans_storage()
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
        if self.spans_storage() {
            Rc::try_unwrap(self.values).unwrap_or_else(|values| (*values).clone())
        } else {
            self.iter().cloned().collect()
        }
    }
}

impl Deref for List {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        &self.values[self.range()]
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

/// Maps with more entries than this keep a hash index beside their entries.
/// Up to it, a linear scan of the contiguous entries is as fast as hashing
/// the key, and copying a shared map does not also copy an index. Measured
/// crossovers: about 20 entries for unique-owner updates, 32-48 for lookups.
const INDEX_THRESHOLD: usize = 32;

/// An immutable, unordered Slug map.
///
/// Keys are unique under Slug key equality (see `MapKey`); constructing a map
/// from entries that repeat a key keeps the last value for it, as a literal or
/// merge evaluated in source order would. Entry order is a storage detail and
/// is never observable as map semantics.
#[doc(hidden)]
#[derive(Clone)]
pub struct Map(Rc<MapStorage>);

#[derive(Clone)]
struct MapStorage {
    entries: Vec<(Value, Value)>,
    /// Present once the map is larger than `INDEX_THRESHOLD`.
    index: Option<MapIndex>,
}

impl MapStorage {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            index: None,
        }
    }

    fn position(&self, key: &Value) -> Option<usize> {
        match &self.index {
            Some(index) => index.find(&self.entries, key),
            None => self
                .entries
                .iter()
                .position(|(candidate, _)| candidate == key),
        }
    }

    /// Sets `key` to `value`, replacing an existing entry for an equal key.
    fn insert(&mut self, key: Value, value: Value) {
        if let Some(position) = self.position(&key) {
            self.entries[position].1 = value;
            return;
        }
        self.entries.push((key, value));
        match &mut self.index {
            Some(index) => index.insert(&self.entries, self.entries.len() - 1),
            None if self.entries.len() > INDEX_THRESHOLD => {
                self.index = Some(MapIndex::build(&self.entries));
            }
            None => {}
        }
    }

    fn remove(&mut self, key: &Value) {
        let Some(position) = self.position(key) else {
            return;
        };
        if let Some(index) = &mut self.index {
            index.remove(&self.entries, position);
        }
        self.entries.swap_remove(position);
    }
}

/// An open-addressing table of entry positions for a large map.
///
/// The table stores only `u32` positions into the map's entries and hashes a
/// key's canonical form (see `MapKey`) on demand, so it never duplicates a
/// key. Linear probing keeps the load factor at most one half, and removal
/// shifts later entries back instead of leaving tombstones.
#[derive(Clone)]
struct MapIndex {
    hasher: MapHasher,
    slots: Vec<u32>,
}

impl MapIndex {
    const EMPTY: u32 = u32::MAX;

    fn build(entries: &[(Value, Value)]) -> Self {
        let mut index = Self {
            hasher: MapHasher::default(),
            slots: Vec::new(),
        };
        index.rebuild(entries, Self::capacity_for(entries.len()));
        index
    }

    fn capacity_for(len: usize) -> usize {
        (len * 2).next_power_of_two().max(64)
    }

    fn rebuild(&mut self, entries: &[(Value, Value)], capacity: usize) {
        self.slots = vec![Self::EMPTY; capacity];
        for position in 0..entries.len() {
            self.place(entries, position);
        }
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// The slot a key's probe sequence starts at, or `None` for a value that
    /// cannot be a map key.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "only the low bits select a slot"
    )]
    fn home(&self, key: &Value) -> Option<usize> {
        let mut hasher = self.hasher.build_hasher();
        match MapKey::from_value(key)? {
            MapKey::Bool(value) => (0u8, value).hash(&mut hasher),
            MapKey::Int(value) => (1u8, value).hash(&mut hasher),
            MapKey::Float(bits) => (2u8, bits).hash(&mut hasher),
            MapKey::Str(value) => (3u8, &*value).hash(&mut hasher),
            MapKey::Bytes(value) => (4u8, value.as_slice()).hash(&mut hasher),
        }
        Some(hasher.finish() as usize & self.mask())
    }

    fn find(&self, entries: &[(Value, Value)], key: &Value) -> Option<usize> {
        let mut slot = self.home(key)?;
        loop {
            let position = self.slots[slot];
            if position == Self::EMPTY {
                return None;
            }
            if entries[position as usize].0 == *key {
                return Some(position as usize);
            }
            slot = (slot + 1) & self.mask();
        }
    }

    /// Indexes `entries[position]`, whose key is not yet indexed.
    fn insert(&mut self, entries: &[(Value, Value)], position: usize) {
        if entries.len() * 2 > self.slots.len() {
            self.rebuild(entries, self.slots.len() * 2);
        } else {
            self.place(entries, position);
        }
    }

    fn place(&mut self, entries: &[(Value, Value)], position: usize) {
        let mut slot = self
            .home(&entries[position].0)
            .expect("map keys are validated");
        while self.slots[slot] != Self::EMPTY {
            slot = (slot + 1) & self.mask();
        }
        self.slots[slot] = u32::try_from(position).expect("map length fits in u32");
    }

    /// The slot holding `position`, whose key is `entries[position].0`.
    fn slot_of(&self, entries: &[(Value, Value)], position: usize) -> usize {
        let mut slot = self
            .home(&entries[position].0)
            .expect("map keys are validated");
        while self.slots[slot] as usize != position {
            slot = (slot + 1) & self.mask();
        }
        slot
    }

    /// Unindexes `entries[position]` ahead of `entries.swap_remove(position)`,
    /// which moves the last entry into `position`.
    fn remove(&mut self, entries: &[(Value, Value)], position: usize) {
        let mut hole = self.slot_of(entries, position);
        let mut next = hole;
        loop {
            next = (next + 1) & self.mask();
            let candidate = self.slots[next];
            if candidate == Self::EMPTY {
                break;
            }
            let start = self
                .home(&entries[candidate as usize].0)
                .expect("map keys are validated");
            // An entry may fill the hole unless its probe starts cyclically
            // in (hole, next], in which case moving it would break its probe.
            let stays = if hole <= next {
                hole < start && start <= next
            } else {
                hole < start || start <= next
            };
            if !stays {
                self.slots[hole] = candidate;
                hole = next;
            }
        }
        self.slots[hole] = Self::EMPTY;
        let last = entries.len() - 1;
        if position != last {
            let moved = self.slot_of(entries, last);
            self.slots[moved] = u32::try_from(position).expect("map length fits in u32");
        }
    }
}

pub(crate) struct MapView<'a>(&'a [(Value, Value)]);

impl<'a> MapView<'a> {
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

impl From<Vec<(Value, Value)>> for Map {
    fn from(entries: Vec<(Value, Value)>) -> Self {
        Self::new(entries)
    }
}

impl Map {
    /// Builds a map from entries in source order; a repeated key keeps its
    /// last value.
    pub(crate) fn new(entries: Vec<(Value, Value)>) -> Self {
        let mut storage = MapStorage::with_capacity(entries.len());
        for (key, value) in entries {
            storage.insert(key, value);
        }
        Self(Rc::new(storage))
    }

    /// Builds a map from entries whose keys are already distinct, such as a
    /// subset of another map's entries.
    pub(crate) fn from_unique(entries: Vec<(Value, Value)>) -> Self {
        let index = (entries.len() > INDEX_THRESHOLD).then(|| MapIndex::build(&entries));
        Self(Rc::new(MapStorage { entries, index }))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.entries.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &(Value, Value)> {
        self.0.entries.iter()
    }

    pub(crate) fn view(&self) -> MapView<'_> {
        MapView(&self.0.entries)
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn is_uniquely_owned(&self) -> bool {
        Rc::strong_count(&self.0) == 1
    }

    /// How many entries a lookup of `key` compares: one probe for an indexed
    /// map, otherwise the entries scanned up to and including a match.
    #[cfg(feature = "metrics")]
    pub(crate) fn lookup_cost(&self, key: &Value) -> usize {
        if self.0.index.is_some() {
            1
        } else {
            self.0
                .position(key)
                .map_or(self.len(), |position| position + 1)
        }
    }

    pub(crate) fn get(&self, key: &Value) -> Option<&Value> {
        self.0
            .position(key)
            .map(|position| &self.0.entries[position].1)
    }

    pub(crate) fn merge(self, other: &Self) -> Self {
        let mut storage = self.into_storage();
        for (key, value) in other.iter() {
            storage.insert(key.clone(), value.clone());
        }
        Self(Rc::new(storage))
    }

    pub(crate) fn remove(self, key: &Value) -> Self {
        if self.0.position(key).is_none() {
            return self;
        }
        let mut storage = self.into_storage();
        storage.remove(key);
        Self(Rc::new(storage))
    }

    pub(crate) fn copy_string_fields(
        self,
        names: &[String],
        replacements: &[Value],
    ) -> Result<Self, String> {
        for (index, name) in names.iter().enumerate() {
            if names[..index].contains(name) {
                return Err(format!("duplicate map key '{name}'"));
            }
        }
        let mut storage = self.into_storage();
        for (name, replacement) in names.iter().zip(replacements) {
            storage.insert(Value::string(name.clone()), replacement.clone());
        }
        Ok(Self(Rc::new(storage)))
    }

    pub(crate) fn equals(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key).is_some_and(|other| other == value))
    }

    fn into_storage(self) -> MapStorage {
        Rc::try_unwrap(self.0).unwrap_or_else(|storage| (*storage).clone())
    }
}

#[cfg(test)]
mod map_tests {
    use super::{INDEX_THRESHOLD, Map};
    use crate::Value;

    fn int_map(count: i64) -> Map {
        Map::new(
            (0..count)
                .map(|key| (Value::Int(key), Value::Int(key * 10)))
                .collect(),
        )
    }

    #[test]
    fn indexes_only_maps_larger_than_the_threshold() {
        let threshold = i64::try_from(INDEX_THRESHOLD).unwrap();
        assert!(int_map(threshold).0.index.is_none());
        assert!(int_map(threshold + 1).0.index.is_some());
    }

    #[test]
    fn indexed_lookup_uses_numeric_key_equivalence() {
        let map = int_map(64);
        assert_eq!(map.get(&Value::Float(3.0)), Some(&Value::Int(30)));
        assert_eq!(map.get(&Value::Float(-0.0)), Some(&Value::Int(0)));
        assert_eq!(map.get(&Value::Float(3.5)), None);
        assert_eq!(map.get(&Value::list(vec![Value::Int(3)])), None);
    }

    #[test]
    fn repeated_keys_keep_the_last_value_with_and_without_an_index() {
        for count in [4, 64] {
            let mut entries: Vec<_> = (0..count)
                .map(|key| (Value::Int(key), Value::Int(key)))
                .collect();
            entries.push((Value::Float(1.0), Value::string("last")));
            let map = Map::new(entries);
            assert_eq!(map.len(), usize::try_from(count).unwrap());
            assert_eq!(map.get(&Value::Int(1)), Some(&Value::string("last")));
        }
    }

    #[test]
    fn removal_keeps_every_remaining_key_reachable() {
        let mut map = int_map(64);
        for key in [0, 63, 7, 8, 20] {
            map = map.remove(&Value::Int(key));
            assert_eq!(map.get(&Value::Int(key)), None);
        }
        assert_eq!(map.len(), 59);
        for key in (0..64).filter(|key| ![0, 63, 7, 8, 20].contains(key)) {
            assert_eq!(map.get(&Value::Int(key)), Some(&Value::Int(key * 10)));
        }
    }

    #[test]
    fn merge_across_the_threshold_replaces_and_adds_keys() {
        let threshold = i64::try_from(INDEX_THRESHOLD).unwrap();
        let base = int_map(threshold - 2);
        assert!(base.0.index.is_none());
        let merged = base.merge(&Map::new(
            (threshold - 4..threshold + 4)
                .map(|key| (Value::Int(key), Value::Int(-key)))
                .collect(),
        ));
        assert_eq!(merged.len(), INDEX_THRESHOLD + 4);
        assert!(merged.0.index.is_some());
        assert_eq!(merged.get(&Value::Int(0)), Some(&Value::Int(0)));
        assert_eq!(
            merged.get(&Value::Int(threshold - 3)),
            Some(&Value::Int(3 - threshold))
        );
        assert_eq!(
            merged.get(&Value::Int(threshold + 3)),
            Some(&Value::Int(-threshold - 3))
        );
    }

    #[test]
    fn matches_a_reference_model_across_random_inserts_and_removals() {
        // A linear congruential generator keeps the sequence reproducible.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |bound: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % bound
        };
        let key = |n: u64| match n % 4 {
            0 => Value::Int(i64::try_from(n).unwrap()),
            // Integral floats must find the integer keys and vice versa.
            1 => Value::Float(f64::from(u32::try_from(n - 1).unwrap())),
            2 => Value::string(format!("key-{n}")),
            _ => Value::Bytes(std::rc::Rc::new(n.to_le_bytes().to_vec())),
        };
        let mut map = Map::new(Vec::new());
        let mut model: Vec<(Value, Value)> = Vec::new();
        for step in 0..20_000_i64 {
            let candidate = key(next(400));
            match next(3) {
                0 | 1 => {
                    map = map.merge(&Map::new(vec![(candidate.clone(), Value::Int(step))]));
                    match model
                        .iter_mut()
                        .find(|(existing, _)| *existing == candidate)
                    {
                        Some((_, value)) => *value = Value::Int(step),
                        None => model.push((candidate, Value::Int(step))),
                    }
                }
                _ => {
                    map = map.remove(&candidate);
                    model.retain(|(existing, _)| *existing != candidate);
                }
            }
            assert_eq!(map.len(), model.len());
            if step % 97 == 0 {
                for (key, value) in &model {
                    assert_eq!(map.get(key), Some(value));
                }
            }
        }
        assert!(map.0.index.is_some());
    }

    #[test]
    fn shared_maps_are_copied_before_an_update() {
        let original = int_map(64);
        let updated = original
            .clone()
            .merge(&Map::new(vec![(Value::Int(0), Value::Nil)]));
        assert_eq!(original.get(&Value::Int(0)), Some(&Value::Int(0)));
        assert_eq!(updated.get(&Value::Int(0)), Some(&Value::Nil));
    }
}
