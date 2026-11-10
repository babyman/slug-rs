use crate::sync::Rc;
use alloc::{collections::VecDeque, vec::Vec};
use core::cell::RefCell;

#[cfg(feature = "metrics")]
use crate::vm::VmMetrics;
use crate::{
    RuntimeError, Value,
    value::{NurseryWatch, Task},
};

use super::{TaskRunOutcome, progress::ProgressDriver, timers::TimerService};

/// The tasks a root, or an explicit `nursery`, started, and how it settles
/// them. Either kind cancels pending children when the owner's body fails.
///
/// One scheduler runs every nursery of a VM: an explicit nursery shares its
/// parent's ready queue and timers, so a host's `poll` makes progress on
/// all of them, and none ever waits for another to finish.
pub(super) struct Nursery {
    tasks: RefCell<Vec<Rc<Task>>>,
    /// An explicit nursery's: the first unobserved child failure cancels the
    /// remaining children. The root, without one, reports an unobserved
    /// child failure once every task has settled.
    watch: Option<Rc<NurseryWatch>>,
    /// A fork that yielded mid-turn, on a build that runs forks
    /// cooperatively: until it runs again, no other task of its fork domain
    /// may, as on an island, where they would never interleave.
    #[cfg(all(
        feature = "concurrency",
        not(all(feature = "threads", not(target_os = "none")))
    ))]
    yielded: Rc<RefCell<Option<Rc<Task>>>>,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
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
        Self {
            tasks: RefCell::new(Vec::new()),
            watch: None,
            #[cfg(all(
                feature = "concurrency",
                not(all(feature = "threads", not(target_os = "none")))
            ))]
            yielded: Rc::new(RefCell::new(None)),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            timers: Rc::new(RefCell::new(TimerService::new(
                #[cfg(feature = "metrics")]
                metrics.clone(),
            ))),
            progress,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    /// An explicit `nursery` started in a task of `parent`'s, run by the
    /// same scheduler.
    pub(super) fn explicit(parent: &Self) -> Self {
        Self {
            tasks: RefCell::new(Vec::new()),
            watch: Some(Rc::new(NurseryWatch::default())),
            #[cfg(all(
                feature = "concurrency",
                not(all(feature = "threads", not(target_os = "none")))
            ))]
            yielded: parent.yielded.clone(),
            ready: parent.ready.clone(),
            timers: parent.timers.clone(),
            progress: parent.progress.clone(),
            #[cfg(feature = "metrics")]
            metrics: parent.metrics.clone(),
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

    /// Starts a child of this nursery's, the one way a task begins. A fork
    /// its nursery admits now leaves for a worker at once; any other task,
    /// and a fork waiting for a permit, waits in the ready queue in start
    /// order, and is dispatched from there when the scheduler admits it.
    pub(super) fn start(&self, task: Rc<Task>) {
        if let Some(watch) = &self.watch {
            task.join(watch.clone());
        }
        self.tasks.borrow_mut().push(task.clone());
        if !Self::dispatch_remote(&task) {
            self.enqueue(task);
        }
    }

    /// Sends an admitted fork to run on a worker, on a build with threads;
    /// false for any task that runs on this scheduler instead.
    fn dispatch_remote(task: &Rc<Task>) -> bool {
        #[cfg(all(feature = "threads", not(target_os = "none")))]
        return super::island::ship(task);
        #[cfg(not(all(feature = "threads", not(target_os = "none"))))]
        {
            let _ = task;
            false
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

    /// Settles an explicit nursery's body, which returned `result`, once
    /// its children allow: `None` while one is still pending, when the body
    /// suspends to wait for them.
    pub(super) fn settle_body(
        &self,
        result: &Result<Value, RuntimeError>,
        cancellation: &RuntimeError,
    ) -> Option<Result<Value, RuntimeError>> {
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.cancel_all(cancellation);
                return Some(Err(error.clone()));
            }
        };
        let watch = self.watch.as_ref()?;
        // Only a child's failure since the last look can fail it fast, so
        // the children are scanned once per failure, not once per wake.
        if watch.take_failed()
            && let Some(error) = self.first_unobserved_error()
        {
            self.cancel_all(cancellation);
            return Some(Err(error));
        }
        if watch.pending() == 0 {
            return Some(
                self.first_unobserved_error()
                    .map_or_else(|| Ok(value.clone()), Err),
            );
        }
        None
    }

    /// The watch on this explicit nursery's children.
    pub(super) fn watch(&self) -> Option<&Rc<NurseryWatch>> {
        self.watch.as_ref()
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

    /// Attempts the root's settlement using only work already available to
    /// the current host turn. `None` means that a task is still pending.
    pub(super) fn settle_available(
        &self,
        result: &Result<Value, RuntimeError>,
        cancellation: &RuntimeError,
    ) -> Option<Result<Value, RuntimeError>> {
        // A failed body cancels its pending children, as an explicit
        // nursery's does:
        // nothing can await them any more, and the root would otherwise wait
        // indefinitely on a task served by a live native producer.
        if let Err(error) = result {
            self.cancel_all(cancellation);
            return Some(Err(error.clone()));
        }

        let mut index = 0;
        loop {
            // Released before the task runs: a `while let` would hold the
            // borrow through the body, and a task that spawns needs it.
            let Some(task) = self.tasks.borrow().get(index).cloned() else {
                break;
            };
            while task.is_pending() && self.make_available_progress() {}
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

    /// Blocking-adapter wait only, on a freestanding host, which cannot
    /// sleep: spins until a timer is due or a native producer sends. Core VM
    /// progress must use [`Self::make_available_progress`], and hosts that
    /// can sleep use `Vm::next_timer_in`.
    #[cfg(target_os = "none")]
    pub(super) fn wait_for_progress(&self) -> bool {
        loop {
            if self.make_available_progress() {
                return true;
            }
            if self.timers.borrow().next_deadline().is_none()
                && !self.progress.has_live_native_source()
            {
                // A producer may have sent, closed, and released since the
                // drain above; whatever it sent before closing is queued.
                return self.progress.make_available_progress();
            }
            core::hint::spin_loop();
        }
    }

    /// How long until the next timer is due: zero when one is, and `None`
    /// with no timer pending.
    pub(super) fn next_timer_in(&self) -> Option<core::time::Duration> {
        let deadline = self.timers.borrow().next_deadline()?;
        Some(deadline.saturating_duration_since(self.progress.now()))
    }

    /// Blocking-adapter wait only. Core VM progress must use
    /// [`Self::make_available_progress`].
    #[cfg(not(target_os = "none"))]
    pub(super) fn wait_for_progress(&self) -> bool {
        if self.make_available_progress() {
            return true;
        }
        let observed = self.progress.snapshot();
        if self.make_available_progress() {
            return true;
        }
        let deadline = self.timers.borrow().next_deadline();
        // Only another thread can wake this one: sleep until it does, or
        // until every thread of the program is asleep, which is a deadlock.
        #[cfg(all(feature = "threads", not(target_os = "none")))]
        if deadline.is_none()
            && !self.progress.has_live_native_source()
            && self.progress.may_be_woken_elsewhere()
        {
            return self.progress.sleep_until_woken(observed)
                || self.progress.make_available_progress();
        }
        if deadline.is_none() && !self.progress.has_live_native_source() {
            // A producer may have sent, closed, and released since the drain
            // above; whatever it sent before closing is queued by now.
            return self.progress.make_available_progress();
        }
        let timeout =
            deadline.map(|deadline| deadline.saturating_duration_since(self.progress.now()));
        #[cfg(feature = "metrics")]
        let wait_started = self.progress.now();
        self.progress.wait(observed, timeout);
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().scheduler_wait_time +=
                self.progress.now().saturating_duration_since(wait_started);
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
        // A fork admitted only now starts on a worker.
        if Self::dispatch_remote(&next) {
            return true;
        }
        let Some(run) = next.take_pending(&next) else {
            return true;
        };
        match run.run() {
            TaskRunOutcome::Settled(result) => next.complete(&result),
            TaskRunOutcome::Suspended(mut execution) => {
                #[cfg(all(
                    feature = "concurrency",
                    not(all(feature = "threads", not(target_os = "none")))
                ))]
                let yielded = execution.yielded();
                let wait_registration = execution.take_wait_registration();
                let settling = execution.settling_watch();
                next.suspend(*execution, wait_registration);
                // A nursery's body waits for its children to settle.
                if let Some(watch) = settling {
                    watch.wait(next.clone());
                }
                #[cfg(all(
                    feature = "concurrency",
                    not(all(feature = "threads", not(target_os = "none")))
                ))]
                if yielded {
                    next.requeue();
                    *self.yielded.borrow_mut() = Some(next);
                }
            }
        }
        true
    }

    fn next_ready_task(&self) -> Option<Rc<Task>> {
        // A yielded fork that has since settled, such as by cancellation,
        // holds its domain no longer.
        #[cfg(all(
            feature = "concurrency",
            not(all(feature = "threads", not(target_os = "none")))
        ))]
        let held = {
            let mut yielded = self.yielded.borrow_mut();
            if yielded.as_ref().is_some_and(|task| !task.is_queued()) {
                *yielded = None;
            }
            yielded
                .as_ref()
                .and_then(|task| task.queued_domain().map(|domain| (task.clone(), domain)))
        };
        let mut ready = self.ready.borrow_mut();
        let candidates = ready.len();
        for _ in 0..candidates {
            let task = ready.pop_front().expect("ready queue length was checked");
            if !task.is_queued() {
                continue;
            }
            #[cfg(all(
                feature = "concurrency",
                not(all(feature = "threads", not(target_os = "none")))
            ))]
            if let Some((yielded, domain)) = &held {
                if task.is(yielded) {
                    *self.yielded.borrow_mut() = None;
                } else if task.queued_domain() == Some(*domain) {
                    ready.push_back(task);
                    continue;
                }
            }
            if task.try_admit() {
                return Some(task);
            }
            ready.push_back(task);
        }
        None
    }

    pub(super) fn wake_due_timers(&self) -> bool {
        let due = self.timers.borrow_mut().take_due(self.progress.now());
        let woke = !due.is_empty();
        for waiter in due {
            waiter.resume(Ok(Value::Nil));
        }
        woke
    }
}
