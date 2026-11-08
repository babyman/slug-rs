//! The reference counts the VM shares values with.
//!
//! A build with `threads` counts atomically, so values can be shared with
//! forks on other threads; a build without it, such as an embedded one, keeps
//! the cheaper single-threaded counts.

#[cfg(feature = "threads")]
pub(crate) use alloc::sync::{Arc as Rc, Weak};

#[cfg(not(feature = "threads"))]
pub(crate) use alloc::rc::{Rc, Weak};
