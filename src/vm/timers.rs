use std::{cell::RefCell, rc::Rc, time::Instant};

#[cfg(feature = "metrics")]
use super::VmMetrics;
use crate::value::Waiter;

/// Scheduler-owned timer queue for a dynamic nursery.
///
/// Timer registration and wake-up policy live here rather than alongside
/// channel readiness. The shared wait-set only retains a cancellation handle.
pub(crate) struct TimerService {
    waiters: Vec<(Instant, Waiter)>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl TimerService {
    pub(crate) fn new(#[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>) -> Self {
        Self {
            waiters: Vec::new(),
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn register(&mut self, deadline: Instant, waiter: Waiter) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_registrations += 1;
        }
        self.waiters.push((deadline, waiter));
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.peak_timer_waiters = metrics.peak_timer_waiters.max(self.waiters.len());
        }
    }

    pub(crate) fn take_due(&mut self) -> Vec<Waiter> {
        let now = Instant::now();
        let mut due = Vec::new();
        #[cfg(feature = "metrics")]
        let examined = self.waiters.len();
        self.waiters.retain(|(deadline, waiter)| {
            if *deadline <= now {
                due.push(waiter.clone());
                false
            } else {
                true
            }
        });
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.timer_wakeups += due.len();
            metrics.timer_wakeup_entries_examined += examined;
        }
        due
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.timer_deadline_lookups += 1;
            metrics.timer_deadline_entries_examined += self.waiters.len();
        }
        self.waiters.iter().map(|(deadline, _)| *deadline).min()
    }
}

/// A timer-specific cancellation handle stored in a general wait set.
#[derive(Clone)]
pub(crate) struct TimerRegistration {
    timers: Rc<RefCell<TimerService>>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl TimerRegistration {
    pub(crate) fn new(
        timers: Rc<RefCell<TimerService>>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        Self {
            timers,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn remove(&self, waiter: &Waiter) {
        let mut timers = self.timers.borrow_mut();
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_waiter_entries_examined += timers.waiters.len();
        }
        timers
            .waiters
            .retain(|(_, candidate)| !candidate.is_same(waiter));
    }
}
