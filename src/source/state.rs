use std::collections::HashMap;

use crate::{Capture, Chunk, Op, SourceSpan};

#[derive(Clone, Debug)]
pub(super) enum Binding {
    Global { mutable: bool },
    Local { slot: usize, mutable: bool },
    Capture { slot: usize, mutable: bool },
}
pub(super) struct State {
    pub(super) chunk: Chunk,
    pub(super) scopes: Vec<HashMap<String, Binding>>,
    pub(super) outer: HashMap<String, Binding>,
    pub(super) captures: Vec<Capture>,
    pub(super) root: bool,
    pub(super) next_local: usize,
}
impl State {
    pub(super) fn root() -> Self {
        Self {
            chunk: Chunk::new("main", 0),
            scopes: vec![HashMap::new()],
            outer: HashMap::new(),
            captures: Vec::new(),
            root: true,
            next_local: 0,
        }
    }
    pub(super) fn function(parameters: &[String], visible: HashMap<String, Binding>) -> Self {
        let mut outer = HashMap::new();
        let mut captures = Vec::new();
        for (name, binding) in visible {
            match binding {
                Binding::Global { mutable } => {
                    outer.insert(name, Binding::Global { mutable });
                }
                Binding::Local { slot, mutable } => {
                    let capture = captures.len();
                    captures.push(Capture::Local(slot));
                    outer.insert(
                        name,
                        Binding::Capture {
                            slot: capture,
                            mutable,
                        },
                    );
                }
                Binding::Capture { slot, mutable } => {
                    let capture = captures.len();
                    captures.push(Capture::Capture(slot));
                    outer.insert(
                        name,
                        Binding::Capture {
                            slot: capture,
                            mutable,
                        },
                    );
                }
            }
        }
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
            outer,
            captures,
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
    pub(super) fn emit(&mut self, op: Op, span: &SourceSpan) {
        self.chunk.emit_at(op, span.clone());
    }
    pub(super) fn jump_if_false(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::JumpIfFalse(usize::MAX), span);
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
            Op::Jump(value) | Op::JumpIfFalse(value) => *value = target,
            _ => unreachable!("only jump instructions are patched"),
        }
    }
    pub(super) fn is_root(&self) -> bool {
        self.root && self.scopes.len() == 1
    }
    pub(super) fn allows_return(&self) -> bool {
        !self.root
    }
    pub(super) fn arity(&self) -> usize {
        self.chunk.arity
    }
    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    pub(super) fn leave_scope(&mut self) {
        self.scopes.pop();
    }
    pub(super) fn declare(&mut self, name: String, mutable: bool) -> usize {
        let slot = self.next_local;
        self.next_local += 1;
        self.scopes
            .last_mut()
            .expect("a compiler state always has a scope")
            .insert(name, Binding::Local { slot, mutable });
        slot
    }
    pub(super) fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .or_else(|| self.outer.get(name).cloned())
    }
    pub(super) fn visible(&self) -> HashMap<String, Binding> {
        let mut result = self.outer.clone();
        for scope in &self.scopes {
            result.extend(scope.clone());
        }
        result
    }
}
