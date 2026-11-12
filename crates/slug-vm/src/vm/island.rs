//! Forks on worker threads: each runs as an island, a VM with a scheduler of
//! its own, linked to the thread that made it only by its result, channel
//! wakes, cancellation, and forwarded host calls.
//!
//! See "Phase 4" in `docs/planning/fork-tasks.md`.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};

use super::{
    RootWaiter, TaskExecution, Vm, VmResult, Waiter, progress::ProgressDriver, scheduler::Nursery,
};
use crate::platform::HashMap;
use crate::{
    EmptyVmConfiguration, NativeDescriptorError, NativeFunction, NativeResourceRegistry, Value,
    VmConfiguration, VmHost, VmHostError, VmModuleDescription, VmModuleExports,
    scheduler_signal::ProgressSignal,
    sync::{Rc, Sendable},
    value::Task,
};

type Job = Sendable<Box<dyn FnOnce()>>;

/// Work carried to one thread from others: wakes of the executions it owns,
/// settled forks, and host requests. Every thread has one, and every
/// progress driver on a thread waits on its signal, so a post wakes
/// whichever is waiting.
pub(crate) struct Inbox {
    queue: Mutex<VecDeque<Job>>,
    /// Host calls from forks, which a busy thread serves at its next call,
    /// return, or `recur` rather than only when it waits (see `attention`).
    requests: Mutex<VecDeque<Job>>,
    /// What every VM on this thread checks at those points, in one load:
    /// [`REQUESTED`] while host calls are queued, and [`CANCELLED`] once this
    /// island's fork is cancelled.
    attention: AtomicU8,
    signal: Arc<ProgressSignal>,
    /// Forks started from this thread that have not reported back.
    remote: AtomicUsize,
    /// The group of the driver asleep on this thread until another wakes
    /// it, which a post counts as awake again.
    sleeper: Mutex<Option<Arc<Group>>>,
}

/// The threads running one program: its root and the islands of its forks.
/// Each counts as active unless it sleeps until another wakes it, so when
/// none is active, none ever will be: a deadlock, which the root reports.
pub(crate) struct Group {
    active: AtomicUsize,
    /// The program's islands that have not finished: what its run, or its
    /// VM's shutdown, waits for, rather than every fork of its thread,
    /// which may belong to another VM there.
    islands: AtomicUsize,
    deadlocked: AtomicBool,
    root: Arc<Inbox>,
}

impl Group {
    pub(crate) fn new(root: Arc<Inbox>) -> Self {
        Self {
            active: AtomicUsize::new(1),
            islands: AtomicUsize::new(0),
            deadlocked: AtomicBool::new(false),
            root,
        }
    }

    /// A thread of the program counts as active, as when it wakes.
    fn join(&self) {
        self.active.fetch_add(1, Ordering::AcqRel);
    }

    fn leave(&self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }

    /// A fork starts on an island of this program, which is active.
    fn island_started(&self) {
        self.islands.fetch_add(1, Ordering::AcqRel);
        self.join();
    }

    /// An island of this program has finished, its last work posted.
    fn island_finished(&self) {
        self.leave();
        self.islands.fetch_sub(1, Ordering::AcqRel);
    }

    /// Runs what this thread is posted, including the destruction of
    /// resources the program's forks let go of last, until every island of
    /// the program has finished. An island waits for its own forks before
    /// it finishes, so none is missed.
    pub(crate) fn join_islands(&self, inbox: &Inbox) {
        let waiter = inbox.signal.blocking_waiter();
        loop {
            let observed = inbox.signal.snapshot();
            inbox.drain();
            if self.islands.load(Ordering::Acquire) == 0 {
                break;
            }
            waiter.wait(&inbox.signal, observed, None);
        }
        // An island posts its last work before it stops counting.
        inbox.drain();
    }

    pub(crate) fn deadlocked(&self) -> bool {
        self.deadlocked.load(Ordering::Acquire)
    }

    pub(crate) fn report_deadlock(&self) {
        self.deadlocked.store(true, Ordering::Release);
        self.root.signal.notify();
    }
}

/// [`Inbox::attention`]: host calls are queued.
const REQUESTED: u8 = 1;
/// [`Inbox::attention`]: the fork on this island is cancelled.
const CANCELLED: u8 = 2;

std::thread_local! {
    static CURRENT: Arc<Inbox> = Arc::new(Inbox::new());
}

