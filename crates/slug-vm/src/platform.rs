//! Small host-library substitutions used by the VM implementation.
//!
//! Desktop builds retain the hash collections from `std`.  Freestanding builds
//! use ordered collections supplied by `alloc`, avoiding an additional runtime
//! dependency while preserving the VM's internal collection contract.

#[cfg(not(target_os = "none"))]
pub(crate) use std::collections::{HashMap, HashSet};

#[cfg(target_os = "none")]
pub(crate) use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet};

/// A string-keyed map for global names. Names are short identifiers looked up
/// on every global read, so FNV-1a is used instead of the default `SipHash`,
/// whose flood resistance buys nothing for compiler-chosen keys.
#[cfg(not(target_os = "none"))]
pub(crate) type NameMap<V> =
    HashMap<alloc::string::String, V, core::hash::BuildHasherDefault<Fnv1a>>;

#[cfg(target_os = "none")]
pub(crate) type NameMap<V> = HashMap<alloc::string::String, V>;

/// Seeds the hash index of large maps. Map keys can come from untrusted
/// input, so hosted builds keep std's randomly keyed `SipHash`; freestanding
/// builds have no randomness source and use FNV-1a.
#[cfg(not(target_os = "none"))]
pub(crate) type MapHasher = std::collections::hash_map::RandomState;

#[cfg(target_os = "none")]
pub(crate) type MapHasher = core::hash::BuildHasherDefault<Fnv1a>;

/// Hashes the address of a module variable's cell, the key of every task's
/// view of the variables, read on each access to a `task var` or, in a
/// fork, a `var`. The VM chooses these keys, never input, and they are
/// distinct, so one multiply spreads them; `SipHash` buys nothing here.
#[derive(Default)]
#[cfg_attr(target_os = "none", allow(dead_code))]
pub(crate) struct AddressHasher(u64);

impl core::hash::Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        // The multiply leaves aligned addresses' low bits zero, and the
        // table picks a bucket from the low bits: fold the high bits down.
        self.0 ^ (self.0 >> 32)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}

pub(crate) struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl core::hash::Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}
