//! What a task sees of the module variables, and what a fork may not do.
//!
//! Every task owns a copy of each `task var`, taken from its spawner when it
//! starts. A fork, a task started by `fork`, also sees a snapshot of its
//! captures and of every other `var`, and cannot change what it did not
//! make. Its context is shared by every task it spawns and nursery it opens,
//! which are inside the fork too.

use crate::sync::Rc;
use alloc::string::String;
use core::cell::RefCell;

use crate::{
    SourceSpan, Value,
    platform::HashSet,
    value::BindingCell,
    variables::{BindingKind, VarKey, VarMap},
};

/// What one task sees of the module variables, when it does not simply see
/// them: inside a fork, or with `task var`s of its own.
pub(super) struct TaskView {
    pub(super) fork: Option<Rc<ForkContext>>,
    /// This task's copy of each `task var`.
    own: RefCell<VarMap<Value>>,
}

impl TaskView {
    pub(super) fn new(fork: Option<Rc<ForkContext>>, own: VarMap<Value>) -> Self {
        Self {
            fork,
            own: RefCell::new(own),
        }
    }

    /// The value this task sees for the variable `key` of `kind`, when it is
    /// not the variable's own: its `task var` copy, or the fork's snapshot.
    pub(super) fn value(&self, key: VarKey, kind: BindingKind) -> Option<Value> {
        match kind {
            BindingKind::Val => None,
            BindingKind::TaskVar => self.own.borrow().get(&key).cloned(),
            BindingKind::Var => self.fork.as_ref()?.vars.get(&key).cloned(),
        }
    }

    /// The value of this task's copy of the `task var` `key`, if it has one.
    pub(super) fn own_value(&self, key: VarKey) -> Option<Value> {
        self.own.borrow().get(&key).cloned()
    }

    /// Replaces this task's copy of the `task var` `key`, returning the old
    /// value, or hands `value` back when it has no copy.
    pub(super) fn replace_own(&self, key: VarKey, value: Value) -> Result<Value, Value> {
        match self.own.borrow_mut().get_mut(&key) {
            Some(slot) => Ok(core::mem::replace(slot, value)),
            None => Err(value),
        }
    }

    /// Whether this task is inside a fork.
    pub(super) fn in_fork(&self) -> bool {
        self.fork.is_some()
    }

    pub(super) fn set_own(&self, key: VarKey, value: Value) {
        self.own.borrow_mut().insert(key, value);
    }
}

pub(super) struct ForkContext {
    /// What the fork and the tasks inside it make belongs to this domain.
    pub(super) domain: u32,
    /// Where the fork started, for errors.
    span: Option<SourceSpan>,
    /// The capture cells the fork's snapshot made, which it cannot assign.
    frozen: HashSet<usize>,
    /// Every module `var` but the `task var`s, as the fork started.
    vars: VarMap<Value>,
}

impl ForkContext {
    pub(super) fn new(
        domain: u32,
        span: Option<SourceSpan>,
        captures: &[BindingCell],
        vars: VarMap<Value>,
    ) -> Self {
        Self {
            domain,
            span,
            frozen: captures.iter().map(cell_key).collect(),
            vars,
        }
    }

    /// Whether `cell` is a capture the fork's snapshot made.
    pub(super) fn is_frozen(&self, cell: &BindingCell) -> bool {
        self.frozen.contains(&cell_key(cell))
    }

    /// The fork's snapshot of the module `var` `key`.
    pub(super) fn var(&self, key: VarKey) -> Option<Value> {
        self.vars.get(&key).cloned()
    }

    /// ` at line:column`, or nothing when the fork has no source span.
    pub(super) fn location(&self) -> String {
        self.span.as_ref().map_or_else(String::new, |span| {
            alloc::format!(" at {}:{}", span.line, span.column)
        })
    }
}

fn cell_key(cell: &BindingCell) -> usize {
    crate::sync::Code::as_ptr(cell).cast::<()>() as usize
}

use crate::{Program, value::GlobalTable, variables::declared_kind};

use super::{RuntimeErrorKind, Vm, VmResult};

impl Vm {
    /// Marks and registers the module variables of `program`, whose names
    /// this VM already holds as binding cells: a module's, before it runs,
    /// so the modules that import a `var` share its kind.
    pub(super) fn register_module_variables(&self, program: &Program) {
        let mut globals = self.globals.borrow_mut();
        for declaration in program
            .declarations()
            .iter()
            .filter(|declaration| declaration.mutable)
        {
            let kind = BindingKind::of(true, declaration.task);
            for name in &declaration.bindings {
                let Some(slot) = globals.slot(name) else {
                    continue;
                };
                if matches!(globals.value(slot), Value::Binding { .. })
                    && let Some(cell) = globals.bind_variable(slot, kind)
                {
                    self.variables.register_cell(&cell, declaration.task);
                }
            }
        }
    }

