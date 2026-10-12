//! The uniform shim ABI as Rust types. See
//! `docs/decisions/2026-10-10-uniform-shim-entry.md`.

use core::ffi::{c_char, c_void};

/// Version of the entry and table layout this adapter calls.
pub const SHIM_ABI_VERSION: u32 = 1;

/// Maximum argument cells, and separately result cells, for one binding.
pub const MAX_CELLS: usize = 16;

/// One argument or result cell: C's `slug_value`.
///
/// The C type is a union of `int64_t`, `uint64_t`, `double`, and
/// `const void *` followed by a length. The union is eight bytes on every
/// supported target, so it is kept here as one native-endian word. A pointer
/// occupies the union's first bytes, which [`Cell::address`] accounts for on
/// big-endian 32-bit targets.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cell {
    pub word: u64,
    /// Length of `str`, `bytes`, and `cstr` values; otherwise 0.
    pub length: u64,
}

const POINTER_SHIFT: u32 = if cfg!(target_endian = "big") && usize::BITS == 32 {
    32
} else {
    0
};

impl Cell {
    #[must_use]
    pub const fn signed(value: i64) -> Self {
        Self {
            word: u64::from_ne_bytes(value.to_ne_bytes()),
            length: 0,
        }
    }

    #[must_use]
    pub const fn unsigned(value: u64) -> Self {
        Self {
            word: value,
            length: 0,
        }
    }

    #[must_use]
    pub const fn float(value: f64) -> Self {
        Self {
            word: value.to_bits(),
            length: 0,
        }
    }

    /// A pointer cell holding `address`, as produced by
    /// `pointer.expose_provenance()`.
    #[must_use]
    pub const fn pointer(address: usize, length: u64) -> Self {
        Self {
            word: (address as u64) << POINTER_SHIFT,
            length,
        }
    }

    #[must_use]
    pub const fn as_signed(self) -> i64 {
        i64::from_ne_bytes(self.word.to_ne_bytes())
    }

    #[must_use]
    pub const fn as_float(self) -> f64 {
        f64::from_bits(self.word)
    }

    /// The pointer stored in this cell, as an address.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // a pointer cell holds a usize
    pub const fn address(self) -> usize {
        (self.word >> POINTER_SHIFT) as usize
    }
}

/// C's `slug_shim_cleanup`: a release the adapter runs after decoding.
#[repr(C)]
#[derive(Debug)]
pub struct Cleanup {
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
    pub context: *mut c_void,
}

/// C's `slug_shim_entry`.
pub type Entry = unsafe extern "C" fn(*const Cell, *mut Cell, *mut Cleanup) -> i32;

/// C's `slug_shim_binding`.
#[repr(C)]
#[derive(Debug)]
pub struct RawBinding {
    pub module: *const c_char,
    pub name: *const c_char,
    pub signature: *const c_char,
    pub entry: Option<Entry>,
    pub argument_count: u32,
    pub result_count: u32,
}

/// C's `slug_shim_table`.
#[repr(C)]
#[derive(Debug)]
pub struct RawTable {
    pub abi_version: u32,
    pub binding_count: u32,
    pub bindings: *const RawBinding,
}

/// A table function: `slug_shim_get_table` or `slug_shim_table_<provider>`.
pub type GetTable = unsafe extern "C" fn() -> *const RawTable;
