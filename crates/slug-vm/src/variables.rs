//! Every module variable of a host's VMs, so a task can take its own copy.
//!
//! A `task var` belongs to each task, and a fork sees a snapshot of every
//! `var`, so starting either reads the current value of each registered
//! variable. A module's variables are binding cells, shared with the modules
//! that import them; an entry program's are entries of its global table.

use crate::sync::Rc;
use core::cell::RefCell;

use crate::{Program, Value, value::BindingCell};

/// The kind of the module-level binding `name` that `program` declares.
#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
pub(crate) fn declared_kind(program: &Program, name: &str) -> BindingKind {
    program
        .declarations()
        .iter()
        .find(|declaration| declaration.bindings.iter().any(|binding| binding == name))
        .map_or(BindingKind::Val, |declaration| {
            BindingKind::of(declaration.mutable, declaration.task)
        })
}

/// Whether a module-level binding is a `val`, a `var`, or a `task var`.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum BindingKind {
    #[default]
    Val,
    Var,
    TaskVar,
}

impl BindingKind {
    pub(crate) fn of(mutable: bool, task: bool) -> Self {
        match (mutable, task) {
            (false, _) => Self::Val,
            (true, false) => Self::Var,
            (true, true) => Self::TaskVar,
        }
    }
}

/// A module binding's name, and whether it is a `val`, `var`, or `task var`.
/// Every module that imports the binding shares this record, so marking the
/// kind once, when its module installs, marks it for all of them.
#[doc(hidden)]
#[derive(Debug)]
pub struct BindingName {
    name: alloc::string::String,
    /// Set when its module installs, before any fork can read it.
    kind: core::sync::atomic::AtomicU8,
}

impl BindingName {
    pub(crate) fn new(name: alloc::string::String, kind: BindingKind) -> Self {
        Self {
            name,
            kind: core::sync::atomic::AtomicU8::new(kind as u8),
        }
    }

    #[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
    pub(crate) fn kind(&self) -> BindingKind {
        match self.kind.load(core::sync::atomic::Ordering::Relaxed) {
            0 => BindingKind::Val,
            1 => BindingKind::Var,
            _ => BindingKind::TaskVar,
        }
    }

    #[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
    pub(crate) fn set_kind(&self, kind: BindingKind) {
        self.kind
            .store(kind as u8, core::sync::atomic::Ordering::Relaxed);
    }
}

impl core::fmt::Display for BindingName {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.name)
    }
}

/// The identity of one module variable: its binding cell.
#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum VarKey {
    Cell(usize),
}

#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
impl VarKey {
    pub(crate) fn cell(cell: &BindingCell) -> Self {
        Self::Cell(Rc::as_ptr(cell).cast::<()>() as usize)
    }
}

/// A map keyed by module variable, hashed by cell address.
#[cfg(not(target_os = "none"))]
pub(crate) type VarMap<V> = crate::platform::HashMap<
    VarKey,
    V,
    core::hash::BuildHasherDefault<crate::platform::AddressHasher>,
>;

#[cfg(target_os = "none")]
pub(crate) type VarMap<V> = crate::platform::HashMap<VarKey, V>;

struct ModuleVariable {
    cell: BindingCell,
    task: bool,
}

impl ModuleVariable {
    fn current(&self) -> Value {
        self.cell.borrow().clone()
    }
}

/// The module variables of every VM sharing one host.
#[derive(Clone, Default)]
pub struct ModuleVariables(Rc<RefCell<Variables>>);

/// While it lives, a module is initializing, so no fork may start.
pub(crate) struct Initializing(ModuleVariables);

impl Drop for Initializing {
    fn drop(&mut self) {
        let mut variables = self.0.0.borrow_mut();
        variables.initializing = variables.initializing.saturating_sub(1);
    }
}

#[derive(Default)]
struct Variables {
    /// How many modules are initializing now.
    initializing: usize,
    entries: VarMap<ModuleVariable>,
    task_vars: usize,
}

/// The last domain id given to a fork; zero is outside every fork. Global, so
/// forks on every island and host draw distinct ids.
static DOMAINS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Creates an empty registry, for a host to share with the VMs it creates.
#[must_use]
pub fn module_variables() -> ModuleVariables {
    ModuleVariables::default()
}

#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
impl ModuleVariables {
    pub(crate) fn register_cell(&self, cell: &BindingCell, task: bool) {
        self.register(
            VarKey::cell(cell),
            ModuleVariable {
                cell: cell.clone(),
                task,
            },
        );
    }

    fn register(&self, key: VarKey, variable: ModuleVariable) {
        let mut variables = self.0.borrow_mut();
        let task = variable.task;
        if variables
            .entries
            .insert(key, variable)
            .is_some_and(|previous| previous.task)
        {
            variables.task_vars -= 1;
        }
        if task {
            variables.task_vars += 1;
        }
    }

    /// Marks a module as initializing until the result is dropped.
    pub(crate) fn initializing(&self) -> Initializing {
        self.0.borrow_mut().initializing += 1;
        Initializing(self.clone())
    }

    /// Whether any module is initializing, so a fork cannot start.
    pub(crate) fn any_initializing(&self) -> bool {
        self.0.borrow().initializing > 0
    }

    /// A new id for a fork's domain: what it makes is its own.
    pub(crate) fn next_domain() -> u32 {
        loop {
            let domain = DOMAINS
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
                .wrapping_add(1);
            if domain != 0 {
                return domain;
            }
        }
    }

    /// A registry for a fork's island: the variables registered now, with no
    /// module initializing, since a fork cannot initialize one. The island
    /// reads values only through its snapshot, so this registry is never
    /// shared with the thread that made it.
    #[cfg(all(feature = "threads", not(target_os = "none")))]
    pub(crate) fn island_copy(&self) -> Self {
        let variables = self.0.borrow();
        Self(Rc::new(RefCell::new(Variables {
            initializing: 0,
            entries: variables
                .entries
                .iter()
                .map(|(key, variable)| {
                    (
                        *key,
                        ModuleVariable {
                            cell: variable.cell.clone(),
                            task: variable.task,
                        },
                    )
                })
                .collect(),
            task_vars: variables.task_vars,
        })))
    }

    /// Whether any `task var` is registered, so a new task needs copies.
    pub(crate) fn has_task_vars(&self) -> bool {
        self.0.borrow().task_vars > 0
    }

    /// The `task var`s (`task`) or the other `var`s, each with the value
    /// `seen` gives for it, or else its current value.
    pub(crate) fn snapshot(
        &self,
        task: bool,
        seen: impl Fn(VarKey) -> Option<Value>,
    ) -> VarMap<Value> {
        self.0
            .borrow()
            .entries
            .iter()
            .filter(|(_, variable)| variable.task == task)
            .map(|(key, variable)| (*key, seen(*key).unwrap_or_else(|| variable.current())))
            .collect()
    }
}
