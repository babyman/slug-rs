use crate::sync::Rc;
#[cfg(feature = "concurrency")]
use alloc::boxed::Box;
use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
#[cfg(feature = "concurrency")]
use core::mem;
use core::{cell::RefCell, fmt, fmt::Write as _};

use crate::platform::NameMap;

#[cfg(feature = "concurrency")]
use crate::sync::Weak;
#[cfg(feature = "concurrency")]
use core::cell::Cell;

use crate::{
    CallableIdentity, ForeignResourceSignature,
    collections::{BytesView, List, Map, Str},
    native::{NativeChannelProducer, NativeFunction, NativeResource},
    scheduler_signal::ProgressSignal,
};

/// VM-owned builtins that require host-service context at call time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builtin {
    Cfg,
    Int,
    Stacktrace,
    Describe,
}

impl Builtin {
    /// The names a call may give the builtin's parameters, in order.
    #[must_use]
    pub const fn parameter_names(self) -> &'static [&'static str] {
        match self {
            Self::Cfg => &["key", "default"],
            Self::Int => &["value"],
            Self::Stacktrace => &["error"],
            Self::Describe => &["module", "unexported"],
        }
    }
}

/// A FIFO channel with bounded buffering and parked task wait queues.
pub struct Channel {
    pub(crate) state: Rc<crate::sync::Lock<ChannelState>>,
    /// Set when native code creates the channel, or when a producer is first
    /// minted from a Slug-created channel.
    native_producer: crate::sync::Once<NativeChannelProducer>,
    /// Set once a fork sends or receives on the channel: every value sent on
    /// it is then checked as sendable.
    #[cfg(feature = "concurrency")]
    crossed: crate::sync::Flag,
}

pub(crate) struct ChannelState {
    pub(crate) capacity: usize,
    pub(crate) messages: VecDeque<Value>,
    pub(crate) senders: VecDeque<(Waiter, Value)>,
    pub(crate) receivers: VecDeque<Waiter>,
    pub(crate) closed: bool,
}

pub(crate) enum ChannelSend {
    Ready,
    Pending,
    Closed,
}

pub(crate) enum ChannelReceive {
    Ready(Value),
    Pending,
}

#[cfg(feature = "concurrency")]
type Screen = dyn Fn(Result<Value, crate::RuntimeError>) -> Result<Value, crate::RuntimeError>;

#[derive(Clone)]
pub(crate) struct Waiter {
    identity: usize,
    /// For one case of a `select`: the flag its cases share, which the first
    /// wake to claim sets, so no other case is taken as well.
    claim: Option<Arc<core::sync::atomic::AtomicBool>>,
    /// The thread whose scheduler owns the waiting execution: a wake from
    /// another thread is carried there to run.
    #[cfg(all(feature = "threads", not(target_os = "none")))]
    home: Arc<crate::vm::island::Inbox>,
    resume: Rc<dyn Fn(Result<Value, crate::RuntimeError>)>,
    /// Applied to a wake before it is carried to the waiter's thread: a
    /// fork's receive rejects an unsendable value here, so the value never
    /// reaches the fork's thread.
    #[cfg(feature = "concurrency")]
    screen: Option<Rc<Screen>>,
    reject_closed_send: Rc<dyn Fn()>,
    set_closed_send_error: Rc<dyn Fn(crate::RuntimeError)>,
}

#[derive(Clone)]
pub(crate) enum SelectWake {
    Value {
        handler: Option<Value>,
    },
    /// A receive case of a fork's `select`: the value it is woken with must
    /// be sendable, checked by the waker before the wake leaves its thread,
    /// since its sender may have sent before it saw the channel cross into a
    /// fork. `unsendable` is the
    /// error to report one with, its reason to be added.
    #[cfg(feature = "concurrency")]
    ForkReceive {
        handler: Option<Value>,
        unsendable: Box<crate::RuntimeError>,
    },
    #[cfg(feature = "concurrency")]
    TaskAwait {
        handler: Option<Value>,
        observer: TaskObserver,
    },
}

impl SelectWake {
    fn selected(&self) -> Option<Value> {
        match self {
            Self::Value { handler } => handler.clone(),
            #[cfg(feature = "concurrency")]
            Self::ForkReceive { handler, .. } => handler.clone(),
            #[cfg(feature = "concurrency")]
            Self::TaskAwait { handler, observer } => {
                observer.observe();
                handler.clone()
            }
        }
    }
}

pub(crate) struct SelectWaitState {
    waiter: Waiter,
    claimed: Arc<core::sync::atomic::AtomicBool>,
    registrations: Option<WaitSet>,
}

impl Waiter {
    pub(crate) fn root(root: RootWaiter) -> Self {
        let identity = Rc::as_ptr(&root.state) as usize;
        Self {
            identity,
            claim: None,
            resume: Rc::new({
                let root = root.clone();
                move |result| root.resume(result)
            }),
            #[cfg(feature = "concurrency")]
            screen: None,
            reject_closed_send: Rc::new({
                let root = root.clone();
                move || root.reject_closed_send()
            }),
            set_closed_send_error: Rc::new(move |error| root.set_closed_send_error(error)),
            #[cfg(all(feature = "threads", not(target_os = "none")))]
            home: crate::vm::island::Inbox::current(),
        }
    }

    #[cfg(feature = "concurrency")]
    pub(crate) fn task(task: &Task) -> Self {
        let identity = Rc::as_ptr(&task.state) as usize;
        Self {
            identity,
            claim: None,
            resume: Rc::new({
                let task = task.clone();
                move |result| task.resume(result)
            }),
            #[cfg(feature = "concurrency")]
            screen: None,
            reject_closed_send: Rc::new({
                let task = task.clone();
                move || task.reject_closed_send()
            }),
            set_closed_send_error: Rc::new(|_| {}),
            #[cfg(all(feature = "threads", not(target_os = "none")))]
            home: crate::vm::island::Inbox::current(),
        }
    }

    pub(crate) fn select(state: Rc<RefCell<SelectWaitState>>, wake: SelectWake) -> Self {
        let identity = state.borrow().waiter.identity;
        let claim = Some(state.borrow().claimed.clone());
        #[cfg(all(feature = "threads", not(target_os = "none")))]
        let home = state.borrow().waiter.home.clone();
        let resume_state = state.clone();
        #[cfg(feature = "concurrency")]
        let screen: Option<Rc<Screen>> = match &wake {
            SelectWake::ForkReceive { unsendable, .. } => {
                let unsendable = unsendable.clone();
                Some(Rc::new(move |result| match result {
                    Ok(value) => match crate::vm::unsendable_reason(&value) {
                        Some(reason) => {
                            let mut error = (*unsendable).clone();
                            error.message = alloc::format!(
                                "a value received on a channel is not sendable: it holds {reason}, which stays with the task that made it"
                            );
                            Err(error)
                        }
                        None => Ok(value),
                    },
                    result => result,
                }))
            }
            SelectWake::Value { .. } | SelectWake::TaskAwait { .. } => None,
        };
        Self {
            identity,
            claim,
            // Runs once the wake has claimed the select.
            resume: Rc::new(move |result| {
                let mut selected = resume_state.borrow_mut();
                let registrations = selected.registrations.take();
                let waiter = selected.waiter.clone();
                drop(selected);
                if let Some(registrations) = registrations {
                    registrations.remove_for_waiter(&waiter);
                }
                let handler = wake.selected();
                waiter.resume(
                    result.map(|value| Value::list(vec![value, handler.unwrap_or(Value::Nil)])),
                );
            }),
            #[cfg(feature = "concurrency")]
            screen,
            reject_closed_send: Rc::new({
                let state = state.clone();
                move || state.borrow().waiter.reject_closed_send()
            }),
            set_closed_send_error: Rc::new(move |error| {
                state.borrow().waiter.set_closed_send_error(error);
            }),
            #[cfg(all(feature = "threads", not(target_os = "none")))]
            home,
        }
    }

    /// Claims this waiter's select, when it has one; whether the claim is
    /// this wake's, so the waiter is to be woken.
    pub(crate) fn try_claim(&self) -> bool {
        self.claim
            .as_ref()
            .is_none_or(|claim| !claim.swap(true, core::sync::atomic::Ordering::AcqRel))
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        if self.try_claim() {
            self.resume_claimed(result);
        }
    }

