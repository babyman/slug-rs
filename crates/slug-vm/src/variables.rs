//! Every module variable of a host's VMs, so a task can take its own copy.
//!
//! A `task var` belongs to each task, and a fork sees a snapshot of every
//! `var`, so starting either reads the current value of each registered
//! variable. A module's variables are binding cells, shared with the modules
//! that import them; an entry program's are entries of its global table.

use alloc::rc::Rc;
use core::cell::RefCell;

use crate::{
    Program, Value,
    platform::HashMap,
    value::{BindingCell, GlobalEnvironment},
};

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
    kind: core::cell::Cell<BindingKind>,
}

impl BindingName {
    pub(crate) fn new(name: alloc::string::String, kind: BindingKind) -> Self {
        Self {
            name,
            kind: core::cell::Cell::new(kind),
        }
    }

    #[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
    pub(crate) fn kind(&self) -> BindingKind {
        self.kind.get()
    }

    #[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
    pub(crate) fn set_kind(&self, kind: BindingKind) {
        self.kind.set(kind);
    }
}

impl core::fmt::Display for BindingName {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.name)
    }
}

/// The identity of one module variable: its binding cell, or its slot in an
/// entry program's global table.
#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum VarKey {
    Cell(usize),
    Slot(usize, u32),
}

#[cfg_attr(not(feature = "concurrency"), allow(dead_code))]
impl VarKey {
    pub(crate) fn cell(cell: &BindingCell) -> Self {
        Self::Cell(Rc::as_ptr(cell).cast::<()>() as usize)
    }

    pub(crate) fn slot(globals: &GlobalEnvironment, slot: u32) -> Self {
        Self::Slot(Rc::as_ptr(globals).cast::<()>() as usize, slot)
    }
}

enum VarSource {
    Cell(BindingCell),
    Slot(GlobalEnvironment, u32),
}

struct ModuleVariable {
    source: VarSource,
    task: bool,
}

impl ModuleVariable {
    fn current(&self) -> Value {
        match &self.source {
            VarSource::Cell(cell) => cell.borrow().clone(),
            VarSource::Slot(globals, slot) => globals.borrow().value(*slot).clone(),
        }
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
    entries: HashMap<VarKey, ModuleVariable>,
    task_vars: usize,
    /// The last domain id given to a fork; zero is outside every fork.
    domains: u32,
}

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
                source: VarSource::Cell(cell.clone()),
                task,
            },
        );
    }

    pub(crate) fn register_slot(&self, globals: &GlobalEnvironment, slot: u32, task: bool) {
        self.register(
            VarKey::slot(globals, slot),
            ModuleVariable {
                source: VarSource::Slot(globals.clone(), slot),
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
    pub(crate) fn next_domain(&self) -> u32 {
        let mut variables = self.0.borrow_mut();
        variables.domains = variables.domains.wrapping_add(1).max(1);
        variables.domains
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
    ) -> HashMap<VarKey, Value> {
        self.0
            .borrow()
            .entries
            .iter()
            .filter(|(_, variable)| variable.task == task)
            .map(|(key, variable)| (*key, seen(*key).unwrap_or_else(|| variable.current())))
            .collect()
    }
}