impl Inbox {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            requests: Mutex::new(VecDeque::new()),
            attention: AtomicU8::new(0),
            signal: Arc::new(ProgressSignal::new()),
            remote: AtomicUsize::new(0),
            sleeper: Mutex::new(None),
        }
    }

    /// Marks this thread's driver asleep in `group` until woken: `None` when
    /// work was posted to it meanwhile, so it must not sleep, or else
    /// whether it was the last active thread of the group, a deadlock.
    ///
    /// A post that arrives before the mark is in the queue, and one after it
    /// counts the thread awake again, so the check below misses neither.
    pub(crate) fn begin_sleep(&self, group: &Arc<Group>) -> Option<bool> {
        let mut sleeper = self.sleeper.lock().unwrap_or_else(PoisonError::into_inner);
        *sleeper = Some(group.clone());
        let last = group.active.fetch_sub(1, Ordering::AcqRel) == 1;
        let pending = !self
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
            || !self
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty();
        if pending {
            *sleeper = None;
            group.join();
            return None;
        }
        Some(last)
    }

    /// Counts this thread's driver as awake again, unless a post already did.
    pub(crate) fn end_sleep(&self) {
        if let Some(group) = self
            .sleeper
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            group.join();
        }
    }

    /// This thread's inbox.
    pub(crate) fn current() -> Arc<Self> {
        CURRENT
            .try_with(Arc::clone)
            .unwrap_or_else(|_| Arc::new(Self::new()))
    }

    pub(crate) fn is_current(&self) -> bool {
        CURRENT
            .try_with(|current| core::ptr::eq(Arc::as_ptr(current), self))
            .unwrap_or(false)
    }

    pub(crate) fn signal(&self) -> &Arc<ProgressSignal> {
        &self.signal
    }

    /// Carries `work` to this inbox's thread, which runs it when it next
    /// makes progress. It may hold values and scheduler state of that thread
    /// (see `Sendable`).
    pub(crate) fn post(&self, work: impl FnOnce() + 'static) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Sendable::new(Box::new(work)));
        // Awake from now on: the work is pending until the thread runs it.
        self.end_sleep();
        self.signal.notify();
    }

    /// Carries a host call from a fork to this inbox's thread, which runs it
    /// at its next safe point, busy or waiting.
    fn request(&self, work: impl FnOnce() + 'static) {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(Sendable::new(Box::new(work)));
        self.attention.fetch_or(REQUESTED, Ordering::Release);
        self.end_sleep();
        self.signal.notify();
    }

    /// Whether a VM on this thread must stop at its next safe point: to
    /// serve host calls, or because its fork is cancelled. One relaxed load;
    /// `attend` synchronizes.
    #[inline]
    pub(crate) fn needs_attention(&self) -> bool {
        self.attention.load(Ordering::Relaxed) != 0
    }

    /// Runs the host calls queued so far; whether there were any.
    pub(crate) fn serve_requests(&self) -> bool {
        // Cleared before the queue is read: a request queued after this is
        // either served below or raises the flag again.
        self.attention.fetch_and(!REQUESTED, Ordering::AcqRel);
        let mut ran = false;
        loop {
            let work = self
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            let Some(work) = work else {
                return ran;
            };
            ran = true;
            (work.into_inner())();
        }
    }

    /// Runs the work posted so far; whether there was any.
    pub(crate) fn drain(&self) -> bool {
        let mut ran = self.serve_requests();
        loop {
            let work = self
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            let Some(work) = work else {
                return ran;
            };
            ran = true;
            (work.into_inner())();
        }
    }

    /// Whether a fork started from this thread may still post to it.
    pub(crate) fn has_remote(&self) -> bool {
        self.remote.load(Ordering::Acquire) > 0
    }

    /// Runs what this thread's forks post, including the destruction of
    /// resources they let go of last, until every one has reported back.
    pub(crate) fn join_remote(&self) {
        let waiter = self.signal.blocking_waiter();
        loop {
            let observed = self.signal.snapshot();
            self.drain();
            if !self.has_remote() {
                break;
            }
            waiter.wait(&self.signal, observed, None);
        }
        // A fork posts its last work before it stops counting.
        self.drain();
    }
}

