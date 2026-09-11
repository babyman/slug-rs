use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::{Rc, Weak},
    sync::Arc,
    time::Instant,
};

#[cfg(feature = "metrics")]
use crate::vm::VmMetrics;
use crate::{
    RuntimeError, Task, Value,
    scheduler_signal::{BlockingProgressWaiter, ProgressSignal},
    value::{Channel, TimerService},
};

use super::TaskRunOutcome;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SettlementPolicy {
    Join,
    FailFast,
}

pub(super) struct Nursery {
    tasks: RefCell<Vec<Rc<Task>>>,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    policy: SettlementPolicy,
    timers: Rc<RefCell<TimerService>>,
    native_channels: RefCell<Vec<Weak<Channel>>>,
    signal: Arc<ProgressSignal>,
    blocking_waiter: BlockingProgressWaiter,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl Nursery {
    pub(super) fn root(#[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>) -> Self {
        Self::new(
            SettlementPolicy::Join,
            #[cfg(feature = "metrics")]
            metrics,
        )
    }

    pub(super) fn explicit(#[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>) -> Self {
        Self::new(
            SettlementPolicy::FailFast,
            #[cfg(feature = "metrics")]
            metrics,
        )
    }

    fn new(
        policy: SettlementPolicy,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        let signal = Arc::new(ProgressSignal::new());
        let blocking_waiter = signal.blocking_waiter();
        Self {
            tasks: RefCell::new(Vec::new()),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            policy,
            timers: Rc::new(RefCell::new(TimerService::new(
                #[cfg(feature = "metrics")]
                metrics.clone(),
            ))),
            native_channels: RefCell::new(Vec::new()),
            signal,
            blocking_waiter,
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

    pub(super) fn cancel_all(&self, error: &RuntimeError) {
        for task in self.tasks.borrow().iter() {
            task.cancel(error);
        }
    }

    pub(super) fn track_native_channel(&self, channel: &Rc<Channel>) {
        if !channel.has_native_producer() {
            return;
        }
        channel.register_progress_signal(&self.signal);
        let mut channels = self.native_channels.borrow_mut();
        if !channels.iter().any(|candidate| {
            candidate
                .upgrade()
                .is_some_and(|candidate| Rc::ptr_eq(&candidate, channel))
        }) {
            channels.push(Rc::downgrade(channel));
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
            if !self.wait_for_progress() {
                let body_error = result.as_ref().err().cloned();
                self.cancel_all(if body_error.is_some() {
                    cancellation
                } else {
                    blocked
                });
                return Err(body_error.unwrap_or_else(|| blocked.clone()));
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
        let body_error = result.as_ref().err().cloned();
        if self.policy == SettlementPolicy::FailFast
            && let Some(error) = &body_error
        {
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
        self.run_next_ready_task() || self.drain_native_channels() || self.wake_due_timers()
    }

    /// Blocking-adapter wait only. Core VM progress must use
    /// [`Self::make_available_progress`].
    pub(super) fn wait_for_progress(&self) -> bool {
        if self.make_available_progress() {
            return true;
        }
        let observed = self.signal.snapshot();
        if self.make_available_progress() {
            return true;
        }
        let deadline = self.timers.borrow().next_deadline();
        if deadline.is_none() && !self.has_live_native_source() {
            return false;
        }
        let timeout = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        #[cfg(feature = "metrics")]
        let wait_started = Instant::now();
        self.blocking_waiter.wait(&self.signal, observed, timeout);
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

    fn native_channels(&self) -> Vec<Rc<Channel>> {
        let mut channels = self.native_channels.borrow_mut();
        channels.retain(|channel| channel.strong_count() > 0);
        channels.iter().filter_map(Weak::upgrade).collect()
    }

    fn drain_native_channels(&self) -> bool {
        let mut changed = false;
        for channel in self.native_channels() {
            changed |= channel.drain_native();
        }
        changed
    }

    fn has_live_native_source(&self) -> bool {
        self.native_channels()
            .iter()
            .any(|channel| channel.has_live_native_producer())
    }

    fn wake_due_timers(&self) -> bool {
        let due = self.timers.borrow_mut().take_due();
        let woke = !due.is_empty();
        for waiter in due {
            waiter.resume(Ok(Value::Nil));
        }
        woke
    }
}
