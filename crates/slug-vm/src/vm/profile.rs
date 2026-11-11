//! Sampled Slug call stacks, in counting builds only.
//!
//! Every [`PERIOD`] instructions, the active task's Slug frames are recorded
//! as one folded stack, the input flame-graph tools take: outermost first,
//! each function as `module:line` of the line its code starts on, then the
//! line the innermost one is running, as `module:line` too. It answers where a Slug program spends
//! its instructions, which a native profiler, seeing only the dispatch loop,
//! cannot. Instructions are not time: a native call or a large copy counts
//! as one.

use crate::sync::Rc;
use alloc::{collections::BTreeMap, string::String};
use core::{cell::RefCell, fmt, fmt::Write as _};

use super::Vm;

/// Instructions between samples: prime, so a loop's period does not alias.
pub(super) const PERIOD: usize = 997;

/// The folded stacks sampled so far, shared by a VM and its tasks; `None`
/// until profiling is enabled.
pub(super) type Samples = Rc<RefCell<Option<BTreeMap<String, usize>>>>;

impl Vm {
    /// Starts sampling Slug call stacks; [`Vm::take_profile`] returns them.
    pub fn enable_profile(&self) {
        *self.profile.borrow_mut() = Some(BTreeMap::new());
    }

    /// The sampled folded stacks and how often each was seen, ending
    /// profiling; `None` when it was not enabled.
    #[must_use]
    pub fn take_profile(&self) -> Option<BTreeMap<String, usize>> {
        self.profile.borrow_mut().take()
    }

    #[cold]
    #[inline(never)]
    pub(super) fn sample_stack(&self) {
        let mut profile = self.profile.borrow_mut();
        let Some(samples) = profile.as_mut() else {
            return;
        };
        let mut stack = String::new();
        let line = |frame: &super::frames::Frame, at: Option<usize>| -> Option<u32> {
            let chunk = frame.program().chunk(frame.closure.chunk)?;
            let instruction = match at {
                Some(index) => chunk.code.get(index)?,
                None => chunk
                    .code
                    .iter()
                    .find(|instruction| instruction.span.is_some())?,
            };
            Some(frame.program().span(instruction.span?)?.line)
        };
        for frame in &self.frames {
            if !stack.is_empty() {
                stack.push(';');
            }
            // A function is named by the line its code starts on.
            let _ = write!(
                stack,
                "{}:{}",
                frame.program().module_name(),
                Line(line(frame, None))
            );
        }
        // The innermost frame's line, so that the samples a function takes
        // itself divide by line.
        if let Some(frame) = self.frames.last() {
            let _ = write!(
                stack,
                ";{}:{}",
                frame.program().module_name(),
                Line(line(frame, frame.ip.checked_sub(1)))
            );
        }
        *samples.entry(stack).or_insert(0) += 1;
    }
}

struct Line(Option<u32>);

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(line) => write!(f, "{line}"),
            None => f.write_str("?"),
        }
    }
}