impl Drop for Inbox {
    /// Work left for a thread that is gone is never run, and not dropped
    /// here, perhaps on another thread: it may hold that thread's values,
    /// such as a resource's payload awaiting destruction.
    fn drop(&mut self) {
        let queue = core::mem::take(self.queue.get_mut().unwrap_or_else(PoisonError::into_inner));
        let requests = core::mem::take(
            self.requests
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner),
        );
        core::mem::forget(queue);
        core::mem::forget(requests);
    }
}

/// What a fork's spawner and its island share: whether it was cancelled,
/// and the island's inbox, to wake it when it is.
pub(crate) struct IslandControl {
    cancelled: AtomicBool,
    inbox: OnceLock<Arc<Inbox>>,
}

impl IslandControl {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            inbox: OnceLock::new(),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        // Pairs with the fence in `run`: either this sees the island's inbox,
        // or the island sees the flag once it has set it.
        core::sync::atomic::fence(Ordering::SeqCst);
        if let Some(inbox) = self.inbox.get() {
            inbox.attention.fetch_or(CANCELLED, Ordering::Release);
            inbox.signal.notify();
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// How many forks may run at once in this process, each on a thread of its
/// own (see [`set_fork_limit`]).
const DEFAULT_FORK_LIMIT: usize = 256;

static FORK_LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_FORK_LIMIT);

/// Islands running now, each holding a worker thread.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Sets how many forks may run at once in this process, 256 unless set; at
/// least one. A fork started beyond it fails with a fork error, rather than
/// waiting, since the forks running might be waiting for it. Each running
/// fork holds a thread, which is what the limit bounds: forks that wait
/// sleep and cost no processor time, so the limit is well above the number
/// of cores, and a program pacing work to the cores does so with
/// `nursery limit`.
pub fn set_fork_limit(limit: usize) {
    FORK_LIMIT.store(limit.max(1), Ordering::Release);
}

/// How many forks may run at once in this process.
#[must_use]
pub fn fork_limit() -> usize {
    FORK_LIMIT.load(Ordering::Acquire)
}

/// Takes one of the running forks' places, if one is free.
fn reserve_place() -> bool {
    let limit = fork_limit();
    RUNNING
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |running| {
            (running < limit).then_some(running + 1)
        })
        .is_ok()
}

fn release_place() {
    RUNNING.fetch_sub(1, Ordering::AcqRel);
}

/// Ships `task`, a fork its nursery has admitted, to an island on a worker
/// thread. Returns whether it took the task: shipped, or failed because no
/// fork may start now; `false` leaves a fork not yet admitted queued.
pub(super) fn ship(task: &Rc<Task>) -> bool {
    let control = Arc::new(IslandControl::new());
    let Some(execution) = task.take_for_island(&control) else {
        return false;
    };
    // Built here, where the fork's start is still known, for either failure.
    let mut failure = execution
        .vm
        .error_at(crate::RuntimeErrorKind::Fork, String::new());
    if !reserve_place() {
        drop(execution);
        failure.message = alloc::format!(
            "this fork cannot start: {} forks are running already, the most this program may run at once; start fewer at a time, such as with `nursery limit`",
            fork_limit()
        );
        task.cancel(&failure);
        return true;
    }
    let parent = Inbox::current();
    let (inputs, link) = ForkInputs::take(execution, task.clone(), parent.clone(), control);
    link.group.island_started();
    parent.remote.fetch_add(1, Ordering::AcqRel);
    #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
    crate::sync::solo_check::fork_started();
    let group = link.group.clone();
    let package = Sendable::new((inputs, link));
    let started = submit(Sendable::new(Box::new(move || {
        let (inputs, link) = package.into_inner();
        run(inputs, link);
    })));
    if let Err(error) = started {
        // The package, and the task handle in it, dropped here with the job.
        group.island_finished();
        parent.remote.fetch_sub(1, Ordering::AcqRel);
        #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
        crate::sync::solo_check::fork_ended();
        release_place();
        failure.message =
            alloc::format!("this fork cannot start: no thread could be started for it: {error}");
        task.cancel(&failure);
    }
    true
}

