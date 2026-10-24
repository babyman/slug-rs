use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Instant};

#[cfg(feature = "metrics")]
use crate::vm::VmMetrics;
use crate::{RuntimeError, Value, value::Task};

use super::{TaskRunOutcome, progress::ProgressDriver, timers::TimerService};

#[derive(Clone, Copy, Eq, PartialEq)]
/// How a nursery settles a failed child. Either policy cancels pending
/// children when the owner's body fails.
pub(super) enum SettlementPolicy {
    /// The root: an unobserved child failure is reported once every task has
    /// settled.
    Join,
    /// An explicit nursery: the first unobserved child failure cancels the
    /// remaining children.
    FailFast,
}

pub(super) struct Nursery {
    tasks: RefCell<Vec<Rc<Task>>>,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    policy: SettlementPolicy,
    timers: Rc<RefCell<TimerService>>,
    progress: Rc<ProgressDriver>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl Nursery {
    pub(super) fn root(
        progress: Rc<ProgressDriver>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        Self::new(
            SettlementPolicy::Join,
            progress,
            #[cfg(feature = "metrics")]
            metrics,
        )
    }

    pub(super) fn explicit(
        progress: Rc<ProgressDriver>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        Self::new(
            SettlementPolicy::FailFast,
            progress,
            #[cfg(feature = "metrics")]
            metrics,
        )
    }

    fn new(
        policy: SettlementPolicy,
        progress: Rc<ProgressDriver>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        Self {
            tasks: RefCell::new(Vec::new()),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            policy,
            timers: Rc::new(RefCell::new(TimerService::new(
                #[cfg(feature = "metrics")]
                metrics.clone(),
            ))),
            progress,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(super) fn ready_queue(&self) -> Rc<RefCell<VecDeque<Rc<Task>>>> {
        self.ready.clone()
    }

    pub(super) fn timer_service(&self) -> Rc<RefCell<TimerService>> {
        self.timers.clone()
    }

    pub(super) fn clear(&self) {
        self.tasks.borrow_mut().clear();
        self.ready.borrow_mut().clear();
    }

    pub(super) fn add_task(&self, task: Rc<Task>) {
        self.tasks.borrow_mut().push(task.clone());
        let mut ready = self.ready.borrow_mut();
        ready.push_back(task);
        #[cfg(feature = "metrics")]
        {
            let depth = ready.len();
            let mut metrics = self.metrics.borrow_mut();
            metrics.peak_ready_queue = metrics.peak_ready_queue.max(depth);
        }
    }

    pub(super) fn enqueue(&self, task: Rc<Task>) {
        let mut ready = self.ready.borrow_mut();
        ready.push_back(task);
        #[cfg(feature = "metrics")]
        {
            let depth = ready.len();
            let mut metrics = self.metrics.borrow_mut();
            metrics.peak_ready_queue = metrics.peak_ready_queue.max(depth);
        }
    }

    fn first_unobserved_error(&self) -> Option<RuntimeError> {
        self.tasks
            .borrow()
            .iter()
            .find_map(|task| task.unobserved_error())
    }

    /// The error of the first task that is still waiting, located where it
    /// waits.
    pub(super) fn first_blocked_error(&self) -> Option<RuntimeError> {
        self.tasks
            .borrow()
            .iter()
            .find_map(|task| task.blocked_error())
    }

    pub(super) fn cancel_all(&self, error: &RuntimeError) {
        for task in self.tasks.borrow().iter() {
            task.cancel(error);
        }
    }

    pub(super) fn run_task(&self, task: &Task) {
        while task.is_pending() && self.make_progress() {}
    }

    pub(super) fn settle(
        &self,
        result: &Result<Value, RuntimeError>,
        cancellation: &RuntimeError,
        blocked: &RuntimeError,
    ) -> Result<Value, RuntimeError> {
        loop {
            if let Some(result) = self.settle_available(result, cancellation) {
                return result;
            }
            // Only a successful body waits here; a failed one settled above.
            if !self.wait_for_progress() {
                // Report the first task that is still waiting, where it waits,
                // rather than the settling owner.
                let blocked = self
                    .first_blocked_error()
                    .unwrap_or_else(|| blocked.clone());
                self.cancel_all(&blocked);
                return Err(blocked);
            }
        }
    }

    /// Attempts nursery settlement using only work already available to the
    /// current host turn. `None` means that a task is still pending.
    pub(super) fn settle_available(
        &self,
        result: &Result<Value, RuntimeError>,
        cancellation: &RuntimeError,
    ) -> Option<Result<Value, RuntimeError>> {
        // A failed body cancels its pending children under either policy:
        // nothing can await them any more, and the root would otherwise wait
        // indefinitely on a task served by a live native producer.
        if let Err(error) = result {
            self.cancel_all(cancellation);
            return Some(Err(error.clone()));
        }

        let mut index = 0;
        while let Some(task) = self.tasks.borrow().get(index).cloned() {
            while task.is_pending() && self.make_available_progress() {}
            if self.policy == SettlementPolicy::FailFast
                && let Some(error) = self.first_unobserved_error()
            {
                self.cancel_all(cancellation);
                return Some(Err(error));
            }
            if task.is_pending() {
                return None;
            }
            index += 1;
        }

        Some(match result {
            Err(error) => Err(error.clone()),
            Ok(value) => self
                .first_unobserved_error()
                .map_or_else(|| Ok(value.clone()), Err),
        })
    }

    /// Performs one available runtime round. It never waits for an operating
    /// system event, so it is safe for a host-driven VM pump.
    pub(super) fn make_available_progress(&self) -> bool {
        let native_progress = self.progress.make_available_progress();
        let task_progress = self.run_next_ready_task();
        let timer_progress = self.wake_due_timers();
        native_progress || task_progress || timer_progress
    }

    /// Blocking-adapter wait only. Core VM progress must use
    /// [`Self::make_available_progress`].
    pub(super) fn wait_for_progress(&self) -> bool {
        if self.make_available_progress() {
            return true;
        }
        let observed = self.progress.snapshot();
        if self.make_available_progress() {
            return true;
        }
        let deadline = self.timers.borrow().next_deadline();
        if deadline.is_none() && !self.progress.has_live_native_source() {
            // A producer may have sent, closed, and released since the drain
            // above; whatever it sent before closing is queued by now.
            return self.progress.make_available_progress();
        }
        let timeout = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        #[cfg(feature = "metrics")]
        let wait_started = Instant::now();
        self.progress.wait(observed, timeout);
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().scheduler_wait_time += wait_started.elapsed();
        }
        true
    }

    pub(super) fn make_progress(&self) -> bool {
        self.make_available_progress() || self.wait_for_progress()
    }

    fn run_next_ready_task(&self) -> bool {
        let Some(next) = self.next_ready_task() else {
            return false;
        };
        let Some(run) = next.take_pending(&next) else {
            return true;
        };
        match run.run() {
            TaskRunOutcome::Settled(result) => next.complete(&result),
            TaskRunOutcome::Suspended(mut execution) => {
                let wait_registration = execution.take_wait_registration();
                next.suspend(*execution, wait_registration);
            }
        }
        true
    }

    fn next_ready_task(&self) -> Option<Rc<Task>> {
        let mut ready = self.ready.borrow_mut();
        let candidates = ready.len();
        for _ in 0..candidates {
            let task = ready.pop_front().expect("ready queue length was checked");
            if !task.is_pending() {
                continue;
            }
            if task.try_admit() {
                return Some(task);
            }
            ready.push_back(task);
        }
        None
    }

    pub(super) fn wake_due_timers(&self) -> bool {
        let due = self.timers.borrow_mut().take_due();
        let woke = !due.is_empty();
        for waiter in due {
            waiter.resume(Ok(Value::Nil));
        }
        woke
    }
}