    /// Registers `name` once `program` has defined it, when it is a
    /// variable: an entry program's variable moves into a binding cell, as a
    /// module's already is, so its table never changes after its top level.
    pub(super) fn register_defined_variable(&self, program: &Program, name: &str) {
        let kind = declared_kind(program, name);
        if kind == BindingKind::Val {
            return;
        }
        let mut globals = self.globals.borrow_mut();
        let Some(slot) = globals.slot(name) else {
            return;
        };
        // A module's binding cells were registered when it installed.
        if matches!(globals.value(slot), Value::Binding { .. }) {
            return;
        }
        let Some(cell) = globals.bind_variable(slot, kind) else {
            return;
        };
        drop(globals);
        self.variables
            .register_cell(&cell, kind == BindingKind::TaskVar);
    }

    /// The domain of the fork this VM's task runs in, or zero outside forks.
    pub(super) fn domain(&self) -> u32 {
        self.domain
    }

    pub(super) fn fork_context(&self) -> Option<&Rc<ForkContext>> {
        self.view.as_ref().and_then(|view| view.fork.as_ref())
    }

    /// The identity and kind of the variable in `slot`.
    fn variable_at(globals: &GlobalTable, slot: u32) -> Option<(VarKey, BindingKind)> {
        match globals.value(slot) {
            Value::Binding { cell, name } => Some((VarKey::cell(cell), name.kind())),
            _ => None,
        }
    }

    /// What `view` sees in `slot`, when it is not the variable's own value.
    pub(super) fn viewed_value(view: &TaskView, globals: &GlobalTable, slot: u32) -> Option<Value> {
        let (key, kind) = Self::variable_at(globals, slot)?;
        view.value(key, kind)
    }

    /// What this task sees of the module binding `cell`, `name`, however it
    /// reached it, by its own name or through a module's import map:
    /// `None` when that is the cell's own value. A fork reads a variable only
    /// through its snapshot, never the cell, which belongs to another thread;
    /// one it has no snapshot of, from a module that initialized after the
    /// fork started, it cannot read.
    pub(super) fn viewed_binding(
        &self,
        cell: &BindingCell,
        name: &crate::variables::BindingName,
    ) -> Result<Option<Value>, String> {
        let Some(view) = &self.view else {
            return Ok(None);
        };
        let kind = name.kind();
        if kind == BindingKind::Val {
            return Ok(None);
        }
        if let Some(value) = view.value(VarKey::cell(cell), kind) {
            return Ok(Some(value));
        }
        if view.in_fork() {
            return Err(alloc::format!(
                "a fork cannot read `{name}`: its module initialized after the fork started, so the fork has no snapshot of it"
            ));
        }
        Ok(None)
    }
}

impl Vm {
    /// Assigns `value` to the module-level `name` as this task sees it: its
    /// own copy of a `task var`, or, in a fork, an error for any other
    /// `var`. Hands `value` back when the assignment changes the variable
    /// itself.
    pub(super) fn set_viewed_global(&self, name: &str, value: Value) -> VmResult<Option<Value>> {
        let Some(view) = &self.view else {
            return Ok(Some(value));
        };
        let globals = self.globals.borrow();
        let Some(slot) = globals.slot(name) else {
            return Ok(Some(value));
        };
        let Some((key, kind)) = Self::variable_at(&globals, slot) else {
            return Ok(Some(value));
        };
        drop(globals);
        match (kind, &view.fork) {
            (BindingKind::TaskVar, _) => {
                view.set_own(key, value);
                Ok(None)
            }
            (BindingKind::Var, Some(fork)) => Err(self.error_at(
                RuntimeErrorKind::Fork,
                alloc::format!(
                    "a fork cannot change `{name}`, a module `var`: the fork{} sees a snapshot of it, so the change would never reach the rest of the program; return the value from the fork, or declare it `task var {name}` if each task's copy is its own",
                    fork.location()
                ),
            )),
            _ => Ok(Some(value)),
        }
    }

    /// What a task started now sees: its own copy of each `task var`, as
    /// this task sees it, and, for a fork (`forked`), a snapshot of every
    /// other `var` and of `captures`; a task started inside a fork is inside
    /// it too.
    pub(super) fn task_view(&self, forked: bool, captures: &[BindingCell]) -> Option<Rc<TaskView>> {
        let parent = self.view.as_ref();
        let own = if self.variables.has_task_vars() {
            self.variables
                .snapshot(true, |key| parent.and_then(|view| view.own_value(key)))
        } else {
            VarMap::default()
        };
        let enclosing = parent.and_then(|view| view.fork.clone());
        let fork = if forked {
            let vars = self.variables.snapshot(false, |key| {
                enclosing.as_ref().and_then(|fork| fork.var(key))
            });
            Some(Rc::new(ForkContext::new(
                crate::ModuleVariables::next_domain(),
                self.owned_span(None),
                captures,
                vars,
            )))
        } else {
            enclosing
        };
        (fork.is_some() || !own.is_empty()).then(|| Rc::new(TaskView::new(fork, own)))
    }
}

