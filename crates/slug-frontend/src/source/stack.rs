//! Stack for recursion over source trees.
//!
//! Each compiler pass recurses once per level of a source tree. Trees are
//! bounded (see the parser's depth limit), but a level can take tens of
//! kilobytes of stack in a debug build, more than a 2 MiB thread holds at
//! the limit. Every recursive pass over the tree therefore runs through
//! [`grow`], which continues on a fresh heap-allocated stack segment when
//! the current one runs low, on whatever thread compiles.

/// The stack that must remain before a recursive step runs on the current
/// segment. It covers the deepest stack any one step uses before reaching
/// the next `grow`, with room to spare for debug builds.
const RED_ZONE: usize = 512 * 1024;

/// The size of each additional stack segment.
const SEGMENT: usize = 8 * 1024 * 1024;

/// Runs one recursive step, on a new stack segment if this one is low.
pub(super) fn grow<R>(step: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, SEGMENT, step)
}
