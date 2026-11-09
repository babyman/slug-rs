//! The reference counts the VM shares values with.
//!
//! A build with `threads` counts atomically, so values can be shared with
//! forks on other threads; a build without it, such as an embedded one, keeps
//! the cheaper single-threaded counts.

#[cfg(feature = "threads")]
pub(crate) use alloc::sync::{Arc as Rc, Weak};

#[cfg(not(feature = "threads"))]
pub(crate) use alloc::rc::{Rc, Weak};

/// A value computed on first use, such as a string's offset table. A
/// `threads` build may race to fill it from several threads, so it uses
/// `OnceLock`; other builds keep the cheaper `OnceCell`.
#[cfg(all(feature = "threads", not(target_os = "none")))]
pub(crate) type Once<T> = std::sync::OnceLock<T>;

#[cfg(not(all(feature = "threads", not(target_os = "none"))))]
pub(crate) type Once<T> = core::cell::OnceCell<T>;

/// A cached position in shared bytecode, such as a global's slot or a map
/// key's entry. Any thread may read or refresh it, and a stale one is only
/// a missed hint, so relaxed loads and stores suffice: on the targets the VM
/// runs on they are plain loads and stores, as a `Cell` would be.
#[derive(Debug, Default)]
pub(crate) struct Hint(core::sync::atomic::AtomicU32);

impl Hint {
    pub(crate) const fn new(value: u32) -> Self {
        Self(core::sync::atomic::AtomicU32::new(value))
    }

    #[inline]
    pub(crate) fn get(&self) -> u32 {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn set(&self, value: u32) {
        self.0.store(value, core::sync::atomic::Ordering::Relaxed);
    }
}

impl Clone for Hint {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

/// A cell the VM reads without a lock: module binding cells and module
/// global tables.
///
/// Every such cell is either written by one thread only, or written only
/// before any fork can see it. A `val` cell and a module's table are filled
/// while their module initializes, and no fork exists during initialization;
/// a `var` cell is reached only by its own domain, since a fork reads
/// variables from its snapshot, a write from a fork to anything outside it is
/// refused, and a closure capturing a `var` cannot be sent. Phase 2's domain
/// checks and the initialization rule enforce this.
///
/// Debug builds keep a borrow count and panic on a conflicting borrow, as a
/// `RefCell` would; release builds read and write with no bookkeeping.
/// Debug builds with `threads` also check the rule itself (`solo_check`):
/// a write to a cell another thread has reached panics while any fork runs.
#[doc(hidden)]
pub struct SoloCell<T> {
    value: core::cell::UnsafeCell<T>,
    #[cfg(debug_assertions)]
    borrows: core::sync::atomic::AtomicIsize,
    #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
    seen: core::sync::atomic::AtomicU64,
}

// SAFETY: see the type's documentation. Readers on several threads share
// `&T`, and a writer is alone: either on its value's own thread, or before
// any other thread can reach the cell.
#[cfg(feature = "threads")]
#[allow(
    unsafe_code,
    reason = "the one Sync claim; see the type's documentation"
)]
unsafe impl<T: Send + Sync> Sync for SoloCell<T> {}

impl<T> SoloCell<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            value: core::cell::UnsafeCell::new(value),
            #[cfg(debug_assertions)]
            borrows: core::sync::atomic::AtomicIsize::new(0),
            #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
            seen: core::sync::atomic::AtomicU64::new(solo_check::NONE),
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn borrow(&self) -> SoloRef<'_, T> {
        #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
        solo_check::read(&self.seen);
        #[cfg(debug_assertions)]
        {
            let previous = self
                .borrows
                .fetch_add(1, core::sync::atomic::Ordering::Acquire);
            if previous < 0 {
                self.borrows
                    .fetch_sub(1, core::sync::atomic::Ordering::Release);
                panic!("cell already mutably borrowed");
            }
        }
        SoloRef { cell: self }
    }

    /// A shared borrow, or `None` in a debug build when a mutable one is out.
    #[inline]
    pub(crate) fn try_borrow(&self) -> Option<SoloRef<'_, T>> {
        #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
        solo_check::read(&self.seen);
        #[cfg(debug_assertions)]
        {
            let previous = self
                .borrows
                .fetch_add(1, core::sync::atomic::Ordering::Acquire);
            if previous < 0 {
                self.borrows
                    .fetch_sub(1, core::sync::atomic::Ordering::Release);
                return None;
            }
        }
        Some(SoloRef { cell: self })
    }

    #[inline]
    #[track_caller]
    pub(crate) fn borrow_mut(&self) -> SoloMut<'_, T> {
        #[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
        solo_check::write(&self.seen);
        #[cfg(debug_assertions)]
        if self
            .borrows
            .compare_exchange(
                0,
                -1,
                core::sync::atomic::Ordering::Acquire,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            panic!("cell already borrowed");
        }
        SoloMut { cell: self }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn replace(&self, value: T) -> T {
        core::mem::replace(&mut *self.borrow_mut(), value)
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for SoloCell<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.try_borrow() {
            Some(value) => formatter
                .debug_struct("SoloCell")
                .field("value", &*value)
                .finish(),
            None => formatter.write_str("SoloCell { <borrowed> }"),
        }
    }
}

