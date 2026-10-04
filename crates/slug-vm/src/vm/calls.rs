//! Private call-shape metadata shared by VM call paths.
//!
//! Call execution remains with the dispatch owner because overload selection,
//! live-binding validation, and frame creation must remain traceable together.

#[cfg(feature = "concurrency")]
use alloc::rc::Rc;
use alloc::{string::String, vec::Vec};
#[cfg(feature = "concurrency")]
use core::cell::Cell;

use crate::{CallableIdentity, Value};

#[cfg(feature = "concurrency")]
use super::Nursery;

pub(super) type NamedArgument = (String, Value);
pub(super) type ExpandedCallArguments = (Vec<Value>, Vec<NamedArgument>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CallableRuntimeSignature {
    pub(super) identity: Option<CallableIdentity>,
    pub(super) shape: Vec<(bool, bool)>,
}

#[cfg(feature = "concurrency")]
pub(super) struct ClosureCallOptions {
    pub(super) direct_task_limit: Option<usize>,
    pub(super) direct_task_count: Option<Rc<Cell<usize>>>,
    pub(super) nursery: Rc<Nursery>,
}