    /// Wakes a waiter whose claim this wake already holds.
    pub(crate) fn resume_claimed(&self, result: Result<Value, crate::RuntimeError>) {
        #[cfg(feature = "concurrency")]
        let result = match &self.screen {
            Some(screen) => screen(result),
            None => result,
        };
        #[cfg(all(feature = "threads", not(target_os = "none")))]
        if !self.home.is_current() {
            let waiter = self.clone();
            self.home.post(move || (waiter.resume)(result));
            return;
        }
        (self.resume)(result);
    }

    pub(crate) fn set_closed_send_error(&self, error: crate::RuntimeError) {
        (self.set_closed_send_error)(error);
    }

    pub(crate) fn reject_closed_send(&self) {
        if self.try_claim() {
            self.reject_claimed();
        }
    }

    pub(crate) fn reject_claimed(&self) {
        #[cfg(all(feature = "threads", not(target_os = "none")))]
        if !self.home.is_current() {
            let waiter = self.clone();
            self.home.post(move || (waiter.reject_closed_send)());
            return;
        }
        (self.reject_closed_send)();
    }

    pub(crate) fn is_same(&self, other: &Waiter) -> bool {
        self.identity == other.identity
    }
}

#[derive(Clone)]
pub(crate) enum WaitRegistration {
    ChannelSend(Rc<Channel>),
    ChannelReceive(Rc<Channel>),
    #[cfg(feature = "concurrency")]
    TaskAwait(Rc<Task>),
    /// A task waiting in a `nursery` for its body to settle. Leaving that
    /// wait other than by the body's settling, as when the task is
    /// cancelled, cancels the body and its children: nothing can settle
    /// them any more.
    #[cfg(feature = "concurrency")]
    Nursery {
        body: Rc<Task>,
        cancel: Rc<dyn Fn()>,
    },
    #[cfg(feature = "concurrency")]
    Timer(crate::vm::timers::TimerRegistration),
}

impl WaitRegistration {
    fn remove(
        self,
        waiter: &Waiter,
        #[cfg(feature = "metrics")] metrics: &Rc<RefCell<crate::vm::VmMetrics>>,
    ) {
        match self {
            Self::ChannelSend(channel) => {
                let mut state = channel.state.lock();
                #[cfg(feature = "metrics")]
                {
                    let mut metrics = metrics.borrow_mut();
                    metrics.channel_waiter_entries_examined += state.senders.len();
                    metrics.peak_channel_waiters =
                        metrics.peak_channel_waiters.max(state.senders.len());
                }
                state
                    .senders
                    .retain(|(candidate, _)| !candidate.is_same(waiter));
            }
            Self::ChannelReceive(channel) => {
                let mut state = channel.state.lock();
                #[cfg(feature = "metrics")]
                {
                    let mut metrics = metrics.borrow_mut();
                    metrics.channel_waiter_entries_examined += state.receivers.len();
                    metrics.peak_channel_waiters =
                        metrics.peak_channel_waiters.max(state.receivers.len());
                }
                state
                    .receivers
                    .retain(|candidate| !candidate.is_same(waiter));
            }
            #[cfg(feature = "concurrency")]
            Self::TaskAwait(target) => {
                let mut state = target.state.borrow_mut();
                #[cfg(feature = "metrics")]
                {
                    let mut metrics = metrics.borrow_mut();
                    metrics.task_waiter_entries_examined += state.waiters.len();
                    metrics.peak_task_waiters = metrics.peak_task_waiters.max(state.waiters.len());
                }
                state.waiters.retain(|candidate| !candidate.is_same(waiter));
            }
            #[cfg(feature = "concurrency")]
            Self::Nursery { body, cancel } => {
                body.state
                    .borrow_mut()
                    .waiters
                    .retain(|candidate| !candidate.is_same(waiter));
                // A settled body has no children left to cancel.
                cancel();
            }
            #[cfg(feature = "concurrency")]
            Self::Timer(registration) => registration.remove(waiter),
        }
    }
}

/// All queues a suspended execution currently occupies. A regular blocking
/// operation has one entry; a future select owns several and removes losers
/// before its winner resumes.
#[derive(Clone)]
pub(crate) struct WaitSet {
    registrations: Vec<WaitRegistration>,
    #[cfg(feature = "metrics")]
    metrics: Rc<RefCell<crate::vm::VmMetrics>>,
}

impl WaitSet {
    pub(crate) fn many(
        registrations: Vec<WaitRegistration>,
        #[cfg(feature = "metrics")] metrics: Rc<RefCell<crate::vm::VmMetrics>>,
    ) -> Self {
        Self {
            registrations,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub(crate) fn select_state(waiter: Waiter) -> Rc<RefCell<SelectWaitState>> {
        Rc::new(RefCell::new(SelectWaitState {
            waiter,
            claimed: Arc::new(core::sync::atomic::AtomicBool::new(false)),
            registrations: None,
        }))
    }

    pub(crate) fn set_select_registrations(
        state: &Rc<RefCell<SelectWaitState>>,
        registrations: WaitSet,
    ) {
        state.borrow_mut().registrations = Some(registrations);
    }

    fn remove(self, waiter: &Waiter) {
        #[cfg(feature = "metrics")]
        {
            self.metrics.borrow_mut().wait_registration_removals += self.registrations.len();
        }
        for registration in self.registrations {
            registration.remove(
                waiter,
                #[cfg(feature = "metrics")]
                &self.metrics,
            );
        }
    }

    #[cfg(feature = "concurrency")]
    fn remove_losers(self, task: &Task) {
        if self.registrations.len() > 1 {
            self.remove(&Waiter::task(task));
        }
    }

    pub(crate) fn remove_for_waiter(self, waiter: &Waiter) {
        self.remove(waiter);
    }
}

#[derive(Clone)]
pub(crate) struct RootWaiter {
    state: Rc<RefCell<RootWaiterState>>,
}

struct RootWaiterState {
    resume: Option<Result<Value, crate::RuntimeError>>,
    closed_send_error: Option<crate::RuntimeError>,
}

impl RootWaiter {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(RootWaiterState {
                resume: None,
                closed_send_error: None,
            })),
        }
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        self.state.borrow_mut().resume = Some(result);
    }

    pub(crate) fn set_closed_send_error(&self, error: crate::RuntimeError) {
        self.state.borrow_mut().closed_send_error = Some(error);
    }

    pub(crate) fn reject_closed_send(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.closed_send_error.clone() {
            state.resume = Some(Err(error));
        }
    }

    pub(crate) fn take_resume(&self) -> Option<Result<Value, crate::RuntimeError>> {
        self.state.borrow_mut().resume.take()
    }
}

impl Channel {
    /// Whether a fork has used the channel, so what is sent on it may reach
    /// one.
    #[cfg(feature = "concurrency")]
    pub(crate) fn crossed(&self) -> bool {
        self.crossed.get()
    }

    #[cfg(feature = "concurrency")]
    pub(crate) fn mark_crossed(&self) {
        self.crossed.set(true);
    }

