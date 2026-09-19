use std::collections::{HashMap, HashSet};

use crate::{Capture, Chunk, Op, SourceSpan, Value};

#[derive(Clone, Debug)]
pub(super) enum Binding {
    Global { mutable: bool },
    Local { slot: usize, mutable: bool },
    Capture { slot: usize, mutable: bool },
    Outer { source: Capture, mutable: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CaptureRequest {
    Direct(Capture),
    ThroughParent(Capture),
}

pub(super) struct State {
    chunk: Chunk,
    scopes: Vec<HashMap<String, Binding>>,
    callables: Vec<HashSet<String>>,
    outer: HashMap<String, Binding>,
    captures: Vec<CaptureRequest>,
    root: bool,
    next_local: usize,
}
impl State {
    pub(super) fn root() -> Self {
        Self {
            chunk: Chunk::new("main", 0),
            scopes: vec![HashMap::new()],
            callables: vec![HashSet::new()],
            outer: HashMap::new(),
            captures: Vec::new(),
            root: true,
            next_local: 0,
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
            outer,
            captures: Vec::new(),
            root: false,
            next_local: parameters.len(),
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
        self.chunk.emit_at(op, span.clone());
    }
    pub(super) fn constant(&mut self, value: Value) -> usize {
        self.chunk.constant(value)
    }
    pub(super) fn jump_if_false(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::JumpIfFalse(usize::MAX), span);
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
    pub(super) fn jump(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::Jump(usize::MAX), span);
        index
    }
    pub(super) fn patch(&mut self, instruction: usize) {
        let target = self.chunk.code.len();
        match &mut self.chunk.code[instruction].op {
            Op::Jump(value) | Op::JumpIfFalse(value) | Op::JumpIfProvided { target: value, .. } => {
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
    }
    pub(super) fn leave_scope(&mut self) {
        self.scopes.pop();
        self.callables.pop();
    }
    pub(super) fn declare(&mut self, name: String, mutable: bool, callable: bool) -> usize {
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
        let binding = self.outer.get(name).cloned()?;
        match binding {
            Binding::Global { .. } | Binding::Capture { .. } => Some(binding),
            Binding::Local { slot, mutable } => {
                let slot = self.request_capture(CaptureRequest::Direct(Capture::Local(slot)));
                let binding = Binding::Capture { slot, mutable };
                self.outer.insert(name.into(), binding.clone());
                Some(binding)
            }
            Binding::Outer { source, mutable } => {
                let slot = self.request_capture(CaptureRequest::ThroughParent(source));
                let binding = Binding::Capture { slot, mutable };
                self.outer.insert(name.into(), binding.clone());
                Some(binding)
            }
        }
    }
    pub(super) fn visible(&self) -> HashMap<String, Binding> {
        let mut result: HashMap<String, Binding> = self
            .outer
            .iter()
            .map(|(name, binding)| {
                let binding = match binding {
                    Binding::Global { mutable } => Binding::Global { mutable: *mutable },
                    Binding::Local { slot, mutable } => Binding::Outer {
                        source: Capture::Local(*slot),
                        mutable: *mutable,
                    },
                    Binding::Capture { slot, mutable } => Binding::Capture {
                        slot: *slot,
                        mutable: *mutable,
                    },
                    Binding::Outer { source, mutable } => Binding::Outer {
                        source: source.clone(),
                        mutable: *mutable,
                    },
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
                CaptureRequest::ThroughParent(capture) => {
                    Capture::Capture(self.request_capture(CaptureRequest::Direct(capture)))
                }
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
        if let Some(CaptureRequest::Direct(source)) = self.captures.last() {
            for binding in self.outer.values_mut() {
                if let Binding::Outer {
                    source: outer_source,
                    mutable,
                } = binding
                    && outer_source == source
                {
                    *binding = Binding::Capture {
                        slot,
                        mutable: *mutable,
                    };
                }
            }
        }
        slot
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, State};
    use crate::Capture;

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
    fn does_not_capture_an_unused_outer_binding() {
        let mut root = State::root();
        root.declare("outer".into(), true, false);

        let child = State::function(&[], root.visible());
        assert!(root.resolve_child_captures(child.captures()).is_empty());
    }
}
