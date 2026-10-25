use std::collections::{HashMap, HashSet};

use slug_vm::{BinaryOperator, Capture, Chunk, Constant, MatchPattern, Op, SourceSpan, Value};

/// Where a name lives, as seen by one function. In `State::outer`, a
/// `Local` is a slot of the enclosing function and a `Capture` is a slot of
/// this function's own captures.
#[derive(Clone, Debug)]
pub(super) enum Binding {
    Global {
        mutable: bool,
    },
    Local {
        slot: usize,
        mutable: bool,
    },
    Capture {
        slot: usize,
        mutable: bool,
    },
    /// A name the enclosing function itself sees from further out. The
    /// enclosing function resolves it, capturing it in turn, when this one
    /// is finished.
    Inherited {
        mutable: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CaptureRequest {
    /// A capture of the enclosing function's local or capture slot.
    Direct(Capture),
    /// A name the enclosing function must capture from further out.
    ThroughParent(String),
}

pub(super) struct State {
    chunk: Chunk,
    scopes: Vec<HashMap<String, Binding>>,
    callables: Vec<HashSet<String>>,
    /// Per scope, the local functions whose slots were declared when the
    /// block began and are not yet claimed by their declarations.
    hoisted: Vec<HashSet<String>>,
    /// Per scope, the other names the block declares later; and those of the
    /// enclosing functions' blocks. Such a name cannot be used yet.
    later: Vec<HashSet<String>>,
    outer_later: HashSet<String>,
    outer: HashMap<String, Binding>,
    captures: Vec<CaptureRequest>,
    root: bool,
    next_local: usize,
    /// The latest instruction index a patched jump lands on; fusion must not
    /// remove an instruction at or after it.
    last_jump_target: usize,
}
impl State {
    pub(super) fn root() -> Self {
        Self {
            chunk: Chunk::new("main", 0),
            scopes: vec![HashMap::new()],
            callables: vec![HashSet::new()],
            hoisted: vec![HashSet::new()],
            later: vec![HashSet::new()],
            outer_later: HashSet::new(),
            outer: HashMap::new(),
            captures: Vec::new(),
            root: true,
            next_local: 0,
            last_jump_target: 0,
        }
    }
    pub(super) fn function(parameters: &[String], outer: HashMap<String, Binding>) -> Self {
        let mut parameters_scope = HashMap::new();
        for (slot, name) in parameters.iter().enumerate() {
            parameters_scope.insert(
                name.clone(),
                Binding::Local {
                    slot,
                    mutable: false,
                },
            );
        }
        Self {
            chunk: Chunk::new("<fn>", parameters.len()),
            scopes: vec![parameters_scope],
            callables: vec![HashSet::new()],
            hoisted: vec![HashSet::new()],
            later: vec![HashSet::new()],
            outer_later: HashSet::new(),
            outer,
            captures: Vec::new(),
            root: false,
            next_local: parameters.len(),
            last_jump_target: 0,
        }
    }
    pub(super) fn finish(mut self, name: &str, arity: usize) -> Chunk {
        self.chunk.name = name.into();
        self.chunk.arity = arity;
        self.chunk.locals = self.next_local;
        self.chunk
    }
    pub(super) fn is_module_scope(&self) -> bool {
        self.root && self.scopes.len() == 1
    }
    pub(super) fn emit(&mut self, op: Op, span: &SourceSpan) {
        if let Some(operator) = BinaryOperator::of(&op)
            && self.fuse_local_constant(operator, span)
        {
            return;
        }
        if matches!(op, Op::Pop) && self.drop_trailing_nil() {
            return;
        }
        self.chunk.emit_at(op, span.clone());
    }
    /// Removes a trailing `Nil` that a `Pop` would discard at once, the
    /// value of a declaration or `defer` in statement position. A jump to the
    /// `Pop` would arrive with a different value to discard, so none may
    /// land there; a jump to the `Nil` lands on whatever follows instead.
    fn drop_trailing_nil(&mut self) -> bool {
        let end = self.chunk.code.len();
        if self.last_jump_target >= end
            || !matches!(
                self.chunk.code.last().map(|instruction| &instruction.op),
                Some(Op::Nil)
            )
        {
            return false;
        }
        self.chunk.code.truncate(end - 1);
        true
    }
    /// Replaces a trailing `GetLocal`, `Constant` pair with one
    /// `LocalConstantBinary` applying `operator`, the shape of `x - 1` or
    /// `n == 0`. It skips the operand stack and two dispatches. No jump may
    /// land on the constant or the operator, or it would skip the local.
    fn fuse_local_constant(&mut self, operator: BinaryOperator, span: &SourceSpan) -> bool {
        let Some(start) = self.chunk.code.len().checked_sub(2) else {
            return false;
        };
        if self.last_jump_target > start {
            return false;
        }
        let (Op::GetLocal(slot), Op::Constant(constant)) =
            (&self.chunk.code[start].op, &self.chunk.code[start + 1].op)
        else {
            return false;
        };
        if !matches!(
            self.chunk.constants.get(*constant),
            Some(Constant::Value(_))
        ) {
            return false;
        }
        let fused = Op::LocalConstantBinary {
            slot: *slot,
            constant: *constant,
            operator,
        };
        self.chunk.code.truncate(start);
        self.chunk.emit_at(fused, span.clone());
        true
    }
    pub(super) fn constant(&mut self, value: Value) -> usize {
        self.chunk.constant(value)
    }
    pub(super) fn jump_if_false(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::JumpIfFalse(usize::MAX), span);
        index
    }
    /// Emits a jump that consumes its condition, so neither branch pops it.
    pub(super) fn jump_if_false_pop(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::JumpIfFalsePop(usize::MAX), span);
        index
    }
    pub(super) fn jump_if_provided(&mut self, slot: usize, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(
            Op::JumpIfProvided {
                slot,
                target: usize::MAX,
            },
            span,
        );
        index
    }
    /// Emits a match of the value under `operands` that leaves the value on
    /// the stack, pushes its bindings on success, and jumps on failure.
    pub(super) fn match_case(
        &mut self,
        pattern: MatchPattern,
        operands: usize,
        span: &SourceSpan,
    ) -> usize {
        let index = self.chunk.code.len();
        self.emit(
            Op::MatchCase {
                pattern,
                operands,
                target: usize::MAX,
            },
            span,
        );
        index
    }
    pub(super) fn jump(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::Jump(usize::MAX), span);
        index
    }
    pub(super) fn patch(&mut self, instruction: usize) {
        let target = self.chunk.code.len();
        self.last_jump_target = self.last_jump_target.max(target);
        match &mut self.chunk.code[instruction].op {
            Op::Jump(value)
            | Op::JumpIfFalse(value)
            | Op::JumpIfFalsePop(value)
            | Op::JumpIfProvided { target: value, .. }
            | Op::MatchCase { target: value, .. } => {
                *value = target;
            }
            _ => unreachable!("only jump instructions are patched"),
        }
    }
    pub(super) fn is_root(&self) -> bool {
        self.root && self.scopes.len() == 1
    }
    pub(super) fn allows_return(&self) -> bool {
        !self.root
    }
    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.callables.push(HashSet::new());
        self.hoisted.push(HashSet::new());
        self.later.push(HashSet::new());
    }
    pub(super) fn leave_scope(&mut self) {
        self.scopes.pop();
        self.callables.pop();
        self.hoisted.pop();
        self.later.pop();
    }
    /// Records a name the current block declares later, other than a hoisted
    /// function.
    pub(super) fn declare_later(&mut self, name: &str) {
        self.later
            .last_mut()
            .expect("a compiler state always has a scope")
            .insert(name.into());
    }
    /// Whether `name` is declared later in an enclosing block, here or in an
    /// enclosing function, and so cannot be used yet.
    pub(super) fn declared_later(&self, name: &str) -> bool {
        self.later.iter().any(|later| later.contains(name)) || self.outer_later.contains(name)
    }
    /// The names a nested function cannot use yet.
    pub(super) fn inherit_later(&mut self, parent: &State) {
        self.outer_later = parent
            .later
            .iter()
            .flatten()
            .chain(&parent.outer_later)
            .cloned()
            .collect();
    }
    /// Declares a local function's slot when its block begins, so functions
    /// declared earlier in the block can call it; its declaration claims it.
    pub(super) fn hoist(&mut self, name: &str, mutable: bool) {
        let hoisted = self
            .hoisted
            .last_mut()
            .expect("a compiler state always has a scope");
        if hoisted.insert(name.into()) {
            self.declare(name.into(), mutable, false);
        }
    }
    /// Whether `name`, read in this function, is a local function hoisted in
    /// an enclosing scope of it whose declaration has not run yet. A nested
    /// function may refer to it, since that runs later, but a direct use
    /// would find no function.
    pub(super) fn before_declaration(&self, name: &str) -> bool {
        for (scope, hoisted) in self.scopes.iter().zip(&self.hoisted).rev() {
            if scope.contains_key(name) {
                return hoisted.contains(name);
            }
        }
        false
    }
    /// The slot hoisted for `name` in this scope, now claimed by its
    /// declaration as a callable binding.
    pub(super) fn claim_hoisted(&mut self, name: &str) -> Option<usize> {
        if !self.hoisted.last_mut()?.remove(name) {
            return None;
        }
        let Some(Binding::Local { slot, .. }) = self.scopes.last()?.get(name) else {
            return None;
        };
        let slot = *slot;
        self.callables
            .last_mut()
            .expect("a compiler state always has a callable scope")
            .insert(name.into());
        Some(slot)
    }
    pub(super) fn declare(&mut self, name: String, mutable: bool, callable: bool) -> usize {
        if let Some(later) = self.later.last_mut() {
            later.remove(&name);
        }
        let slot = self.next_local;
        self.next_local += 1;
        self.scopes
            .last_mut()
            .expect("a compiler state always has a scope")
            .insert(name.clone(), Binding::Local { slot, mutable });
        if callable {
            self.callables
                .last_mut()
                .expect("a compiler state always has a callable scope")
                .insert(name);
        } else {
            self.callables
                .last_mut()
                .expect("a compiler state always has a callable scope")
                .remove(&name);
        }
        slot
    }
    pub(super) fn current_callable(&self, name: &str) -> Option<Binding> {
        self.callables.last()?.contains(name).then(|| {
            self.scopes
                .last()
                .and_then(|scope| scope.get(name))
                .cloned()
                .expect("callable names have a local binding")
        })
    }
    pub(super) fn lookup(&mut self, name: &str) -> Option<Binding> {
        if let Some(binding) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
        {
            return Some(binding);
        }
        self.lookup_outer(name)
    }
    /// Resolves a name from outside this function, capturing it on first use.
    fn lookup_outer(&mut self, name: &str) -> Option<Binding> {
        let binding = self.outer.get(name).cloned()?;
        let (request, mutable) = match binding {
            Binding::Global { .. } | Binding::Capture { .. } => return Some(binding),
            Binding::Local { slot, mutable } => {
                (CaptureRequest::Direct(Capture::Local(slot)), mutable)
            }
            Binding::Inherited { mutable } => (CaptureRequest::ThroughParent(name.into()), mutable),
        };
        let slot = self.request_capture(request);
        let binding = Binding::Capture { slot, mutable };
        self.outer.insert(name.into(), binding.clone());
        Some(binding)
    }
    /// The names a function nested here sees, as that function sees them.
    pub(super) fn visible(&self) -> HashMap<String, Binding> {
        let mut result: HashMap<String, Binding> = self
            .outer
            .iter()
            .map(|(name, binding)| {
                let binding = match binding {
                    Binding::Global { mutable } => Binding::Global { mutable: *mutable },
                    Binding::Local { mutable, .. }
                    | Binding::Capture { mutable, .. }
                    | Binding::Inherited { mutable } => Binding::Inherited { mutable: *mutable },
                };
                (name.clone(), binding)
            })
            .collect();
        for scope in &self.scopes {
            result.extend(scope.clone());
        }
        result
    }
    pub(super) fn resolve_child_captures(&mut self, captures: Vec<CaptureRequest>) -> Vec<Capture> {
        captures
            .into_iter()
            .map(|capture| match capture {
                CaptureRequest::Direct(capture) => capture,
                CaptureRequest::ThroughParent(name) => match self.lookup_outer(&name) {
                    Some(Binding::Capture { slot, .. }) => Capture::Capture(slot),
                    other => unreachable!("inherited name `{name}` resolved to {other:?}"),
                },
            })
            .collect()
    }
    pub(super) fn captures(&self) -> Vec<CaptureRequest> {
        self.captures.clone()
    }

    fn request_capture(&mut self, capture: CaptureRequest) -> usize {
        if let Some(slot) = self
            .captures
            .iter()
            .position(|existing| existing == &capture)
        {
            return slot;
        }
        let slot = self.captures.len();
        self.captures.push(capture);
        slot
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, State};
    use slug_vm::Capture;

    #[test]
    fn captures_only_referenced_outer_bindings_through_intermediate_functions() {
        let mut root = State::root();
        root.declare("outer".into(), true, false);

        let mut middle = State::function(&[], root.visible());
        let mut inner = State::function(&[], middle.visible());
        assert!(matches!(
            inner.lookup("outer"),
            Some(Binding::Capture { .. })
        ));

        let inner_captures = middle.resolve_child_captures(inner.captures());
        assert_eq!(inner_captures, vec![Capture::Capture(0)]);
        assert!(matches!(
            middle.lookup("outer"),
            Some(Binding::Capture { slot: 0, .. })
        ));
        let middle_captures = root.resolve_child_captures(middle.captures());
        assert_eq!(middle_captures, vec![Capture::Local(0)]);
    }

    #[test]
    fn a_child_captures_its_parents_capture_rather_than_reusing_its_slot() {
        let mut root = State::root();
        root.declare("value".into(), false, false);

        let mut parent = State::function(&[], root.visible());
        assert!(matches!(
            parent.lookup("value"),
            Some(Binding::Capture { slot: 0, .. })
        ));
        let mut child = State::function(&[], parent.visible());
        assert!(matches!(
            child.lookup("value"),
            Some(Binding::Capture { slot: 0, .. })
        ));
        assert_eq!(
            parent.resolve_child_captures(child.captures()),
            vec![Capture::Capture(0)]
        );
        assert_eq!(parent.captures().len(), 1);
    }

    #[test]
    fn does_not_capture_an_unused_outer_binding() {
        let mut root = State::root();
        root.declare("outer".into(), true, false);

        let child = State::function(&[], root.visible());
        assert!(root.resolve_child_captures(child.captures()).is_empty());
    }
}