/// The debug check of [`SoloCell`]'s rule. Each cell records which thread
/// has reached it since it was last written: none, one, or several. A write
/// from a thread other than the one recorded is safe only when no other
/// thread can be reading the cell, which a debug build knows only when no
/// fork is running anywhere; otherwise it panics, naming the write. The
/// check is deterministic: it does not need the read and the write to
/// overlap in time, as the borrow count and a thread sanitizer do.
#[cfg(all(debug_assertions, feature = "threads", not(target_os = "none")))]
pub(crate) mod solo_check {
    use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    /// No thread has reached the cell since it was made.
    pub(super) const NONE: u64 = 0;
    /// More than one thread has.
    const SHARED: u64 = u64::MAX;

    static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
    /// Forks running on islands, in every VM of the process.
    static RUNNING_FORKS: AtomicUsize = AtomicUsize::new(0);

    std::thread_local! {
        static THREAD: u64 = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
    }

    fn thread() -> u64 {
        THREAD.with(|thread| *thread)
    }

    /// An island starts running a fork.
    pub(crate) fn fork_started() {
        RUNNING_FORKS.fetch_add(1, Ordering::AcqRel);
    }

    /// An island has finished with its fork, and reaches none of its cells.
    pub(crate) fn fork_ended() {
        RUNNING_FORKS.fetch_sub(1, Ordering::AcqRel);
    }

    pub(super) fn read(seen: &AtomicU64) {
        let me = thread();
        let mut current = seen.load(Ordering::Acquire);
        while current != me && current != SHARED {
            let next = if current == NONE { me } else { SHARED };
            match seen.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    #[track_caller]
    pub(super) fn write(seen: &AtomicU64) {
        let me = thread();
        let current = seen.load(Ordering::Acquire);
        assert!(
            current == NONE || current == me || RUNNING_FORKS.load(Ordering::Acquire) == 0,
            "a SoloCell that another thread has reached is written while a fork runs; \
             see SoloCell's documentation for the rule this breaks"
        );
        seen.store(me, Ordering::Release);
    }
}

/// A shared borrow of a [`SoloCell`].
pub(crate) struct SoloRef<'a, T> {
    cell: &'a SoloCell<T>,
}

impl<T> core::ops::Deref for SoloRef<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        #[allow(
            unsafe_code,
            reason = "borrow access to a SoloCell; see its documentation"
        )]
        // SAFETY: no writer is active while this borrow lives (see
        // `SoloCell`); debug builds check it.
        unsafe {
            &*self.cell.value.get()
        }
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for SoloRef<'_, T> {
    fn drop(&mut self) {
        self.cell
            .borrows
            .fetch_sub(1, core::sync::atomic::Ordering::Release);
    }
}

/// The one mutable borrow of a [`SoloCell`].
pub(crate) struct SoloMut<'a, T> {
    cell: &'a SoloCell<T>,
}

impl<T> core::ops::Deref for SoloMut<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        #[allow(
            unsafe_code,
            reason = "borrow access to a SoloCell; see its documentation"
        )]
        // SAFETY: this is the only borrow of the cell (see `SoloCell`).
        unsafe {
            &*self.cell.value.get()
        }
    }
}

impl<T> core::ops::DerefMut for SoloMut<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        #[allow(
            unsafe_code,
            reason = "borrow access to a SoloCell; see its documentation"
        )]
        // SAFETY: this is the only borrow of the cell (see `SoloCell`).
        unsafe {
            &mut *self.cell.value.get()
        }
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for SoloMut<'_, T> {
    fn drop(&mut self) {
        self.cell
            .borrows
            .store(0, core::sync::atomic::Ordering::Release);
    }
}

