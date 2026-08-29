use std::{collections::HashMap, rc::Rc};

use super::semantic::Type;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CallableParameter {
    pub(super) label: Option<String>,
    pub(super) value_type: Type,
    pub(super) has_default: bool,
    pub(super) variadic: bool,
}

#[derive(Clone, Debug)]
pub(super) struct CallableSignature {
    pub(super) generic_arity: usize,
    pub(super) parameters: Vec<CallableParameter>,
    pub(super) result: Type,
}

impl CallableSignature {
    pub(super) fn has_same_input(&self, other: &Self) -> bool {
        self.generic_arity == other.generic_arity && self.parameters == other.parameters
    }

    pub(super) fn has_same_runtime_shape(&self, other: &Self) -> bool {
        self.parameters
            .iter()
            .map(|parameter| (parameter.has_default, parameter.variadic))
            .eq(other
                .parameters
                .iter()
                .map(|parameter| (parameter.has_default, parameter.variadic)))
    }
}

#[derive(Clone, Debug)]
pub(super) struct SemanticBinding {
    pub(super) value_type: Type,
    pub(super) callables: Vec<CallableSignature>,
    pub(super) members: HashMap<String, SemanticBinding>,
}

impl SemanticBinding {
    pub(super) fn value(value_type: Type) -> Self {
        Self {
            value_type,
            callables: Vec::new(),
            members: HashMap::new(),
        }
    }

    pub(super) fn callable(signature: CallableSignature) -> Self {
        Self {
            value_type: Type::Function(None),
            callables: vec![signature],
            members: HashMap::new(),
        }
    }

    pub(super) fn module(members: HashMap<String, SemanticBinding>) -> Self {
        Self {
            value_type: Type::Map(None),
            callables: Vec::new(),
            members,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ModuleSnapshot {
    pub(super) exports: HashMap<String, SemanticBinding>,
}

pub(super) type ImportSnapshots = HashMap<String, ModuleSnapshot>;

#[derive(Clone, Debug)]
pub(super) struct Environment {
    scopes: Vec<HashMap<String, SemanticBinding>>,
    imports: Rc<ImportSnapshots>,
}

impl Environment {
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_imports(HashMap::new())
    }

    pub(super) fn with_imports(imports: ImportSnapshots) -> Self {
        Self {
            scopes: vec![HashMap::new()],
            imports: Rc::new(imports),
        }
    }

    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    #[cfg(test)]
    pub(super) fn leave_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1);
        self.scopes.pop();
    }

    pub(super) fn declare(&mut self, name: String, binding: SemanticBinding) {
        self.scopes
            .last_mut()
            .expect("a semantic environment always has a scope")
            .insert(name, binding);
    }

    pub(super) fn lookup(&self, name: &str) -> Option<&SemanticBinding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    pub(super) fn lookup_mut(&mut self, name: &str) -> Option<&mut SemanticBinding> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
    }

    pub(super) fn import(&self, name: &str) -> Option<&ModuleSnapshot> {
        self.imports.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(input: Type, result: Type) -> CallableSignature {
        CallableSignature {
            generic_arity: 0,
            parameters: vec![CallableParameter {
                label: Some("value".into()),
                value_type: input,
                has_default: false,
                variadic: false,
            }],
            result,
        }
    }

    #[test]
    fn lexical_bindings_shadow_and_restore_callable_sets() {
        let mut environment = Environment::new();
        environment.declare(
            "render".into(),
            SemanticBinding::callable(signature(Type::Str, Type::Str)),
        );
        environment.enter_scope();
        environment.declare("render".into(), SemanticBinding::value(Type::Num));
        assert!(environment.lookup("render").unwrap().callables.is_empty());
        environment.leave_scope();
        assert_eq!(environment.lookup("render").unwrap().callables.len(), 1);
    }

    #[test]
    fn result_types_do_not_participate_in_callable_identity() {
        let left = signature(Type::Str, Type::Str);
        let right = signature(Type::Str, Type::Num);
        assert!(left.has_same_input(&right));
    }
}