    #[must_use]
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            state: Rc::new(crate::sync::Lock::new(ChannelState {
                capacity,
                messages: VecDeque::new(),
                senders: VecDeque::new(),
                receivers: VecDeque::new(),
                closed: false,
            })),
            native_producer: crate::sync::Once::new(),
            #[cfg(feature = "concurrency")]
            crossed: crate::sync::Flag::new(false),
        }
    }

    pub(crate) fn native(capacity: usize) -> (Self, NativeChannelProducer) {
        let producer = NativeChannelProducer::bounded(capacity);
        (
            Self {
                state: Rc::new(crate::sync::Lock::new(ChannelState {
                    capacity,
                    messages: VecDeque::new(),
                    senders: VecDeque::new(),
                    receivers: VecDeque::new(),
                    closed: false,
                })),
                native_producer: crate::sync::Once::from(producer.receiver_handle()),
                #[cfg(feature = "concurrency")]
                crossed: crate::sync::Flag::new(false),
            },
            producer,
        )
    }

    /// Mints a producer lease for native code. The first mint attaches a
    /// producer queue whose occupancy counts the messages already buffered,
    /// so native and Slug sends share the channel's capacity. Returns `None`
    /// for an unbuffered channel, which no native send could ever enter.
    pub(crate) fn attach_native_producer(&self) -> Option<NativeChannelProducer> {
        let state = self.state.lock();
        if state.capacity == 0 {
            return None;
        }
        if let Some(existing) = self.native_producer.get() {
            return Some(existing.producer_lease());
        }
        let producer =
            NativeChannelProducer::bounded_with_occupied(state.capacity, state.messages.len());
        if state.closed {
            producer.close();
        }
        // Published before the lock is released, so a second attach on
        // another thread leases this producer rather than making one the
        // channel never drains.
        let _ = self.native_producer.set(producer.receiver_handle());
        drop(state);
        Some(producer)
    }

    pub(crate) fn has_native_producer(&self) -> bool {
        self.native_producer.get().is_some()
    }

    pub(crate) fn revoke_native_producer(&self) {
        if let Some(producer) = self.native_producer.get() {
            producer.close();
        }
    }

    pub(crate) fn register_progress_signal(&self, signal: &Arc<ProgressSignal>) {
        if let Some(producer) = self.native_producer.get() {
            producer.register_progress_signal(signal);
        }
    }

    #[cfg_attr(
        all(target_os = "none", not(feature = "concurrency")),
        allow(dead_code)
    )]
    pub(crate) fn has_live_native_producer(&self) -> bool {
        self.native_producer
            .get()
            .is_some_and(NativeChannelProducer::has_external_producer)
    }

    pub(crate) fn reserve_buffer_slot(&self) -> bool {
        self.native_producer
            .get()
            .is_none_or(NativeChannelProducer::reserve_slot)
    }

    pub(crate) fn release_buffer_slot(&self) {
        if let Some(producer) = self.native_producer.get() {
            producer.release_slot();
        }
    }

    pub(crate) fn drain_native(&self) -> bool {
        let Some(producer) = self.native_producer.get() else {
            return false;
        };
        // Drained under the channel's lock, so two threads draining at once
        // cannot buffer the producer's values out of order. The producer
        // never takes this lock, so the order of the two locks is fixed.
        let mut state = self.state.lock();
        // Read before draining: a producer closes after its last send, so
        // every value it sent is then queued and drains with the others.
        let producer_closed = producer.is_closed();
        let values = producer.drain(usize::MAX);
        // A channel closed by an earlier drain is no progress; reporting it
        // as progress forever would keep the scheduler from running tasks.
        let changed = !values.is_empty() || (producer_closed && !state.closed);
        let mut wakes = Vec::new();
        for value in values {
            if let Some(receiver) = pop_receiver(&mut state) {
                self.release_buffer_slot();
                wakes.push(ChannelWake::Resume(receiver, value.into_value()));
            } else {
                state.messages.push_back(value.into_value());
            }
        }
        if producer_closed {
            state.closed = true;
            close_waiters(&mut state, &mut wakes);
        }
        drop(state);
        let changed = changed || !wakes.is_empty();
        run_wakes(wakes);
        changed
    }

    /// The channel's state, locked: a `select` about to wait holds the locks
    /// of all its channels while it checks them once more and parks on
    /// them, so no send or receive can slip between the check and the park.
    pub(crate) fn lock_state(&self) -> crate::sync::LockGuard<'_, ChannelState> {
        self.state.lock()
    }

    pub(crate) fn try_send(&self, value: &Value) -> ChannelSend {
        self.drain_native();
        let mut wakes = Vec::new();
        let outcome = self.send_locked(&mut self.state.lock(), value, &mut wakes);
        run_wakes(wakes);
        outcome
    }

    /// Sends `value` on the locked `state`, if a receiver or buffer space
    /// takes it, adding the wakes it owes to `wakes`.
    pub(crate) fn send_locked(
        &self,
        state: &mut ChannelState,
        value: &Value,
        wakes: &mut Vec<ChannelWake>,
    ) -> ChannelSend {
        if state.closed {
            return ChannelSend::Closed;
        }
        if let Some(receiver) = pop_receiver(state) {
            wakes.push(ChannelWake::Resume(receiver, value.clone()));
            return ChannelSend::Ready;
        }
        if state.messages.len() < state.capacity && self.reserve_buffer_slot() {
            state.messages.push_back(value.clone());
            return ChannelSend::Ready;
        }
        ChannelSend::Pending
    }

    pub(crate) fn park_sender(state: &mut ChannelState, waiter: Waiter, value: Value) {
        state.senders.push_back((waiter, value));
    }

    pub(crate) fn try_receive(&self) -> ChannelReceive {
        self.drain_native();
        let mut wakes = Vec::new();
        let outcome = self.receive_locked(&mut self.state.lock(), &mut wakes);
        run_wakes(wakes);
        outcome.map_or(ChannelReceive::Pending, ChannelReceive::Ready)
    }

    /// Receives from the locked `state`, if a message, a waiting sender, or
    /// the channel's closing gives a value, adding the wakes it owes.
    pub(crate) fn receive_locked(
        &self,
        state: &mut ChannelState,
        wakes: &mut Vec<ChannelWake>,
    ) -> Option<Value> {
        if let Some(value) = state.messages.pop_front() {
            self.release_buffer_slot();
            if !state.senders.is_empty() && self.reserve_buffer_slot() {
                if let Some((sender, pending)) = pop_sender(state) {
                    state.messages.push_back(pending);
                    wakes.push(ChannelWake::Resume(sender, Value::Nil));
                } else {
                    self.release_buffer_slot();
                }
            }
            return Some(value);
        }
        if let Some((sender, value)) = pop_sender(state) {
            wakes.push(ChannelWake::Resume(sender, Value::Nil));
            return Some(value);
        }
        state.closed.then_some(Value::Nil)
    }

    pub(crate) fn park_receiver(state: &mut ChannelState, waiter: Waiter) {
        state.receivers.push_back(waiter);
    }

    pub(crate) fn close(&self) {
        // Under the lock that publishes a first producer, so one attached
        // as the channel closes is either revoked here or made closed.
        let mut state = self.state.lock();
        self.revoke_native_producer();
        if state.closed {
            return;
        }
        state.closed = true;
        let mut wakes = Vec::new();
        close_waiters(&mut state, &mut wakes);
        drop(state);
        run_wakes(wakes);
    }
}

/// A wake a channel owes a waiter it has claimed, run once its lock is
/// released.
pub(crate) enum ChannelWake {
    Resume(Waiter, Value),
    Reject(Waiter),
}

pub(crate) fn run_wakes(wakes: Vec<ChannelWake>) {
    for wake in wakes {
        match wake {
            ChannelWake::Resume(waiter, value) => waiter.resume_claimed(Ok(value)),
            ChannelWake::Reject(waiter) => waiter.reject_claimed(),
        }
    }
}

/// The first parked receiver this channel can claim. One already claimed,
/// by another case of its `select`, is stale and dropped.
fn pop_receiver(state: &mut ChannelState) -> Option<Waiter> {
    while let Some(receiver) = state.receivers.pop_front() {
        if receiver.try_claim() {
            return Some(receiver);
        }
    }
    None
}

fn pop_sender(state: &mut ChannelState) -> Option<(Waiter, Value)> {
    while let Some((sender, value)) = state.senders.pop_front() {
        if sender.try_claim() {
            return Some((sender, value));
        }
    }
    None
}

/// Wakes every waiter a closing channel can claim: receivers with `nil`,
/// senders with the closed-send error.
fn close_waiters(state: &mut ChannelState, wakes: &mut Vec<ChannelWake>) {
    for receiver in state.receivers.drain(..) {
        if receiver.try_claim() {
            wakes.push(ChannelWake::Resume(receiver, Value::Nil));
        }
    }
    for (sender, _) in state.senders.drain(..) {
        if sender.try_claim() {
            wakes.push(ChannelWake::Reject(sender));
        }
    }
}

impl fmt::Debug for Channel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<chan>")
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.revoke_native_producer();
    }
}

/// Shared storage for a lexical binding captured by one or more closures.
pub(crate) type BindingCell = crate::sync::Code<crate::sync::SoloCell<Value>>;
pub(crate) type GlobalEnvironment = crate::sync::Code<crate::sync::SoloCell<GlobalTable>>;