/// Everything a fork's island is built from, and all that crosses to its
/// worker for the island to own: the code it runs and what it may see.
/// Nothing of the spawner's scheduler, resources, or counters is in it.
struct ForkInputs {
    closure: crate::sync::Code<crate::value::Closure>,
    /// Where the fork was started, for its errors' call stacks.
    span: Option<crate::SourceSpan>,
    view: Option<Rc<super::fork::TaskView>>,
    domain: u32,
    /// The island's own registry of module variables.
    variables: crate::ModuleVariables,
    /// The host's arguments, read on the spawner's thread, which owns the
    /// host; `None` without a host.
    configuration: Option<(Vec<String>, Value)>,
    /// Whether the spawner samples call stacks, so the island does too.
    #[cfg(feature = "metrics")]
    profiling: bool,
}

/// What stays the spawner's while its fork runs: the island touches these
/// only to post work back to the spawner's thread, which alone uses them.
struct ParentLink {
    /// The spawner's host, which the island's forwards host calls to.
    host: Option<Rc<dyn VmHost>>,
    task: Rc<Task>,
    parent: Arc<Inbox>,
    control: Arc<IslandControl>,
    group: Arc<Group>,
    #[cfg(feature = "metrics")]
    counts: Counts,
}

impl ForkInputs {
    /// Takes a fork's inputs from the execution its spawner prepared,
    /// dropping the rest of that execution here, on the spawner's thread.
    fn take(
        execution: TaskExecution,
        task: Rc<Task>,
        parent: Arc<Inbox>,
        control: Arc<IslandControl>,
    ) -> (Self, ParentLink) {
        let vm = &execution.vm;
        let frame = vm
            .frames
            .first()
            .expect("a fork's execution starts with its closure's frame");
        let span = match &frame.call_span {
            Some(super::frames::CallSpan::Owned(span)) => Some((**span).clone()),
            _ => None,
        };
        let configuration = vm.host.as_ref().map(|host| {
            let configuration = host.configuration();
            (
                configuration.arguments().to_vec(),
                configuration.argument_map(),
            )
        });
        let inputs = Self {
            closure: frame.closure.clone(),
            span,
            view: vm.view.clone(),
            domain: vm.domain,
            variables: vm.variables.island_copy(),
            configuration,
            #[cfg(feature = "metrics")]
            profiling: vm.profile.borrow().is_some(),
        };
        let link = ParentLink {
            host: vm.host.clone(),
            task,
            parent,
            control,
            group: vm.progress.group().clone(),
            #[cfg(feature = "metrics")]
            counts: Counts {
                metrics: vm.metrics.clone(),
                profile: vm.profile.clone(),
            },
        };
        // The rest is the spawner's, released on its thread.
        drop(execution);
        (inputs, link)
    }

    /// The island's VM, built on its worker from these inputs alone and
    /// the worker's own scheduler, cancellation, and attention.
    fn island_vm(self, link: &ParentLink, inbox: &Arc<Inbox>) -> Vm {
        let Self {
            closure,
            span,
            view,
            domain,
            variables,
            configuration,
            #[cfg(feature = "metrics")]
            profiling,
        } = self;
        let progress = Rc::new(ProgressDriver::island(
            link.control.clone(),
            link.group.clone(),
        ));
        #[cfg(feature = "metrics")]
        let metrics = Rc::new(core::cell::RefCell::new(super::VmMetrics::default()));
        let nursery = Rc::new(Nursery::root(
            progress.clone(),
            #[cfg(feature = "metrics")]
            metrics.clone(),
        ));
        let host = link.host.clone().map(|host| {
            let (arguments, argument_map) =
                configuration.unwrap_or_else(|| (Vec::new(), EmptyVmConfiguration.argument_map()));
            Rc::new(ForkHost::new(
                host,
                link.parent.clone(),
                link.control.clone(),
                arguments,
                argument_map,
                variables.clone(),
            )) as Rc<dyn VmHost>
        });
        Vm::closure_vm(
            super::Inherited {
                host,
                progress,
                view,
                domain,
                cancel: Some(link.control.clone()),
                attention: inbox.clone(),
                variables,
                native_resources: NativeResourceRegistry::new(),
                #[cfg(feature = "metrics")]
                metrics,
                #[cfg(feature = "metrics")]
                profile: Rc::new(core::cell::RefCell::new(
                    profiling.then(alloc::collections::BTreeMap::new),
                )),
            },
            closure,
            Vec::new(),
            None,
            span,
            super::calls::ClosureCallOptions {
                direct_task_limit: None,
                direct_task_count: None,
                nursery,
            },
        )
    }
}