/// State that tasks on several threads change, such as a channel's queues.
/// A `threads` build guards it with a mutex; other builds, which run every
/// task on one thread, keep a `RefCell`. Holders keep a guard briefly and
/// never resume a waiter while holding one.
#[cfg(all(feature = "threads", not(target_os = "none")))]
#[derive(Debug, Default)]
pub(crate) struct Lock<T>(std::sync::Mutex<T>);

#[cfg(all(feature = "threads", not(target_os = "none")))]
impl<T> Lock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }

    /// The guarded value. A panic while it was held leaves it as it was, as
    /// a `RefCell` would, so a poisoned lock is taken anyway.
    #[inline]
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A held [`Lock`].
#[cfg(all(feature = "threads", not(target_os = "none")))]
pub(crate) type LockGuard<'a, T> = std::sync::MutexGuard<'a, T>;

/// A held [`Lock`].
#[cfg(not(all(feature = "threads", not(target_os = "none"))))]
pub(crate) type LockGuard<'a, T> = core::cell::RefMut<'a, T>;

#[cfg(not(all(feature = "threads", not(target_os = "none"))))]
#[derive(Debug, Default)]
pub(crate) struct Lock<T>(core::cell::RefCell<T>);

#[cfg(not(all(feature = "threads", not(target_os = "none"))))]
impl<T> Lock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(core::cell::RefCell::new(value))
    }

    #[inline]
    #[track_caller]
    pub(crate) fn lock(&self) -> core::cell::RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

/// A flag set once and read by tasks on any thread, such as whether a
/// channel has crossed into a fork. Relaxed: it orders nothing else.
#[derive(Debug, Default)]
pub(crate) struct Flag(core::sync::atomic::AtomicBool);

impl Flag {
    pub(crate) const fn new(value: bool) -> Self {
        Self(core::sync::atomic::AtomicBool::new(value))
    }

    #[inline]
    pub(crate) fn get(&self) -> bool {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn set(&self, value: bool) {
        self.0.store(value, core::sync::atomic::Ordering::Relaxed);
    }
}

/// What crosses between a fork's island and the thread that made it: the
/// island itself, its result, and the wakes and host requests between them.
///
/// The VM's values are not `Send` by type, since a task handle or native
/// resource cannot be shared, and its scheduler state is single-threaded.
/// What is wrapped here may cross anyway, because:
///
/// - every value a fork can reach from outside is `val`-only data (programs,
///   strings, collections, closures over `val`s, module tables, and binding
///   cells), whose shared reference counts are atomic and whose interior
///   state is thread-safe (`SoloCell`, `Lock`, `Once`, `Hint`); a value that
///   is not (a task handle, a resource, a closure capturing a `var`) is
///   refused by the sendable check or by the fork's domain checks before it
///   can be used;
/// - scheduler state (tasks, nurseries, waiters, timers, the progress driver,
///   the host) is used only on its own thread: an island gets fresh ones on
///   its worker, and a wake or host request for another thread is only
///   carried to it here, then run there.
///
/// Dropping a wrapped value on the other thread only releases atomic counts,
/// or frees state no other thread can still reach.
#[cfg(all(feature = "threads", not(target_os = "none")))]
pub(crate) struct Sendable<T>(T);

// SAFETY: see the type's documentation.
#[cfg(all(feature = "threads", not(target_os = "none")))]
#[allow(
    unsafe_code,
    reason = "the fork package's one Send claim; see the type's documentation"
)]
unsafe impl<T> Send for Sendable<T> {}

#[cfg(all(feature = "threads", not(target_os = "none")))]
impl<T> Sendable<T> {
    /// Wraps `value` to cross to another thread, under the type's rules.
    pub(crate) fn new(value: T) -> Self {
        Self(value)
    }

    pub(crate) fn into_inner(self) -> T {
        self.0
    }
}

/// The reference-counted pointer for code: programs, closures, global
/// tables, and native functions. A `threads` build counts it atomically, as
/// `Arc`, until its module has initialized; [`immortalize`] then stops its
/// counting, so forks on many cores can clone and drop it without writing
/// its cache line. Other builds keep `Rc`.
///
/// See `docs/decisions/2026-11-08-immortal-code-objects.md`.
#[cfg(feature = "threads")]
#[doc(hidden)]
pub struct Code<T> {
    inner: core::ptr::NonNull<CodeInner<T>>,
    marker: core::marker::PhantomData<CodeInner<T>>,
}

#[cfg(not(feature = "threads"))]
#[doc(hidden)]
pub type Code<T> = alloc::rc::Rc<T>;

