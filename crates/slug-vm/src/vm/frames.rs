//! Call-frame representation and frame-local binding storage.
//!
//! Frames retain the code owner, lexical locals, deferred scopes, and source
//! location needed to preserve checked runtime errors across calls and cleanup.

use std::rc::Rc;

use crate::{
    Program, SourceSpan, SpanId, Value,
    value::{BindingCell, Closure, GlobalEnvironment},
};

use super::Deferred;

#[derive(Clone)]
pub(super) struct Frame {
    /// The code unit that owns this frame's instruction pointer and chunk.
    ///
    /// An execution can cross program boundaries through closures, so this
    /// cannot be inferred from a VM-wide program owner.
    pub(super) program: Rc<Program>,
    pub(super) globals: GlobalEnvironment,
    pub(super) closure: Rc<Closure>,
    pub(super) call_span: Option<CallSpan>,
    pub(super) ip: usize,
    pub(super) stack_base: usize,
    pub(super) locals: Vec<LocalSlot>,
    pub(super) provided: ProvidedArguments,
    pub(super) scopes: Vec<Vec<Deferred>>,
    pub(super) cleanup_action: bool,
    pub(super) cleanup_recovers: bool,
    /// Lexical scope depth, including the function's root scope.
    pub(super) scope_depth: u32,
    pub(super) cleanup_owner_depth: Option<usize>,
}

/// The call site retained for an eventual stack trace.
///
/// Ordinary source calls refer to the caller's installed span table. The
/// caller frame remains live while its callee runs, so the owned span only
/// remains necessary for host-initiated and cross-execution calls.
#[derive(Clone)]
pub(super) enum CallSpan {
    Instruction(SpanId),
    Owned(Box<SourceSpan>),
}

#[derive(Clone)]
pub(crate) enum ProvidedArguments {
    All,
    Bitmap(Vec<bool>),
}

impl ProvidedArguments {
    pub(super) fn is_provided(&self, slot: usize) -> bool {
        match self {
            Self::All => true,
            Self::Bitmap(provided) => provided.get(slot).copied().unwrap_or(false),
        }
    }
}

#[derive(Clone)]
pub(super) enum LocalSlot {
    Direct(Value),
    Captured(BindingCell),
}

pub(super) fn frame_locals(arguments: Vec<Value>, local_count: usize) -> Vec<LocalSlot> {
    let mut locals = arguments
        .into_iter()
        .map(LocalSlot::Direct)
        .collect::<Vec<_>>();
    locals.resize_with(local_count, || LocalSlot::Direct(Value::Nil));
    locals
}
