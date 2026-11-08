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
#[doc(hidden)]
pub struct SoloCell<T> {
    value: core::cell::UnsafeCell<T>,
    #[cfg(debug_assertions)]
    borrows: core::sync::atomic::AtomicIsize,
}

// SAFETY: see the type's documentation. Readers on several threads share
// `&T`, and a writer is alone: either on its value's own thread, or before
// any other thread can reach the cell.
#[cfg(feature = "threads")]
#[allow(unsafe_code, reason = "the one Sync claim; see the type's documentation")]
unsafe impl<T: Send + Sync> Sync for SoloCell<T> {}

impl<T> SoloCell<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            value: core::cell::UnsafeCell::new(value),
            #[cfg(debug_assertions)]
            borrows: core::sync::atomic::AtomicIsize::new(0),
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn borrow(&self) -> SoloRef<'_, T> {
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

/// A shared borrow of a [`SoloCell`].
pub(crate) struct SoloRef<'a, T> {
    cell: &'a SoloCell<T>,
}

impl<T> core::ops::Deref for SoloRef<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        #[allow(unsafe_code, reason = "borrow access to a SoloCell; see its documentation")]
        // SAFETY: no writer is active while this borrow lives (see
        // `SoloCell`); debug builds check it.
        unsafe { &*self.cell.value.get() }
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
        #[allow(unsafe_code, reason = "borrow access to a SoloCell; see its documentation")]
        // SAFETY: this is the only borrow of the cell (see `SoloCell`).
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> core::ops::DerefMut for SoloMut<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        #[allow(unsafe_code, reason = "borrow access to a SoloCell; see its documentation")]
        // SAFETY: this is the only borrow of the cell (see `SoloCell`).
        unsafe { &mut *self.cell.value.get() }
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
#[allow(unsafe_code, reason = "the fork package's one Send claim; see the type's documentation")]
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