impl Vm {
    /// Whether this task is a fork that cannot call `closure`: a function
    /// made outside it that captures a `var`, which its creator can still
    /// change.
    #[cold]
    #[inline(never)]
    pub(super) fn refuses_fork_call(&self, closure: &crate::Closure) -> bool {
        self.fork_context().is_some()
            && !closure.captures.is_empty()
            && captured_var(closure).is_some()
    }

    #[cold]
    #[inline(never)]
    pub(super) fn fork_call_error(&self, closure: &crate::Closure) -> super::RuntimeError {
        let name = captured_var(closure).unwrap_or_default();
        self.error_at(
            RuntimeErrorKind::Fork,
            alloc::format!(
                "a fork cannot call a function made outside it that captures `{name}`, a `var` its creator can still change, so the function is not sendable; pass the fork the values it needs instead"
            ),
        )
    }

    /// Checks a send on `channel`: from a fork, or on a channel a fork has
    /// used, the value must be sendable.
    #[cold]
    #[inline(never)]
    pub(super) fn check_channel_send(
        &self,
        channel: &crate::Channel,
        value: &Value,
    ) -> VmResult<()> {
        if self.fork_context().is_some() {
            channel.mark_crossed();
        } else if !channel.crossed() {
            return Ok(());
        }
        self.check_sendable(value, "a value sent on a channel a fork uses")
    }

    /// `value`, a fork's result, when it is sendable.
    pub(super) fn sendable_result(&self, value: Value) -> VmResult<Value> {
        self.check_sendable(&value, "this fork's result")?;
        Ok(value)
    }

    /// A fork's outcome, as it leaves the fork: its result when sendable,
    /// and its error when every value the error carries is, its thrown
    /// value, a native error's data, and those of its causes. An error
    /// carrying one that is not becomes a fork error, with the original,
    /// stripped of its values, as its cause.
    pub(super) fn sendable_outcome(&self, outcome: VmResult<Value>) -> VmResult<Value> {
        let error = match outcome {
            Ok(value) => return self.sendable_result(value),
            Err(error) => error,
        };
        let mut current = Some(&error);
        while let Some(link) = current {
            let carried = link
                .thrown
                .as_deref()
                .into_iter()
                .chain(link.native.as_ref().and_then(|native| native.data.as_ref()));
            for value in carried {
                if let Some(reason) = unsendable(value) {
                    let mut stripped = error.clone();
                    strip_values(&mut stripped);
                    let mut replacement = self.error_at(
                        RuntimeErrorKind::Fork,
                        alloc::format!(
                            "this fork's error is not sendable: the value it carries holds {reason}, which stays with the task that made it"
                        ),
                    );
                    replacement.cause = Some(alloc::boxed::Box::new(stripped));
                    return Err(replacement);
                }
            }
            current = link.cause.as_deref();
        }
        Err(error)
    }

    pub(super) fn check_sendable(&self, value: &Value, what: &str) -> VmResult<()> {
        match unsendable(value) {
            Some(reason) => Err(self.error_at(
                RuntimeErrorKind::Fork,
                alloc::format!(
                    "{what} is not sendable: it holds {reason}, which stays with the task that made it"
                ),
            )),
            None => Ok(()),
        }
    }
}

/// Drops the values `error` and its causes carry, keeping what describes
/// them.
fn strip_values(error: &mut crate::RuntimeError) {
    error.thrown = None;
    if let Some(native) = &mut error.native {
        native.data = None;
    }
    if let Some(cause) = &mut error.cause {
        strip_values(cause);
    }
}

/// The name of a `var` that `closure` captures, if it captures one.
fn captured_var(closure: &crate::Closure) -> Option<String> {
    let chunk = closure.program.chunk(closure.chunk)?;
    chunk
        .captures
        .iter()
        .take(closure.captures.len())
        .find(|capture| capture.mutable)
        .map(|capture| capture.name.clone())
}

