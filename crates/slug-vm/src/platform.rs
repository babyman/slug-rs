//! Small host-library substitutions used by the VM implementation.
//!
//! Desktop builds retain the hash collections from `std`.  Freestanding builds
//! use ordered collections supplied by `alloc`, avoiding an additional runtime
//! dependency while preserving the VM's internal collection contract.

#[cfg(not(target_os = "none"))]
pub(crate) use std::collections::{HashMap, HashSet};

#[cfg(target_os = "none")]
pub(crate) use alloc::collections::{BTreeMap as HashMap, BTreeSet as HashSet};