/// Runs a fork's island on this worker thread, then reports its result.
fn run(inputs: ForkInputs, link: ParentLink) {
    let inbox = Inbox::current();
    // The thread's previous island may have been cancelled.
    inbox.attention.fetch_and(!CANCELLED, Ordering::AcqRel);
    let _ = link.control.inbox.set(inbox.clone());
    core::sync::atomic::fence(Ordering::SeqCst);
    if link.control.cancelled() {
        inbox.attention.fetch_or(CANCELLED, Ordering::Release);
    }
    let mut vm = inputs.island_vm(&link, &inbox);
    let ParentLink {
        host,
        task,
        parent,
        control,
        group,
        #[cfg(feature = "metrics")]
        counts,
    } = link;
    // The island's host holds its own handle on the spawner's.
    drop(host);
    // A panic here is a VM bug. Unwinding past this point would leave the
    // spawner waiting for a result that never comes, so it becomes the
    // fork's error instead, and the island's bookkeeping still runs. Release
    // builds abort on a panic, which cannot leave the spawner waiting either.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.run_island(&control)
            .map(|result| vm.sendable_outcome(result))
    }))
    .unwrap_or_else(|panic| Some(Err(vm.island_panic_error(panic.as_ref()))));
    // Posted before the result, so the spawner has merged them by the time
    // it sees the fork settle.
    #[cfg(feature = "metrics")]
    counts.merge_from(&vm, &parent);
    drop(vm);
    #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
    crate::sync::solo_check::fork_ended();
    // Free before the spawner can see the fork settle, which may lead it to
    // start the next fork at once.
    release_place();
    if let Some(result) = outcome
        && !control.cancelled()
    {
        parent.post(move || task.complete_remote(&result));
    } else {
        // The task is the spawner's; it is dropped there.
        parent.post(move || drop(task));
    }
    parent.remote.fetch_sub(1, Ordering::AcqRel);
    // After the post, which counts the parent awake if it slept.
    group.island_finished();
    parent.signal.notify();
}

/// A spawner's counters and samples, which only its thread touches, kept
/// while its fork's island counts and samples on its own.
#[cfg(feature = "metrics")]
struct Counts {
    metrics: Rc<core::cell::RefCell<super::VmMetrics>>,
    profile: super::profile::Samples,
}

#[cfg(feature = "metrics")]
impl Counts {
    /// Posts the island's counters and samples to the spawner's thread,
    /// which adds them into its own.
    fn merge_from(self, vm: &Vm, parent: &Inbox) {
        let counts = vm.metrics.borrow().clone();
        let samples = vm.profile.borrow_mut().take();
        parent.post(move || {
            self.metrics.borrow_mut().merge(&counts);
            if let (Some(into), Some(samples)) = (self.profile.borrow_mut().as_mut(), samples) {
                for (stack, count) in samples {
                    *into.entry(stack).or_default() += count;
                }
            }
        });
    }
}