pub(crate) fn global_environment() -> GlobalEnvironment {
    crate::sync::Code::new(crate::sync::SoloCell::new(GlobalTable::default()))
}

/// Global bindings stored in definition order behind a name index.
///
/// Bindings are never removed, so a slot stays valid for the table's lifetime
/// once a name resolves to it; redefining a name replaces its value in place.
/// This lets compiled code cache a slot per global name instead of hashing
/// the name on every read.
#[derive(Clone, Debug, Default)]
pub(crate) struct GlobalTable {
    slots: NameMap<u32>,
    entries: Vec<(String, Value)>,
}

impl GlobalTable {
    pub(crate) fn slot(&self, name: &str) -> Option<u32> {
        self.slots.get(name).copied()
    }

    /// The value in a slot that `slot` returned for this table.
    #[inline]
    pub(crate) fn value(&self, slot: u32) -> &Value {
        &self.entries[slot as usize].1
    }

    /// Moves the plain entry in `slot` into a binding cell of `kind`, so
    /// assigning the variable later changes the cell and never the table,
    /// which forks on other threads may be reading. A binding cell already
    /// there is marked as `kind` and returned.
    #[cfg(feature = "concurrency")]
    pub(crate) fn bind_variable(
        &mut self,
        slot: u32,
        kind: crate::variables::BindingKind,
    ) -> Option<BindingCell> {
        let (name, value) = self.entries.get_mut(slot as usize)?;
        if let Value::Binding { name, cell } = value {
            name.set_kind(kind);
            return Some(cell.clone());
        }
        let cell = binding_cell(core::mem::replace(value, Value::Nil));
        *value = Value::Binding {
            name: crate::sync::Code::new(crate::variables::BindingName::new(name.clone(), kind)),
            cell: cell.clone(),
        };
        Some(cell)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        self.slot(name).map(|slot| self.value(slot))
    }

    pub(crate) fn contains_key(&self, name: &str) -> bool {
        self.slots.contains_key(name)
    }

    /// Binds `name`, replacing the value of an existing binding in place.
    pub(crate) fn insert(&mut self, name: String, value: Value) {
        if let Some(slot) = self.slot(&name) {
            self.entries[slot as usize].1 = value;
        } else {
            let slot = u32::try_from(self.entries.len()).expect("too many global bindings");
            self.slots.insert(name.clone(), slot);
            self.entries.push((name, value));
        }
    }

    /// Binds `name` only if it is not already bound.
    pub(crate) fn insert_if_absent(&mut self, name: String, value: impl FnOnce() -> Value) {
        if !self.contains_key(&name) {
            self.insert(name, value());
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(name, value)| (name, value))
    }
}

impl Extend<(String, Value)> for GlobalTable {
    fn extend<I: IntoIterator<Item = (String, Value)>>(&mut self, entries: I) {
        for (name, value) in entries {
            self.insert(name, value);
        }
    }
}

/// One cached `GlobalTable` slot per global name of a program, filled on the
/// first read of each name. Valid only for one program and one environment.
pub(crate) type GlobalSlots = Rc<[crate::sync::Hint]>;

/// A `GlobalSlots` entry whose name has not been resolved yet.
pub(crate) const UNRESOLVED_GLOBAL: u32 = u32::MAX;

pub(crate) fn global_slots(program: &crate::Program) -> GlobalSlots {
    (0..program.global_name_count())
        .map(|_| crate::sync::Hint::new(UNRESOLVED_GLOBAL))
        .collect()
}

pub(crate) fn binding_cell(value: Value) -> BindingCell {
    crate::sync::Code::new(crate::sync::SoloCell::new(value))
}

pub(crate) fn module_binding(name: impl Into<String>) -> Value {
    let name = crate::sync::Code::new(crate::variables::BindingName::new(
        name.into(),
        crate::variables::BindingKind::default(),
    ));
    Value::Binding {
        name,
        cell: binding_cell(Value::Uninitialized),
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub(crate) chunk: usize,
    pub(crate) captures: Vec<BindingCell>,
    /// The program the closure's chunk belongs to, and the global table its
    /// names resolve in. A call's frame reads both from here, so entering a
    /// function counts neither: forks on other threads share them.
    pub(crate) program: crate::sync::Code<crate::Program>,
    pub(crate) globals: GlobalEnvironment,
    /// Slots in `globals` already resolved for `program`'s global names,
    /// shared by closures created with the same program and environment.
    pub(crate) global_slots: Option<GlobalSlots>,
    #[cfg(feature = "concurrency")]
    pub(crate) capture_sources: Vec<crate::Capture>,
    /// The fork domain of the task that made the closure, or zero.
    #[cfg(feature = "concurrency")]
    pub(crate) domain: u32,
}

/// A cached task completion. Tasks are runtime-owned and retain their outcome
/// so repeated awaits can observe the same settlement.
#[cfg(feature = "concurrency")]
#[derive(Clone)]
pub struct Task {
    state: Rc<RefCell<TaskState>>,
    /// The fork domain of the task that started this one, which alone may
    /// use its handle; zero outside forks.
    domain: u32,
}

#[cfg(feature = "concurrency")]
struct TaskState {
    phase: TaskPhase,
    admission: Option<TaskAdmission>,
    admitted: bool,
    observed: bool,
    ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    waiters: Vec<Waiter>,
    wait_registration: Option<WaitSet>,
    /// The explicit nursery this task is a child of, told when it settles.
    watch: Option<Rc<NurseryWatch>>,
}

/// What the body of an explicit nursery, once it has returned, waits on:
/// its children all settling, or one failing, which may fail it fast.
#[cfg(feature = "concurrency")]
#[derive(Default)]
pub(crate) struct NurseryWatch {
    pending: Cell<usize>,
    failed: Cell<bool>,
    settling: RefCell<Option<Rc<Task>>>,
}

#[cfg(feature = "concurrency")]
impl NurseryWatch {
    /// How many children have yet to settle.
    pub(crate) fn pending(&self) -> usize {
        self.pending.get()
    }

    /// Whether a child has failed since the last call.
    pub(crate) fn take_failed(&self) -> bool {
        self.failed.replace(false)
    }

    /// Wakes `body`, suspended to settle, once a child fails or the last
    /// one settles.
    pub(crate) fn wait(&self, body: Rc<Task>) {
        *self.settling.borrow_mut() = Some(body);
    }

    fn settled(&self, failed: bool) {
        self.pending.set(self.pending.get().saturating_sub(1));
        if failed {
            self.failed.set(true);
        }
        if failed || self.pending.get() == 0 {
            let body = self.settling.borrow_mut().take();
            if let Some(body) = body {
                // Ahead of every sibling, which a failure may cancel before
                // it runs, as when settlement looked between turns.
                body.resume_at(Ok(Value::Nil), failed);
            }
        }
    }
}

#[cfg(feature = "concurrency")]
pub(crate) fn task_state_layout() -> (usize, usize) {
    (mem::size_of::<TaskState>(), mem::align_of::<TaskState>())
}

#[cfg(feature = "concurrency")]
enum TaskPhase {
    Pending(Box<crate::vm::TaskExecution>),
    Running,
    /// A fork running on its own island, on a worker thread.
    #[cfg(all(feature = "threads", not(target_os = "none")))]
    Remote(Arc<crate::vm::island::IslandControl>),
    Settled(Result<Value, crate::RuntimeError>),
}

#[derive(Clone)]
#[cfg(feature = "concurrency")]
pub(crate) struct TaskObserver(Weak<RefCell<TaskState>>);

#[cfg(feature = "concurrency")]
impl TaskObserver {
    fn observe(&self) {
        if let Some(state) = self.0.upgrade() {
            state.borrow_mut().observed = true;
        }
    }
}

#[cfg(feature = "concurrency")]
impl fmt::Debug for Task {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<task>")
    }
}

#[cfg(feature = "concurrency")]
#[derive(Clone, Debug)]
pub(crate) struct TaskAdmission {
    pub(crate) limit: usize,
    pub(crate) count: Rc<Cell<usize>>,
}

