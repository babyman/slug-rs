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
    scheduler_signal::SchedulerSignal,
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
    signal: Arc<SchedulerSignal>,
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
        Self {
            tasks: RefCell::new(Vec::new()),
            ready: Rc::new(RefCell::new(VecDeque::new())),
            policy,
            timers: Rc::new(RefCell::new(TimerService::new(
                #[cfg(feature = "metrics")]
                metrics,
            ))),
            native_channels: RefCell::new(Vec::new()),
            signal: Arc::new(SchedulerSignal::new()),
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
        self.ready.borrow_mut().push_back(task);
    }

    pub(super) fn enqueue(&self, task: Rc<Task>) {
        self.ready.borrow_mut().push_back(task);
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
        channel.register_scheduler(&self.signal);
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
        result: Result<Value, RuntimeError>,
        cancellation: &RuntimeError,
        blocked: &RuntimeError,
    ) -> Result<Value, RuntimeError> {
        let body_error = result.as_ref().err().cloned();
        if self.policy == SettlementPolicy::FailFast
            && let Some(error) = &body_error
        {
            self.cancel_all(cancellation);
            return Err(error.clone());
        }

        let mut index = 0;
        while let Some(task) = self.tasks.borrow().get(index).cloned() {
            self.run_task(&task);
            if self.policy == SettlementPolicy::FailFast
                && let Some(error) = self.first_unobserved_error()
            {
                self.cancel_all(cancellation);
                return Err(error);
            }
            if task.is_pending() {
                let error = body_error.clone().unwrap_or_else(|| blocked.clone());
                self.cancel_all(if body_error.is_some() {
                    cancellation
                } else {
                    blocked
                });
                return Err(error);
            }
            index += 1;
        }

        match result {
            Err(error) => Err(error),
            Ok(value) => self.first_unobserved_error().map_or(Ok(value), Err),
        }
    }

    pub(super) fn make_progress(&self) -> bool {
        loop {
            if self.run_next_ready_task() || self.drain_native_channels() || self.wake_due_timers()
            {
                return true;
            }

            let observed = self.signal.snapshot();
            if self.run_next_ready_task() || self.drain_native_channels() || self.wake_due_timers()
            {
                return true;
            }

            let deadline = self.timers.borrow().next_deadline();
            if deadline.is_none() && !self.has_live_native_source() {
                return false;
            }
            let timeout =
                deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            self.signal.wait(observed, timeout);
        }
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