impl Vm {
    /// Stops at a safe point when this thread needs it: serves the host
    /// calls forks have queued, and fails once this island's fork is
    /// cancelled. Otherwise it is one load and a branch.
    #[inline]
    pub(super) fn check_attention(&self) -> VmResult<()> {
        if self.attention.needs_attention() {
            return self.attend();
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn attend(&self) -> VmResult<()> {
        self.attention.serve_requests();
        match &self.cancel {
            Some(control) if control.cancelled() => Err(self.cancelled_error()),
            _ => Ok(()),
        }
    }

    #[cold]
    #[inline(never)]
    fn island_panic_error(&self, panic: &(dyn core::any::Any + Send)) -> super::RuntimeError {
        let message = panic
            .downcast_ref::<&str>()
            .map(|message| String::from(*message))
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        self.error_at(
            crate::RuntimeErrorKind::Fork,
            alloc::format!("this fork stopped on an internal VM error: {message}"),
        )
    }

    #[cold]
    #[inline(never)]
    fn cancelled_error(&self) -> super::RuntimeError {
        self.error_at(
            crate::RuntimeErrorKind::Fork,
            "this fork was cancelled".into(),
        )
    }

    /// Runs this island's fork to settlement, as the root runs `main`, or
    /// `None` once its spawner cancels it.
    fn run_island(&mut self, control: &IslandControl) -> Option<VmResult<Value>> {
        let root = RootWaiter::new();
        self.current_waiter = Some(Waiter::root(root.clone()));
        let outcome = self.run_island_root(control, &root);
        if control.cancelled() {
            self.release_cancelled(&root);
            return None;
        }
        outcome
    }

    /// After its fork is cancelled, takes this island's tasks out of every
    /// queue they wait in, cancels its nested forks, and waits until they
    /// have reported back, so that nothing they post reaches the next
    /// island on this thread. Otherwise a send could hand its value to a
    /// receiver here that will never run.
    fn release_cancelled(&mut self, root: &RootWaiter) {
        if let Some(wait_registration) = self.wait_registration.take() {
            wait_registration.remove_for_waiter(&Waiter::root(root.clone()));
        }
        // Cancelling a task leaves the queues it waits in, cancels a nursery
        // it waits for, and cancels a fork running elsewhere.
        let error = self.cancelled_error();
        self.nursery.cancel_all(&error);
        // Nested forks' results and host calls, and wakes of tasks already
        // cancelled, which take nothing.
        self.attention.join_remote();
        self.nursery.clear();
    }

    fn run_island_root(
        &mut self,
        control: &IslandControl,
        root: &RootWaiter,
    ) -> Option<VmResult<Value>> {
        loop {
            if control.cancelled() {
                return None;
            }
            match self.execute() {
                // The fork's children settle as the root's do, but a
                // cancellation, which wakes this thread, ends the wait.
                super::ExecutionOutcome::Settled(result) => loop {
                    if control.cancelled() {
                        return None;
                    }
                    if let Some(settled) = self.settle_tasks_available(&result) {
                        return Some(settled);
                    }
                    self.make_progress();
                },
                super::ExecutionOutcome::Suspended => loop {
                    if control.cancelled() {
                        return None;
                    }
                    if let Some(result) = root.take_resume() {
                        if let Some(wait_registration) = self.wait_registration.take() {
                            wait_registration.remove_for_waiter(&Waiter::root(root.clone()));
                        }
                        self.resume = Some(result);
                        break;
                    }
                    // An island always may be woken from another thread, so
                    // it waits rather than reporting itself blocked.
                    self.make_progress();
                },
            }
        }
    }
}

// The worker pool: an idle worker takes the next island, and a new one
// starts when none is idle, so islands waiting on one another never wait
// for a worker.
type Worker = std::sync::mpsc::Sender<Job>;

static IDLE: Mutex<Vec<Worker>> = Mutex::new(Vec::new());

/// Runs `job` on an idle worker, or a new one; an error when no thread could
/// be started for it, the job dropped.
fn submit(mut job: Job) -> Result<(), std::io::Error> {
    loop {
        let idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner).pop();
        let Some(worker) = idle else {
            break;
        };
        match worker.send(job) {
            Ok(()) => return Ok(()),
            Err(std::sync::mpsc::SendError(returned)) => job = returned,
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("slug-fork".into())
        .spawn(move || {
            let mut job = job;
            loop {
                (job.into_inner())();
                IDLE.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(sender.clone());
                match receiver.recv() {
                    Ok(next) => job = next,
                    Err(_) => return,
                }
            }
        });
    started.map(drop)
}

/// The spawner's host, reached from an island: each call runs on the
/// spawner's thread, at the next call, return, or `recur` of any VM there,
/// or when it next waits.
struct Remote {
    host: Rc<dyn VmHost>,
    parent: Arc<Inbox>,
    /// The fork's: a host call it waits on ends when it is cancelled, since
    /// its spawner may never serve the call then.
    control: Arc<IslandControl>,
}

/// How often a fork waiting on a host call checks whether it was cancelled.
const HOST_CALL_RECHECK: std::time::Duration = std::time::Duration::from_millis(10);

impl Remote {
    /// The spawner's answer to `request`, or `None` once the fork is
    /// cancelled.
    fn call<R: 'static>(&self, request: impl FnOnce(&dyn VmHost) -> R + 'static) -> Option<R> {
        let reply = Arc::new((Mutex::new(None::<Sendable<R>>), Condvar::new()));
        let host = self.host.clone();
        let answer = reply.clone();
        self.parent.request(move || {
            let result = request(&*host);
            *answer.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Sendable::new(result));
            answer.1.notify_all();
        });
        let mut slot = reply.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(result) = slot.take() {
                return Some(result.into_inner());
            }
            // An answer that comes later is dropped on the spawner's thread,
            // with the request's last handle on the reply.
            if self.control.cancelled() {
                return None;
            }
            slot = reply
                .1
                .wait_timeout(slot, HOST_CALL_RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn tell(&self, request: impl FnOnce(&dyn VmHost) + 'static) {
        let host = self.host.clone();
        self.parent.request(move || request(&*host));
    }
}

/// What a host call answers once the fork that made it is cancelled.
fn cancelled_call() -> VmHostError {
    VmHostError::new("this fork was cancelled")
}

struct ForkHost {
    remote: Rc<Remote>,
    configuration: ForkConfiguration,
    variables: crate::ModuleVariables,
    resources: NativeResourceRegistry,
}

impl ForkHost {
    fn new(
        host: Rc<dyn VmHost>,
        parent: Arc<Inbox>,
        control: Arc<IslandControl>,
        arguments: Vec<String>,
        argument_map: Value,
        variables: crate::ModuleVariables,
    ) -> Self {
        let remote = Rc::new(Remote {
            host,
            parent,
            control,
        });
        Self {
            configuration: ForkConfiguration {
                remote: remote.clone(),
                arguments,
                argument_map,
            },
            remote,
            variables,
            resources: NativeResourceRegistry::new(),
        }
    }
}

impl VmHost for ForkHost {
    fn import_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleExports, VmHostError> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.import_module(importer.as_deref(), &name))
            .unwrap_or_else(|| Err(cancelled_call()))
    }

    fn describe_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleDescription, VmHostError> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.describe_module(importer.as_deref(), &name))
            .unwrap_or_else(|| Err(cancelled_call()))
    }

    fn permits(&self, permission: &str) -> bool {
        let permission = String::from(permission);
        self.remote
            .call(move |host| host.permits(&permission))
            .unwrap_or(false)
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.remote
            .call(|host| host.builtin_globals())
            .unwrap_or_default()
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        self.remote
            .call(|host| host.native_globals())
            .unwrap_or_default()
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        let (module, name) = (String::from(module), String::from(name));
        self.remote
            .call(move |host| host.foreign_function(&module, &name))
            .flatten()
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        self.remote
            .call(move |host| host.define_foreign_batch(functions))
            .unwrap_or_else(|| Err(NativeDescriptorError::new("this fork was cancelled")))
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.remote
            .tell(move |host| host.define_native_global(name, value));
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.resources.clone()
    }

    fn import_initialized(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Option<Result<VmModuleExports, VmHostError>> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.import_initialized(importer.as_deref(), &name))
            .unwrap_or_else(|| Some(Err(cancelled_call())))
    }

    fn describe_initialized(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Option<Result<VmModuleDescription, VmHostError>> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.describe_initialized(importer.as_deref(), &name))
            .unwrap_or_else(|| Some(Err(cancelled_call())))
    }

    fn module_variables(&self) -> crate::ModuleVariables {
        self.variables.clone()
    }

    fn configuration(&self) -> &dyn VmConfiguration {
        &self.configuration
    }

    fn warn(&self, message: String) {
        self.remote.tell(move |host| host.warn(message));
    }

    /// The spawner's host outlives its forks and shuts down with it.
    fn shutdown(&self) {}
}

struct ForkConfiguration {
    remote: Rc<Remote>,
    arguments: Vec<String>,
    argument_map: Value,
}

impl VmConfiguration for ForkConfiguration {
    fn resolve(&self, key: &str, fallback: &Value) -> Value {
        // The fork checked its fallback before it asked; the host's answer
        // is checked here, on the spawner's thread, before it crosses back.
        let (key, given) = (String::from(key), fallback.clone());
        self.remote
            .call(move |host| {
                let value = host.configuration().resolve(&key, &given);
                match super::fork::unsendable(&value, &crate::value::running_reader) {
                    None => value,
                    Some(reason) => {
                        host.warn(alloc::format!(
                            "configuration `{key}` holds {reason}, which a fork cannot receive; the fork gets its fallback"
                        ));
                        given
                    }
                }
            })
            .unwrap_or_else(|| fallback.clone())
    }

    fn arguments(&self) -> &[String] {
        &self.arguments
    }

    fn argument_map(&self) -> Value {
        self.argument_map.clone()
    }
}