#[cfg(feature = "concurrency")]
impl Task {
    #[must_use]
    pub(crate) fn pending(
        execution: crate::vm::TaskExecution,
        admission: Option<TaskAdmission>,
        ready: Rc<RefCell<VecDeque<Rc<Task>>>>,
    ) -> Self {
        let admitted = admission.as_ref().is_none_or(|admission| {
            if admission.count.get() < admission.limit {
                admission.count.set(admission.count.get() + 1);
                true
            } else {
                false
            }
        });
        Self {
            state: Rc::new(RefCell::new(TaskState {
                phase: TaskPhase::Pending(Box::new(execution)),
                admission,
                admitted,
                observed: false,
                ready,
                waiters: Vec::new(),
                wait_registration: None,
                watch: None,
            })),
            domain: 0,
        }
    }

    /// This task, whose handle belongs to the fork `domain`.
    pub(crate) fn in_domain(mut self, domain: u32) -> Self {
        self.domain = domain;
        self
    }

    pub(crate) fn domain(&self) -> u32 {
        self.domain
    }

    /// Makes this task a child its explicit nursery waits on.
    pub(crate) fn join(&self, watch: Rc<NurseryWatch>) {
        watch.pending.set(watch.pending.get() + 1);
        self.state.borrow_mut().watch = Some(watch);
    }

    pub(crate) fn take_pending(&self, task: &Rc<Task>) -> Option<crate::vm::TaskExecution> {
        let mut state = self.state.borrow_mut();
        let phase = mem::replace(&mut state.phase, TaskPhase::Running);
        match phase {
            TaskPhase::Pending(mut execution) => {
                execution.set_current_task(task);
                Some(*execution)
            }
            phase => {
                state.phase = phase;
                None
            }
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self.state.borrow().phase, TaskPhase::Running)
    }

    /// Whether the task waits here for its scheduler to run it.
    pub(crate) fn is_queued(&self) -> bool {
        matches!(self.state.borrow().phase, TaskPhase::Pending(_))
    }

    /// Takes the execution of a fork its nursery has admitted, to run on an
    /// island, leaving the task waiting on `control`.
    #[cfg(all(feature = "threads", not(target_os = "none")))]
    pub(crate) fn take_for_island(
        &self,
        control: &Arc<crate::vm::island::IslandControl>,
    ) -> Option<crate::vm::TaskExecution> {
        let mut state = self.state.borrow_mut();
        if !state.admitted
            || !matches!(&state.phase, TaskPhase::Pending(execution) if execution.is_fork())
        {
            return None;
        }
        let TaskPhase::Pending(execution) =
            mem::replace(&mut state.phase, TaskPhase::Remote(control.clone()))
        else {
            unreachable!("the phase was checked");
        };
        Some(*execution)
    }

    /// Settles a fork whose island has finished, unless it was cancelled.
    #[cfg(all(feature = "threads", not(target_os = "none")))]
    pub(crate) fn complete_remote(&self, outcome: &Result<Value, crate::RuntimeError>) {
        let mut state = self.state.borrow_mut();
        if !matches!(state.phase, TaskPhase::Remote(_)) {
            return;
        }
        state.phase = TaskPhase::Settled(outcome.clone());
        release_admission(&mut state);
        let waiters = mem::take(&mut state.waiters);
        let watch = state.watch.take();
        drop(state);
        for waiter in waiters {
            waiter.resume(outcome.clone());
        }
        if let Some(watch) = watch {
            watch.settled(outcome.is_err());
        }
    }

    pub(crate) fn try_admit(&self) -> bool {
        let mut state = self.state.borrow_mut();
        if !matches!(state.phase, TaskPhase::Pending(_)) || state.admitted {
            return state.admitted;
        }
        let Some(admission) = &state.admission else {
            state.admitted = true;
            return true;
        };
        if admission.count.get() >= admission.limit {
            return false;
        }
        admission.count.set(admission.count.get() + 1);
        state.admitted = true;
        true
    }

    pub(crate) fn complete(&self, outcome: &Result<Value, crate::RuntimeError>) {
        let mut state = self.state.borrow_mut();
        debug_assert!(matches!(state.phase, TaskPhase::Running));
        state.phase = TaskPhase::Settled(outcome.clone());
        state.wait_registration = None;
        release_admission(&mut state);
        let waiters = mem::take(&mut state.waiters);
        let watch = state.watch.take();
        drop(state);
        // Waiters first: one that observes the outcome keeps a failure from
        // failing the nursery fast.
        for waiter in waiters {
            waiter.resume(outcome.clone());
        }
        if let Some(watch) = watch {
            watch.settled(outcome.is_err());
        }
    }

    pub(crate) fn suspend(
        &self,
        execution: crate::vm::TaskExecution,
        wait_registration: Option<WaitSet>,
    ) {
        let mut state = self.state.borrow_mut();
        debug_assert!(matches!(state.phase, TaskPhase::Running));
        state.phase = TaskPhase::Pending(Box::new(execution));
        state.wait_registration = wait_registration;
    }

    /// Puts a task that yielded straight back in its ready queue: it waits
    /// on nothing, and resumes where it stopped.
    #[cfg(all(
        feature = "concurrency",
        not(all(feature = "threads", not(target_os = "none")))
    ))]
    pub(crate) fn requeue(&self) {
        let ready = self.state.borrow().ready.clone();
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    /// Whether `other` is a handle on this same task.
    #[cfg(all(
        feature = "concurrency",
        not(all(feature = "threads", not(target_os = "none")))
    ))]
    pub(crate) fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    /// The fork domain the code of a task waiting here runs in.
    #[cfg(all(
        feature = "concurrency",
        not(all(feature = "threads", not(target_os = "none")))
    ))]
    pub(crate) fn queued_domain(&self) -> Option<u32> {
        match &self.state.borrow().phase {
            TaskPhase::Pending(execution) => Some(execution.domain()),
            _ => None,
        }
    }

    pub(crate) fn resume(&self, result: Result<Value, crate::RuntimeError>) {
        self.resume_at(result, false);
    }

    /// Resumes the task, at the front of its ready queue when `first`.
    fn resume_at(&self, result: Result<Value, crate::RuntimeError>, first: bool) {
        let mut state = self.state.borrow_mut();
        let wait_registration = state.wait_registration.take();
        let TaskPhase::Pending(execution) = &mut state.phase else {
            return;
        };
        execution.resume(result);
        let ready = state.ready.clone();
        drop(state);
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_losers(self);
        }
        let task = Rc::new(self.clone());
        if first {
            ready.borrow_mut().push_front(task);
        } else {
            ready.borrow_mut().push_back(task);
        }
    }

    pub(crate) fn reject_closed_send(&self) {
        let mut state = self.state.borrow_mut();
        let wait_registration = state.wait_registration.take();
        let TaskPhase::Pending(execution) = &mut state.phase else {
            return;
        };
        execution.reject_closed_send();
        let ready = state.ready.clone();
        drop(state);
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_losers(self);
        }
        ready.borrow_mut().push_back(Rc::new(self.clone()));
    }

    pub(crate) fn wait_for(&self, waiter: Waiter) {
        self.state.borrow_mut().waiters.push(waiter);
    }

    pub(crate) fn observe(&self) {
        self.state.borrow_mut().observed = true;
    }

    #[cfg(feature = "concurrency")]
    pub(crate) fn observer(&self) -> TaskObserver {
        TaskObserver(Rc::downgrade(&self.state))
    }

    pub(crate) fn outcome(&self) -> Option<Result<Value, crate::RuntimeError>> {
        match &self.state.borrow().phase {
            TaskPhase::Settled(outcome) => Some(outcome.clone()),
            _ => None,
        }
    }

    /// For a task still suspended, the error reporting that it can never
    /// resume, located where it waits.
    pub(crate) fn blocked_error(&self) -> Option<crate::RuntimeError> {
        match &self.state.borrow().phase {
            TaskPhase::Pending(execution) => Some(execution.blocked_error()),
            _ => None,
        }
    }

    pub(crate) fn cancel(&self, error: &crate::RuntimeError) {
        let mut state = self.state.borrow_mut();
        let phase = mem::replace(&mut state.phase, TaskPhase::Running);
        match phase {
            TaskPhase::Pending(_) => {}
            // The island stops at its next scheduling point; its result,
            // when it comes, is dropped.
            #[cfg(all(feature = "threads", not(target_os = "none")))]
            TaskPhase::Remote(control) => control.cancel(),
            phase => {
                state.phase = phase;
                return;
            }
        }
        let wait_registration = state.wait_registration.take();
        let waiters = mem::take(&mut state.waiters);
        let ready = state.ready.clone();
        let watch = state.watch.take();
        state.phase = TaskPhase::Settled(Err(error.clone()));
        release_admission(&mut state);
        drop(state);
        ready
            .borrow_mut()
            .retain(|candidate| !Rc::ptr_eq(&candidate.state, &self.state));
        if let Some(wait_registration) = wait_registration {
            wait_registration.remove_for_waiter(&Waiter::task(self));
        }
        for waiter in waiters {
            waiter.resume(Err(error.clone()));
        }
        if let Some(watch) = watch {
            watch.settled(true);
        }
    }

    /// Whether the task has settled with nothing left for its nursery to
    /// report: a value, or a failure something has awaited.
    pub(crate) fn is_done_with(&self) -> bool {
        let state = self.state.borrow();
        match &state.phase {
            TaskPhase::Settled(Ok(_)) => true,
            TaskPhase::Settled(Err(_)) => state.observed,
            _ => false,
        }
    }

    pub(crate) fn unobserved_error(&self) -> Option<crate::RuntimeError> {
        let state = self.state.borrow();
        if state.observed {
            None
        } else {
            match &state.phase {
                TaskPhase::Settled(Err(error)) => Some(error.clone()),
                _ => None,
            }
        }
    }
}