/// Why `value` cannot cross between a fork and the rest of the program:
/// what in it is a task handle, a native resource, or a function capturing
/// a `var`.
pub(crate) fn unsendable(value: &Value) -> Option<String> {
    let mut pending = alloc::vec![value.clone()];
    let mut closures = HashSet::new();
    while let Some(value) = pending.pop() {
        match value {
            Value::Task(_) => return Some("a task handle".into()),
            Value::NativeResource(_) => return Some("a native resource".into()),
            Value::List(list) => pending.extend(list.iter().cloned()),
            Value::Map(map) => {
                for (key, value) in map.iter() {
                    pending.push(key.clone());
                    pending.push(value.clone());
                }
            }
            Value::Struct(value) => pending.extend(value.values.iter().cloned()),
            Value::Overloads(values) => pending.extend(values.iter().cloned()),
            Value::Binding { cell, .. } => pending.push(cell.borrow().clone()),
            Value::Closure(closure) => {
                if !closures.insert(crate::sync::Code::as_ptr(&closure).cast::<()>() as usize) {
                    continue;
                }
                if let Some(name) = captured_var(&closure) {
                    return Some(alloc::format!("a function that captures `{name}`, a `var`"));
                }
                pending.extend(closure.captures.iter().map(|cell| cell.borrow().clone()));
            }
            Value::Uninitialized
            | Value::Nil
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Float(_)
            | Value::Str(_)
            | Value::Bytes(_)
            | Value::StructSchema(_)
            | Value::Enum(_)
            | Value::Channel(_)
            | Value::Native(_)
            | Value::DeclaredNative { .. }
            | Value::Builtin(_) => {}
        }
    }
    None
}

impl Vm {
    /// A fork may call only a native function its clutch declares
    /// thread-safe.
    #[cold]
    #[inline(never)]
    pub(super) fn check_fork_native(&self, function: &crate::NativeFunction) -> VmResult<()> {
        if self.fork_context().is_none() || function.is_thread_safe() {
            return Ok(());
        }
        Err(self.error_at(
            RuntimeErrorKind::Fork,
            alloc::format!(
                "a fork cannot call `{}`: its clutch does not declare it thread-safe, so only tasks outside forks may call it",
                function.name()
            ),
        ))
    }
}

impl Vm {
    /// Stops counting this module's code once it has initialized: its
    /// global table, its program, and the closures and native functions its
    /// `val`s hold. Forks on any core then call them without writing their
    /// counts. A `var`'s value is left counted, since it changes.
    ///
    /// See `docs/decisions/2026-11-08-immortal-code-objects.md`.
    pub(super) fn immortalize_code(&self) {
        crate::sync::immortalize(&self.globals);
        if let Some(program) = &self.module_program {
            immortalize_program(program);
        }
        for (_, value) in self.globals.borrow().iter() {
            immortalize_value(value);
        }
    }
}

/// Stops counting `program` and the string constants of its chunks, which
/// every instruction that pushes one clones. A program already immortal was
/// walked before: a module's closures share its program. Without `threads`
/// nothing is counted atomically, so nothing is walked.
fn immortalize_program(program: &crate::sync::Code<crate::Program>) {
    #[cfg(feature = "threads")]
    if crate::sync::Code::strong_count(program) != usize::MAX {
        crate::sync::immortalize(program);
        immortalize_constants(program);
    }
    #[cfg(not(feature = "threads"))]
    let _ = program;
}

#[cfg_attr(not(feature = "threads"), allow(dead_code))]
fn immortalize_constants(program: &crate::Program) {
    for index in 0..program.chunk_count() {
        let Some(chunk) = program.chunk(index) else {
            continue;
        };
        for constant in &chunk.constants {
            if let crate::Constant::Value(Value::Str(text)) = constant {
                text.immortalize();
            }
        }
    }
}

fn immortalize_value(value: &Value) {
    match value {
        // A module's binding cells and names live as long as its table, and
        // every read of an export or a destructured import clones them. A
        // `val`'s value is code too, when it is a function; a `var`'s is
        // data, which can be replaced.
        Value::Binding { name, cell } => {
            crate::sync::immortalize(name);
            crate::sync::immortalize(cell);
            if name.kind() == BindingKind::Val {
                immortalize_value(&cell.borrow());
            }
        }
        // An import map, such as `val game = import("slug.games.crawl")`,
        // holds only the bindings of a module's exports: it is that
        // module's table, read on every `game.name`. Other data a `val`
        // holds stays counted.
        Value::Map(map)
            if !map.is_empty()
                && map
                    .iter()
                    .all(|(_, value)| matches!(value, Value::Binding { .. })) =>
        {
            map.immortalize();
        }
        Value::DeclaredNative {
            function,
            declaration,
        } => {
            function.immortalize();
            crate::sync::immortalize(declaration);
        }
        Value::Closure(closure) => {
            crate::sync::immortalize(closure);
            immortalize_program(&closure.program);
            crate::sync::immortalize(&closure.globals);
        }
        Value::Native(function) => function.immortalize(),
        Value::Overloads(values) => values.iter().for_each(immortalize_value),
        _ => {}
    }
}