/// Stops counting `code`: it lives until the program ends. Without
/// `threads` there is nothing to save, and it does nothing.
#[cfg(not(feature = "threads"))]
#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
pub(crate) fn immortalize<T>(_code: &Code<T>) {}

#[cfg(feature = "threads")]
#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
pub(crate) fn immortalize<T>(code: &Code<T>) {
    code.immortalize();
}

#[cfg(feature = "threads")]
struct CodeInner<T> {
    /// The count of references, or [`IMMORTAL`] set once counting stops.
    count: core::sync::atomic::AtomicUsize,
    value: T,
}

/// The count's top bit: set, the object is never counted or freed again.
#[cfg(feature = "threads")]
const IMMORTAL: usize = 1 << (usize::BITS - 1);

/// Past this many references a count aborts, as `Arc`'s does, rather than
/// reach the immortal bit.
#[cfg(feature = "threads")]
const MAX_COUNT: usize = IMMORTAL >> 1;

// SAFETY: as for `Arc<T>`: shared references to `T` cross threads, and the
// last reference, wherever it is dropped, frees `T`.
#[cfg(feature = "threads")]
#[allow(unsafe_code, reason = "Code is Arc's equal; see its documentation")]
unsafe impl<T: Send + Sync> Send for Code<T> {}

// SAFETY: as for `Arc<T>`.
#[cfg(feature = "threads")]
#[allow(unsafe_code, reason = "Code is Arc's equal; see its documentation")]
unsafe impl<T: Send + Sync> Sync for Code<T> {}

#[cfg(feature = "threads")]
impl<T> Code<T> {
    pub fn new(value: T) -> Self {
        let inner = alloc::boxed::Box::new(CodeInner {
            count: core::sync::atomic::AtomicUsize::new(1),
            value,
        });
        Self {
            inner: core::ptr::NonNull::from(alloc::boxed::Box::leak(inner)),
            marker: core::marker::PhantomData,
        }
    }

    #[inline]
    fn inner(&self) -> &CodeInner<T> {
        #[allow(unsafe_code, reason = "Code's allocation; see its documentation")]
        // SAFETY: the allocation lives while any `Code` to it does, and this
        // one does.
        unsafe {
            self.inner.as_ref()
        }
    }

    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        this.inner == other.inner
    }

    #[must_use]
    pub fn as_ptr(this: &Self) -> *const T {
        core::ptr::addr_of!(this.inner().value)
    }

    /// The value, mutably, when this is its only reference. An immortal
    /// value always reads as shared, so it is never changed in place.
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        if this
            .inner()
            .count
            .load(core::sync::atomic::Ordering::Acquire)
            != 1
        {
            return None;
        }
        #[allow(unsafe_code, reason = "Code's allocation; see its documentation")]
        // SAFETY: the count is one, so `this` is the only reference, and the
        // `&mut` it is borrowed through keeps it so.
        unsafe {
            Some(&mut this.inner.as_mut().value)
        }
    }

    /// How many references there are, or `usize::MAX` once immortal.
    #[must_use]
    pub fn strong_count(this: &Self) -> usize {
        let count = this
            .inner()
            .count
            .load(core::sync::atomic::Ordering::Relaxed);
        if count & IMMORTAL == 0 {
            count
        } else {
            usize::MAX
        }
    }

    fn immortalize(&self) {
        self.inner()
            .count
            .fetch_or(IMMORTAL, core::sync::atomic::Ordering::Relaxed);
    }

    #[cfg(test)]
    fn is_immortal(&self) -> bool {
        self.inner()
            .count
            .load(core::sync::atomic::Ordering::Relaxed)
            & IMMORTAL
            != 0
    }
}