#[cfg(feature = "concurrency")]
fn release_admission(state: &mut TaskState) {
    if state.admitted {
        if let Some(admission) = &state.admission {
            admission.count.set(admission.count.get().saturating_sub(1));
        }
        state.admitted = false;
    }
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub(crate) name: Rc<str>,
    pub(crate) default: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct StructSchema {
    pub(crate) fields: Vec<StructField>,
}

#[derive(Clone, Debug)]
pub struct StructValue {
    pub(crate) schema: Rc<StructSchema>,
    pub(crate) values: Vec<Value>,
}

impl Drop for StructValue {
    /// As a list's storage does: nested values it alone holds are taken
    /// apart with a loop.
    fn drop(&mut self) {
        drop_values(&mut self.values);
    }
}

/// Drops `values`, which a list's or a struct's storage holds.
pub(crate) fn drop_values(values: &mut Vec<Value>) {
    guarded_drop(core::mem::take(values), |values, deferred| {
        deferred.extend(values);
    });
}

/// How many containers a drop goes inside each other before it defers what
/// it reaches.
#[cfg(not(target_os = "none"))]
const DROP_DEPTH: u32 = 128;

#[cfg(not(target_os = "none"))]
std::thread_local! {
    /// How many container drops this thread is inside.
    static DROPPING: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    /// What drops deeper than [`DROP_DEPTH`] left for the outermost one.
    static DEFERRED: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

/// Drops a container's `items`, which `spill` turns into values. A drop
/// recurses into nested containers as a vector's does, at a cost per
/// container rather than per element; past [`DROP_DEPTH`] it sets what it
/// reaches aside, and the outermost drop frees that in batches, each as
/// shallow, so a value nested deeper than the native stack can hold is
/// still dropped.
#[cfg(not(target_os = "none"))]
pub(crate) fn guarded_drop<T>(items: Vec<T>, spill: impl FnOnce(Vec<T>, &mut Vec<Value>)) {
    if items.is_empty() {
        return;
    }
    // A thread being torn down drops as a vector does.
    let Ok(depth) = DROPPING.try_with(core::cell::Cell::get) else {
        return;
    };
    if depth >= DROP_DEPTH {
        DEFERRED.with(|deferred| spill(items, &mut deferred.borrow_mut()));
        return;
    }
    DROPPING.with(|dropping| dropping.set(depth + 1));
    drop(items);
    if depth == 0 {
        loop {
            let batch = DEFERRED.with(|deferred| core::mem::take(&mut *deferred.borrow_mut()));
            if batch.is_empty() {
                break;
            }
            drop(batch);
        }
    }
    DROPPING.with(|dropping| dropping.set(depth));
}

/// [`guarded_drop`] on a freestanding build, which has no thread-locals:
/// it takes apart the nested values it alone holds with [`dismantle`].
#[cfg(target_os = "none")]
pub(crate) fn guarded_drop<T>(items: Vec<T>, spill: impl FnOnce(Vec<T>, &mut Vec<Value>)) {
    let mut values = Vec::new();
    spill(items, &mut values);
    if values.iter().any(Value::holds_values) {
        dismantle(values);
    }
}

/// Drops `pending` and everything in it that nothing else holds, taking
/// nested lists, maps, and structs apart one at a time from a stack of its
/// own, so the native stack does not grow with the value's depth.
pub(crate) fn dismantle(mut pending: Vec<Value>) {
    while let Some(value) = pending.pop() {
        match value {
            Value::List(list) => list.release_into(&mut pending),
            Value::Map(map) => map.release_into(&mut pending),
            Value::Struct(mut value) => {
                if let Some(value) = Rc::get_mut(&mut value) {
                    pending.append(&mut value.values);
                }
            }
            Value::Closure(mut closure) => {
                if let Some(closure) = crate::sync::Code::get_mut(&mut closure) {
                    release_captures(core::mem::take(&mut closure.captures), &mut pending);
                }
            }
            _ => {}
        }
    }
}

/// Hands the values of the capture cells a closure alone holds to
/// `pending`, for [`dismantle`] or a deferred drop.
fn release_captures(captures: Vec<BindingCell>, pending: &mut Vec<Value>) {
    for mut cell in captures {
        if let Some(cell) = crate::sync::Code::get_mut(&mut cell) {
            pending.push(core::mem::replace(&mut *cell.borrow_mut(), Value::Nil));
        }
    }
}

/// Drops `value` with the work list freestanding builds use, which hosted
/// builds otherwise do not: for tests, which cannot run on a freestanding
/// build, to exercise it on a deep value.
#[doc(hidden)]
pub fn drop_with_work_list(value: Value) {
    dismantle(alloc::vec![value]);
}

impl Drop for Closure {
    /// As a list's storage does: a chain of closures capturing values that
    /// hold closures drops without the native stack growing with it.
    fn drop(&mut self) {
        guarded_drop(core::mem::take(&mut self.captures), release_captures);
    }
}

impl StructValue {
    /// Takes the value of field `name` out of a struct nothing else holds,
    /// leaving `nil`; `None` when it is shared or has no such field.
    pub(crate) fn take_field(value: &mut Rc<Self>, name: &str) -> Option<Value> {
        let value = Rc::get_mut(value)?;
        let index = value
            .schema
            .fields
            .iter()
            .position(|field| field.name.as_ref() == name)?;
        Some(core::mem::replace(&mut value.values[index], Value::Nil))
    }

    pub(crate) fn copy_fields(
        self: Rc<Self>,
        names: &[String],
        replacements: &[Value],
    ) -> Result<Rc<Self>, String> {
        for (index, name) in names.iter().enumerate() {
            if names[..index].contains(name) {
                return Err(format!("duplicate struct field '{name}'"));
            }
            if !self
                .schema
                .fields
                .iter()
                .any(|field| field.name.as_ref() == name)
            {
                return Err(format!("struct has no field '{name}'"));
            }
        }
        let mut copy = Rc::try_unwrap(self).unwrap_or_else(|value| (*value).clone());
        for (name, replacement) in names.iter().zip(replacements) {
            let index = copy
                .schema
                .fields
                .iter()
                .position(|field| field.name.as_ref() == name)
                .expect("field names were validated");
            copy.values[index] = replacement.clone();
        }
        Ok(Rc::new(copy))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EnumValue {
    #[doc(hidden)]
    pub module: Rc<str>,
    #[doc(hidden)]
    pub name: Rc<str>,
    #[doc(hidden)]
    pub case: Rc<str>,
}

/// A foreign declaration's private identity and resource positions, shared
/// behind one `Rc` so a declared native stays within the 24-byte `Value`.
#[derive(Debug)]
#[doc(hidden)]
pub struct ForeignDeclaration {
    pub(crate) identity: CallableIdentity,
    pub(crate) resources: ForeignResourceSignature,
}

/// The dynamic values used by the initial Slug VM core.
///
/// Collections are reference-counted so closures and later concurrency support
/// can share them without requiring a copying garbage collector.
///
/// On 64-bit targets the tag fills a word. Every payload then starts at the
/// next word, so a value copies as three words instead of a tag byte, seven
/// payload bytes that only `Bool` and `Builtin` use, and two words. The
/// payloads are word-aligned there anyway, so the size stays 24 bytes; on
/// 32-bit targets a word tag would grow the value, so it keeps the default.
#[derive(Clone)]
#[cfg_attr(target_pointer_width = "64", repr(u64))]
pub enum Value {
    /// Internal marker for a predeclared module binding that has not run yet.
    Uninitialized,
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Str),
    Bytes(Rc<Vec<u8>>),
    List(List),
    Map(Map),
    StructSchema(Rc<StructSchema>),
    Struct(Rc<StructValue>),
    Enum(Rc<EnumValue>),
    Channel(Rc<Channel>),
    Closure(crate::sync::Code<Closure>),
    #[cfg(feature = "concurrency")]
    Task(Rc<Task>),
    Native(NativeFunction),
    /// A host callable paired with the source declaration's private identity.
    DeclaredNative {
        function: NativeFunction,
        declaration: crate::sync::Code<ForeignDeclaration>,
    },
    NativeResource(Rc<NativeResource>),
    Builtin(Builtin),
    /// Private callable group assembled by a multi-module import.
    Overloads(Rc<Vec<Value>>),
    /// A live module binding exposed through an import map.
    Binding {
        /// The binding's name and kind, shared by every module that imports
        /// it.
        name: crate::sync::Code<crate::variables::BindingName>,
        cell: BindingCell,
    },
}

/// Runtime-neutral value categories for source semantic conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum ValueKind {
    Nil,
    Bool,
    Num,
    Str,
    Bytes,
    List,
    Map,
    StructSchema,
    Struct,
    Enum,
    Channel,
    Function,
    #[allow(dead_code)]
    Task,
    Unknown,
}

impl Value {
    /// Reports whether this value recursively contains a non-finite float.
    ///
    /// Private bytecode and native callbacks can construct `Value` directly,
    /// so execution boundaries use this to preserve Slug's finite-number rule.
    pub(crate) fn contains_non_finite_number(&self) -> bool {
        match self {
            Self::Float(value) => !value.is_finite(),
            Self::List(values) => values.iter().any(Self::contains_non_finite_number),
            Self::Map(entries) => entries.iter().any(|(key, value)| {
                key.contains_non_finite_number() || value.contains_non_finite_number()
            }),
            Self::Struct(value) => value.values.iter().any(Self::contains_non_finite_number),
            _ => false,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Nil => ValueKind::Nil,
            Self::Bool(_) => ValueKind::Bool,
            Self::Int(_) | Self::Float(_) => ValueKind::Num,
            Self::Str(_) => ValueKind::Str,
            Self::Bytes(_) => ValueKind::Bytes,
            Self::List(_) => ValueKind::List,
            Self::Map(_) => ValueKind::Map,
            Self::StructSchema(_) => ValueKind::StructSchema,
            Self::Struct(_) => ValueKind::Struct,
            Self::Enum(_) => ValueKind::Enum,
            Self::Channel(_) => ValueKind::Channel,
            Self::Closure(_)
            | Self::Native(_)
            | Self::DeclaredNative { .. }
            | Self::Builtin(_)
            | Self::Overloads(_) => ValueKind::Function,
            #[cfg(feature = "concurrency")]
            Self::Task(_) => ValueKind::Task,
            Self::NativeResource(_) | Self::Uninitialized | Self::Binding { .. } => {
                ValueKind::Unknown
            }
        }
    }
    #[must_use]
    pub fn string(value: impl Into<Str>) -> Self {
        Self::Str(value.into())
    }

    /// Creates an immutable list value from its logical elements.
    #[must_use]
    pub fn list(values: Vec<Value>) -> Self {
        Self::List(List::from_values(values))
    }

    /// Creates an immutable map value. A repeated key keeps its last value.
    #[must_use]
    pub fn map(entries: Vec<(Value, Value)>) -> Self {
        Self::Map(Map::new(entries))
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    #[must_use]
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Uninitialized | Self::Binding { .. } => "binding",
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::Float(_) => "num",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::StructSchema(_) => "struct schema",
            Self::Struct(_) => "struct",
            Self::Enum(_) => "enum",
            Self::Channel(_) => "chan",
            Self::Closure(_)
            | Self::Native(_)
            | Self::DeclaredNative { .. }
            | Self::Builtin(_)
            | Self::Overloads(_) => "fn",
            #[cfg(feature = "concurrency")]
            Self::Task(_) => "task",
            Self::NativeResource(_) => "native resource",
        }
    }

    pub(crate) fn resolve(&self) -> Result<Value, String> {
        let Self::Binding { name, cell, .. } = self else {
            return Ok(self.clone());
        };
        let value = cell.borrow().clone();
        if matches!(value, Self::Uninitialized) {
            Err(format!("binding `{name}` is not initialized"))
        } else {
            Ok(value)
        }
    }

    pub(crate) fn replace_binding(&self, value: Value) -> bool {
        let Self::Binding { cell, .. } = self else {
            return false;
        };
        *cell.borrow_mut() = value;
        true
    }

    pub(crate) fn is_uninitialized_binding(&self) -> bool {
        matches!(self, Self::Binding { cell, .. } if matches!(*cell.borrow(), Self::Uninitialized))
    }
}

/// How deep one comparison recurses, through containers and module
/// bindings, before it goes on with a stack of its own: a value nested
/// deeper than the native stack can hold still compares, and one that
/// reaches itself through a module variable still ends.
const NESTING_LIMIT: u32 = 64;

/// How many steps through containers and bindings one comparison takes
/// before it goes on with pair tracking, so that values sharing their parts
/// many times over compare in time bounded by their size.
const STEP_BUDGET: u32 = 4096;

/// How an equality reads a module binding it meets inside a value: as the
/// task comparing sees it, which in a fork is its snapshot.
pub(crate) type BindingReader<'a> =
    &'a dyn Fn(&BindingCell, &crate::variables::BindingName) -> Value;

/// The reader `==` on values uses outside a VM: the running task's view,
/// recorded per thread on hosted builds, or the binding itself.
pub(crate) fn running_reader(cell: &BindingCell, name: &crate::variables::BindingName) -> Value {
    #[cfg(feature = "concurrency")]
    return crate::vm::seen_binding(cell, name);
    #[cfg(not(feature = "concurrency"))]
    {
        let _ = name;
        cell.borrow().clone()
    }
}

/// A module binding's value for a comparison: a `val` reads the same to
/// every task; a variable is read through `read`.
fn compared_binding(
    cell: &BindingCell,
    name: &crate::variables::BindingName,
    read: BindingReader<'_>,
) -> Value {
    if name.kind() == crate::variables::BindingKind::Val {
        cell.borrow().clone()
    } else {
        read(cell, name)
    }
}

/// What a comparison of two containers is known by, so that the deep
/// phase compares each pair once.
type ComparedPair = (u8, (usize, u32, u32), (usize, u32, u32));

impl Value {
    /// Whether dropping this value may drop others nested in it.
    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    pub(crate) fn holds_values(&self) -> bool {
        matches!(
            self,
            Self::List(_) | Self::Map(_) | Self::Struct(_) | Self::Closure(_)
        )
    }

    /// Whether `self` equals `other`, reading each module binding inside
    /// either through `read`. The comparison first recurses, which is fast
    /// for ordinary values; one that nests deeper than [`NESTING_LIMIT`] or
    /// takes more than [`STEP_BUDGET`] steps through containers and
    /// bindings, as two values sharing their parts many times over can,
    /// starts again in the deep phase, whose work is bounded by the values'
    /// size, however their parts are shared.
    pub(crate) fn equals_with(&self, other: &Self, read: BindingReader<'_>) -> bool {
        let mut steps = 0;
        self.equals_shallow(other, read, 0, &mut steps)
            .unwrap_or_else(|| self.equals_deep(other, read))
    }

    /// The shallow phase: the answer, or `None` past either limit.
    fn equals_shallow(
        &self,
        other: &Self,
        read: BindingReader<'_>,
        nesting: u32,
        steps: &mut u32,
    ) -> Option<bool> {
        if nesting >= NESTING_LIMIT {
            return None;
        }
        let nesting = nesting + 1;
        let mut step = || {
            *steps += 1;
            (*steps <= STEP_BUDGET).then_some(())
        };
        Some(match (self, other) {
            (Self::Binding { cell, .. }, Self::Binding { cell: other, .. })
                if crate::sync::Code::ptr_eq(cell, other) =>
            {
                true
            }
            (Self::Binding { cell, name }, value) | (value, Self::Binding { cell, name }) => {
                step()?;
                return compared_binding(cell, name, read)
                    .equals_shallow(value, read, nesting, steps);
            }
            (Self::List(a), Self::List(b)) => {
                if a.identity() == b.identity() {
                    return Some(true);
                }
                step()?;
                if a.len() != b.len() {
                    return Some(false);
                }
                for (a, b) in a.iter().zip(b.iter()) {
                    if !a.equals_shallow(b, read, nesting, steps)? {
                        return Some(false);
                    }
                }
                true
            }
            (Self::Overloads(a), Self::Overloads(b)) => {
                step()?;
                if a.len() != b.len() {
                    return Some(false);
                }
                for (a, b) in a.iter().zip(b.iter()) {
                    if !a.equals_shallow(b, read, nesting, steps)? {
                        return Some(false);
                    }
                }
                true
            }
            (Self::Map(a), Self::Map(b)) => {
                if a.identity() == b.identity() {
                    return Some(true);
                }
                step()?;
                if a.len() != b.len() {
                    return Some(false);
                }
                for (key, value) in a.iter() {
                    let Some(other) = b.get(key) else {
                        return Some(false);
                    };
                    if !value.equals_shallow(other, read, nesting, steps)? {
                        return Some(false);
                    }
                }
                true
            }
            (Self::Struct(a), Self::Struct(b)) => {
                if !Rc::ptr_eq(&a.schema, &b.schema) || a.values.len() != b.values.len() {
                    return Some(false);
                }
                step()?;
                for (a, b) in a.values.iter().zip(&b.values) {
                    if !a.equals_shallow(b, read, nesting, steps)? {
                        return Some(false);
                    }
                }
                true
            }
            _ => self.equals_leaf(other),
        })
    }

    /// The deep phase of a comparison: the same rules, with a stack of its
    /// own, comparing each pair of containers once. A pair met again is
    /// taken as equal, which is what it is unless something else in the
    /// comparison differs.
    fn equals_deep(&self, other: &Self, read: BindingReader<'_>) -> bool {
        let mut pending = alloc::vec![(self.clone(), other.clone())];
        let mut compared: crate::platform::HashSet<ComparedPair> = crate::platform::HashSet::new();
        while let Some((left, right)) = pending.pop() {
            match (&left, &right) {
                (Self::Binding { cell, .. }, Self::Binding { cell: other, .. })
                    if crate::sync::Code::ptr_eq(cell, other) => {}
                (Self::Binding { cell, name }, value) | (value, Self::Binding { cell, name }) => {
                    pending.push((compared_binding(cell, name, read), value.clone()));
                }
                (Self::List(a), Self::List(b)) => {
                    if a.identity() == b.identity()
                        || !compared.insert((0, a.identity(), b.identity()))
                    {
                        continue;
                    }
                    if a.len() != b.len() {
                        return false;
                    }
                    pending.extend(a.iter().cloned().zip(b.iter().cloned()));
                }
                (Self::Overloads(a), Self::Overloads(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    pending.extend(a.iter().cloned().zip(b.iter().cloned()));
                }
                (Self::Map(a), Self::Map(b)) => {
                    let pair = (1, (a.identity(), 0, 0), (b.identity(), 0, 0));
                    if a.identity() == b.identity() || !compared.insert(pair) {
                        continue;
                    }
                    if a.len() != b.len() {
                        return false;
                    }
                    for (key, value) in a.iter() {
                        let Some(other) = b.get(key) else {
                            return false;
                        };
                        pending.push((value.clone(), other.clone()));
                    }
                }
                (Self::Struct(a), Self::Struct(b)) => {
                    if !Rc::ptr_eq(&a.schema, &b.schema) || a.values.len() != b.values.len() {
                        return false;
                    }
                    let pair = (
                        2,
                        (Rc::as_ptr(a) as usize, 0, 0),
                        (Rc::as_ptr(b) as usize, 0, 0),
                    );
                    if Rc::ptr_eq(a, b) || !compared.insert(pair) {
                        continue;
                    }
                    pending.extend(a.values.iter().cloned().zip(b.values.iter().cloned()));
                }
                _ => {
                    if !left.equals_leaf(&right) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Equality of values that hold no others: containers and bindings are
    /// compared by the phases above, and are never equal here.
    #[allow(clippy::float_cmp, reason = "Slug's equality is exact")]
    fn equals_leaf(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Uninitialized, Self::Uninitialized) | (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) | (Self::Float(b), Self::Int(a)) => {
                int_equals_float(*a, *b)
            }
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::StructSchema(a), Self::StructSchema(b)) => Rc::ptr_eq(a, b),
            (Self::Enum(a), Self::Enum(b)) => a == b,
            (Self::Channel(a), Self::Channel(b)) => Rc::ptr_eq(a, b),
            (Self::Closure(a), Self::Closure(b)) => crate::sync::Code::ptr_eq(a, b),
            #[cfg(feature = "concurrency")]
            (Self::Task(a), Self::Task(b)) => Rc::ptr_eq(a, b),
            (Self::Native(a), Self::Native(b)) => a.same_function(b),
            (
                Self::DeclaredNative { function: left, .. },
                Self::DeclaredNative {
                    function: right, ..
                },
            ) => left.same_function(right),
            (Self::NativeResource(a), Self::NativeResource(b)) => Rc::ptr_eq(a, b),
            (Self::Builtin(a), Self::Builtin(b)) => a == b,
            _ => false,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.equals_with(other, &running_reader)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => write!(f, "<uninitialized>"),
            Self::Nil => write!(f, "nil"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bytes(value) => write!(f, "0x\"{}\"", hex(BytesView::new(value).as_slice())),
            Self::List(values) => f.debug_list().entries(values.iter()).finish(),
            Self::Map(entries) => f
                .debug_map()
                .entries(entries.view().iter().map(|(key, value)| (key, value)))
                .finish(),
            Self::StructSchema(_) => write!(f, "<struct schema>"),
            Self::Struct(value) => {
                write!(f, "struct ")?;
                f.debug_map()
                    .entries(
                        value
                            .schema
                            .fields
                            .iter()
                            .zip(&value.values)
                            .map(|(field, value)| (&field.name, value)),
                    )
                    .finish()
            }
            Self::Enum(value) => write!(f, "{}.{}", value.name, value.case),
            Self::Channel(_) => write!(f, "<chan>"),
            Self::Closure(_) => write!(f, "<fn>"),
            #[cfg(feature = "concurrency")]
            Self::Task(_) => write!(f, "<task>"),
            Self::Native(function) => write!(f, "<native {}>", function.qualified_name()),
            Self::DeclaredNative { function, .. } => {
                write!(f, "<native {}>", function.qualified_name())
            }
            Self::NativeResource(_) => write!(f, "<native resource>"),
            Self::Builtin(builtin) => write!(f, "<builtin {builtin:?}>"),
            Self::Overloads(_) => write!(f, "<overloads>"),
            Self::Binding { name, .. } => write!(f, "<binding {name}>"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(value) => write!(f, "{value}"),
            value => write!(f, "{value:?}"),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing into String cannot fail");
    }
    output
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    reason = "the range and integral-value checks make the conversion exact"
)]
fn int_equals_float(integer: i64, float: f64) -> bool {
    float.is_finite()
        && float >= i64::MIN as f64
        && float < -(i64::MIN as f64)
        && (float as i64) as f64 == float
        && float as i64 == integer
}
