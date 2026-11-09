//! Forks on worker threads: each runs as an island, a VM with a scheduler of
//! its own, linked to the thread that made it only by its result, channel
//! wakes, cancellation, and forwarded host calls.
//!
//! See "Phase 4" in `docs/planning/fork-tasks.md`.

use alloc::{boxed::Box, collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};

use super::{
    RootWaiter, TaskExecution, Vm, VmResult, Waiter, progress::ProgressDriver,
    scheduler::Nursery,
};
use crate::platform::HashMap;
use crate::{
    EmptyVmConfiguration, NativeDescriptorError, NativeFunction, NativeResourceRegistry,
    Value, VmConfiguration, VmHost, VmHostError, VmModuleDescription, VmModuleExports,
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
    deadlocked: AtomicBool,
    root: Arc<Inbox>,
}

impl Group {
    pub(crate) fn new(root: Arc<Inbox>) -> Self {
        Self {
            active: AtomicUsize::new(1),
            deadlocked: AtomicBool::new(false),
            root,
        }
    }

    fn join(&self) {
        self.active.fetch_add(1, Ordering::AcqRel);
    }

    fn leave(&self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
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

/// Ships `task`, a fork its nursery has admitted, to an island on a worker
/// thread. Returns whether it did.
pub(super) fn ship(task: &Rc<Task>) -> bool {
    let control = Arc::new(IslandControl::new());
    let Some(execution) = task.take_for_island(&control) else {
        return false;
    };
    let parent = Inbox::current();
    // Taken here, on the thread that owns the host: the island reads them
    // from its own copy.
    let configuration = execution.vm.host.as_ref().map(|host| {
        let configuration = host.configuration();
        (configuration.arguments().to_vec(), configuration.argument_map())
    });
    let variables = execution.vm.variables.island_copy();
    let group = execution.vm.progress.group().clone();
    group.join();
    parent.remote.fetch_add(1, Ordering::AcqRel);
    #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
    crate::sync::solo_check::fork_started();
    let package = Sendable::new(Package {
        execution,
        task: task.clone(),
        configuration,
        variables,
        parent,
        control,
        group,
    });
    submit(Sendable::new(Box::new(move || run(package.into_inner()))));
    true
}

struct Package {
    execution: TaskExecution,
    task: Rc<Task>,
    configuration: Option<(Vec<String>, Value)>,
    variables: crate::ModuleVariables,
    parent: Arc<Inbox>,
    control: Arc<IslandControl>,
    group: Arc<Group>,
}

/// Runs a fork's island on this worker thread, then reports its result.
fn run(package: Package) {
    let Package {
        mut execution,
        task,
        configuration,
        variables,
        parent,
        control,
        group,
    } = package;
    let inbox = Inbox::current();
    // The thread's previous island may have been cancelled.
    inbox.attention.fetch_and(!CANCELLED, Ordering::AcqRel);
    let _ = control.inbox.set(inbox.clone());
    core::sync::atomic::fence(Ordering::SeqCst);
    if control.cancelled() {
        inbox.attention.fetch_or(CANCELLED, Ordering::Release);
    }
    let vm = &mut execution.vm;
    // Everything the fork shared with its spawner's scheduler is replaced
    // before it runs; the spawner's own stays with the spawner.
    let progress = Rc::new(ProgressDriver::island(control.clone(), group.clone()));
    #[cfg(feature = "metrics")]
    {
        vm.metrics = Rc::new(core::cell::RefCell::new(super::VmMetrics::default()));
        vm.profile = super::profile::Samples::default();
    }
    vm.nursery = Rc::new(Nursery::root(
        progress.clone(),
        #[cfg(feature = "metrics")]
        vm.metrics.clone(),
    ));
    vm.progress = progress;
    vm.native_resources = NativeResourceRegistry::new();
    vm.host = vm.host.take().map(|host| {
        let (arguments, argument_map) = configuration
            .unwrap_or_else(|| (Vec::new(), EmptyVmConfiguration.argument_map()));
        Rc::new(ForkHost::new(
            host,
            parent.clone(),
            arguments,
            argument_map,
            variables.clone(),
        )) as Rc<dyn VmHost>
    });
    vm.variables = variables;
    vm.cancel = Some(control.clone());
    vm.attention = inbox.clone();
    // A panic here is a VM bug. Unwinding past this point would leave the
    // spawner waiting for a result that never comes, so it becomes the
    // fork's error instead, and the island's bookkeeping still runs. Release
    // builds abort on a panic, which cannot leave the spawner waiting either.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.run_island(&control)
            .map(|result| result.and_then(|value| vm.sendable_result(value)))
    }))
    .unwrap_or_else(|panic| Some(Err(vm.island_panic_error(panic.as_ref()))));
    drop(execution);
    #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
    crate::sync::solo_check::fork_ended();
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
    group.leave();
    parent.signal.notify();
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
        loop {
            if control.cancelled() {
                return None;
            }
            match self.execute() {
                super::ExecutionOutcome::Settled(result) => {
                    return Some(self.settle_tasks(&result));
                }
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

fn submit(mut job: Job) {
    loop {
        let idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner).pop();
        let Some(worker) = idle else {
            break;
        };
        match worker.send(job) {
            Ok(()) => return,
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
    if let Err(error) = started {
        panic!("cannot start a fork worker thread: {error}");
    }
}

/// The spawner's host, reached from an island: each call runs on the
/// spawner's thread, at the next call, return, or `recur` of any VM there,
/// or when it next waits.
struct Remote {
    host: Rc<dyn VmHost>,
    parent: Arc<Inbox>,
}

impl Remote {
    fn call<R: 'static>(&self, request: impl FnOnce(&dyn VmHost) -> R + 'static) -> R {
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
                return result.into_inner();
            }
            slot = reply.1.wait(slot).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn tell(&self, request: impl FnOnce(&dyn VmHost) + 'static) {
        let host = self.host.clone();
        self.parent.request(move || request(&*host));
    }
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
        arguments: Vec<String>,
        argument_map: Value,
        variables: crate::ModuleVariables,
    ) -> Self {
        let remote = Rc::new(Remote { host, parent });
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
    }

    fn describe_module(
        &self,
        importer: Option<&str>,
        name: &str,
    ) -> Result<VmModuleDescription, VmHostError> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.describe_module(importer.as_deref(), &name))
    }

    fn permits(&self, permission: &str) -> bool {
        let permission = String::from(permission);
        self.remote.call(move |host| host.permits(&permission))
    }

    fn builtin_globals(&self) -> HashMap<String, Value> {
        self.remote.call(|host| host.builtin_globals())
    }

    fn native_globals(&self) -> HashMap<String, Value> {
        self.remote.call(|host| host.native_globals())
    }

    fn foreign_function(&self, module: &str, name: &str) -> Option<NativeFunction> {
        let (module, name) = (String::from(module), String::from(name));
        self.remote
            .call(move |host| host.foreign_function(&module, &name))
    }

    fn define_foreign_batch(
        &self,
        functions: Vec<NativeFunction>,
    ) -> Result<(), NativeDescriptorError> {
        self.remote
            .call(move |host| host.define_foreign_batch(functions))
    }

    fn define_native_global(&self, name: String, value: Value) {
        self.remote
            .tell(move |host| host.define_native_global(name, value));
    }

    fn native_resources(&self) -> NativeResourceRegistry {
        self.resources.clone()
    }

    fn module_initialized(&self, importer: Option<&str>, name: &str) -> Option<bool> {
        let (importer, name) = (importer.map(String::from), String::from(name));
        self.remote
            .call(move |host| host.module_initialized(importer.as_deref(), &name))
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
        let (key, fallback) = (String::from(key), fallback.clone());
        self.remote
            .call(move |host| host.configuration().resolve(&key, &fallback))
    }

    fn arguments(&self) -> &[String] {
        &self.arguments
    }

    fn argument_map(&self) -> Value {
        self.argument_map.clone()
    }
}
