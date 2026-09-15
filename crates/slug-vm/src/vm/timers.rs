use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

#[cfg(feature = "metrics")]
use super::VmMetrics;
use crate::value::Waiter;

type TimerId = u64;

struct TimerEntry {
    deadline: Instant,
    id: TimerId,
    waiter: Waiter,
}

/// Scheduler-owned timer heap for a dynamic nursery.
///
/// The heap preserves registration order for equal deadlines. Timer
/// registrations retain a private ID, which makes cancellation an indexed
/// removal instead of a scan through every pending deadline.
pub(crate) struct TimerService {
    heap: Vec<TimerEntry>,
    positions: HashMap<TimerId, usize>,
    next_id: TimerId,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl TimerService {
    pub(crate) fn new(#[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>) -> Self {
        Self {
            heap: Vec::new(),
            positions: HashMap::new(),
            next_id: 0,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn register(&mut self, deadline: Instant, waiter: Waiter) -> TimerId {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("too many timer registrations");
        let index = self.heap.len();
        self.heap.push(TimerEntry {
            deadline,
            id,
            waiter,
        });
        self.positions.insert(id, index);
        self.sift_up(index);
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.timer_registrations += 1;
            metrics.peak_timer_waiters = metrics.peak_timer_waiters.max(self.heap.len());
        }
        id
    }

    pub(crate) fn take_due(&mut self) -> Vec<Waiter> {
        let now = Instant::now();
        let mut due = Vec::new();
        while self.heap.first().is_some_and(|entry| entry.deadline <= now) {
            let entry = self.pop_min().expect("timer heap was checked");
            due.push(entry.waiter);
        }
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.timer_wakeups += due.len();
            metrics.timer_wakeup_entries_examined += due.len();
        }
        due
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        #[cfg(feature = "metrics")]
        {
            let mut metrics = self.metrics.borrow_mut();
            metrics.timer_deadline_lookups += 1;
            metrics.timer_deadline_entries_examined += usize::from(!self.heap.is_empty());
        }
        self.heap.first().map(|entry| entry.deadline)
    }

    fn remove(&mut self, id: TimerId) {
        let Some(index) = self.positions.remove(&id) else {
            return;
        };
        self.remove_at(index);
    }

    fn pop_min(&mut self) -> Option<TimerEntry> {
        (!self.heap.is_empty()).then(|| self.remove_at(0))
    }

    fn remove_at(&mut self, index: usize) -> TimerEntry {
        let entry = self.heap.swap_remove(index);
        self.positions.remove(&entry.id);
        if index < self.heap.len() {
            self.positions.insert(self.heap[index].id, index);
            self.repair(index);
        }
        entry
    }

    fn repair(&mut self, index: usize) {
        if index > 0 && Self::precedes(&self.heap[index], &self.heap[(index - 1) / 2]) {
            self.sift_up(index);
        } else {
            self.sift_down(index);
        }
    }

    fn sift_up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            if !Self::precedes(&self.heap[index], &self.heap[parent]) {
                break;
            }
            self.swap(index, parent);
            index = parent;
        }
    }

    fn sift_down(&mut self, mut index: usize) {
        loop {
            let left = index * 2 + 1;
            let right = left + 1;
            let Some(mut smallest) = (left < self.heap.len()).then_some(left) else {
                return;
            };
            if right < self.heap.len() && Self::precedes(&self.heap[right], &self.heap[left]) {
                smallest = right;
            }
            if !Self::precedes(&self.heap[smallest], &self.heap[index]) {
                return;
            }
            self.swap(index, smallest);
            index = smallest;
        }
    }

    fn swap(&mut self, left: usize, right: usize) {
        self.heap.swap(left, right);
        self.positions.insert(self.heap[left].id, left);
        self.positions.insert(self.heap[right].id, right);
    }

    fn precedes(left: &TimerEntry, right: &TimerEntry) -> bool {
        left.deadline < right.deadline || (left.deadline == right.deadline && left.id < right.id)
    }
}

/// A timer-specific cancellation handle stored in a general wait set.
#[derive(Clone)]
pub(crate) struct TimerRegistration {
    timers: Rc<RefCell<TimerService>>,
    id: TimerId,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<VmMetrics>>,
}

impl TimerRegistration {
    pub(crate) fn new(
        timers: Rc<RefCell<TimerService>>,
        id: TimerId,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<VmMetrics>>,
    ) -> Self {
        Self {
            timers,
            id,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn remove(&self, _waiter: &Waiter) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().timer_waiter_entries_examined += 1;
        }
        self.timers.borrow_mut().remove(self.id);
    }
}