#[cfg(feature = "threads")]
impl<T> Clone for Code<T> {
    #[inline]
    fn clone(&self) -> Self {
        let count = &self.inner().count;
        // An immortal count is only read, so every core keeps its line.
        // Relaxed, as in `Arc`: a new reference comes from an existing one,
        // so the object cannot be freed meanwhile.
        if count.load(core::sync::atomic::Ordering::Relaxed) & IMMORTAL == 0
            && count.fetch_add(1, core::sync::atomic::Ordering::Relaxed) & !IMMORTAL > MAX_COUNT
        {
            too_many_references();
        }
        Self {
            inner: self.inner,
            marker: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "threads")]
#[cold]
#[inline(never)]
fn too_many_references() -> ! {
    #[cfg(not(target_os = "none"))]
    std::process::abort();
    #[cfg(target_os = "none")]
    panic!("too many references to one code object");
}

#[cfg(feature = "threads")]
impl<T> Drop for Code<T> {
    #[inline]
    fn drop(&mut self) {
        let count = &self.inner().count;
        // A count made immortal meanwhile keeps its top bit, so it never
        // reads as the last reference.
        if count.load(core::sync::atomic::Ordering::Relaxed) & IMMORTAL == 0
            && count.fetch_sub(1, core::sync::atomic::Ordering::Release) == 1
        {
            self.free();
        }
    }
}

#[cfg(feature = "threads")]
impl<T> Code<T> {
    /// Frees the value once its last reference is gone; out of line, as
    /// `Arc`'s is, so the common drop stays small where it is inlined.
    #[cold]
    #[inline(never)]
    fn free(&mut self) {
        // Release in `drop`, then Acquire here, as in `Arc`.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        #[allow(unsafe_code, reason = "Code's allocation; see its documentation")]
        // SAFETY: this was the last reference, and the allocation came from
        // `Box::leak` in `new`.
        unsafe {
            drop(alloc::boxed::Box::from_raw(self.inner.as_ptr()));
        }
    }
}

#[cfg(feature = "threads")]
impl<T> core::ops::Deref for Code<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner().value
    }
}

#[cfg(feature = "threads")]
impl<T> AsRef<T> for Code<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

#[cfg(feature = "threads")]
impl<T: core::fmt::Debug> core::fmt::Debug for Code<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, formatter)
    }
}

#[cfg(all(test, feature = "threads"))]
mod code_tests {
    use super::{Code, immortalize};
    use core::sync::atomic::{AtomicUsize, Ordering};

    struct Counted<'a>(&'a AtomicUsize);

    impl Drop for Counted<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn the_last_reference_frees_the_value_once() {
        let drops = AtomicUsize::new(0);
        let code = Code::new(Counted(&drops));
        let copies = (0..5).map(|_| code.clone()).collect::<alloc::vec::Vec<_>>();
        drop(code);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(copies);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_immortal_value_is_never_freed() {
        let drops = AtomicUsize::new(0);
        let code = Code::new(Counted(&drops));
        immortalize(&code);
        assert!(code.is_immortal());
        let copy = code.clone();
        drop(code);
        drop(copy);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn threads_share_an_immortal_value() {
        let code = Code::new(alloc::vec![1, 2, 3]);
        immortalize(&code);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let code = &code;
                scope.spawn(move || {
                    for _ in 0..10_000 {
                        assert_eq!(code.clone().len(), 3);
                    }
                });
            }
        });
        assert!(code.is_immortal());
    }

    #[test]
    fn threads_count_a_mortal_value() {
        let drops = AtomicUsize::new(0);
        let code = Code::new(Counted(&drops));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let code = &code;
                scope.spawn(move || {
                    for _ in 0..10_000 {
                        drop(code.clone());
                    }
                });
            }
        });
        drop(code);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}

#[cfg(all(test, debug_assertions, feature = "threads", not(target_os = "none")))]
mod solo_check_tests {
    use super::{SoloCell, solo_check};

    /// A fork runs while this lives, as far as the check knows.
    struct Running;

    impl Running {
        fn start() -> Self {
            solo_check::fork_started();
            Self
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            solo_check::fork_ended();
        }
    }

    #[test]
    #[should_panic(expected = "another thread has reached is written while a fork runs")]
    fn writing_a_cell_another_thread_read_while_a_fork_runs_panics() {
        let cell = SoloCell::new(1);
        let _running = Running::start();
        std::thread::scope(|scope| {
            scope.spawn(|| assert_eq!(*cell.borrow(), 1));
        });
        *cell.borrow_mut() = 2;
    }

    #[test]
    fn a_cell_only_its_thread_reached_is_written_while_a_fork_runs() {
        let cell = SoloCell::new(1);
        let _running = Running::start();
        assert_eq!(*cell.borrow(), 1);
        *cell.borrow_mut() = 2;
        assert_eq!(*cell.borrow(), 2);
    }

    #[test]
    fn a_cell_written_before_another_thread_reads_it_is_shared() {
        let cell = SoloCell::new(1);
        *cell.borrow_mut() = 2;
        let _running = Running::start();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| assert_eq!(*cell.borrow(), 2));
            }
        });
        assert_eq!(*cell.borrow(), 2);
    }
}
