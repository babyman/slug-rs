use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::SourceSpan;

use super::semantic::{EnumIdentity, ResourceIdentity, SchemaIdentity, Type};

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

    pub(super) fn identity(&self) -> CallableIdentity {
        CallableIdentity {
            generic_arity: self.generic_arity,
            parameters: self.parameters.clone(),
        }
    }
}

/// Resource positions retained for mandatory runtime validation of a `foreign`
/// declaration. Other source types remain compile-time-only in this subset.
#[derive(Clone, Debug, Default)]
#[doc(hidden)]
pub struct ForeignResourceSignature {
    parameters: Vec<Option<ResourceIdentity>>,
    result: Option<ResourceIdentity>,
}

impl ForeignResourceSignature {
    pub(super) fn from_callable(signature: &CallableSignature) -> Self {
        Self {
            parameters: signature
                .parameters
                .iter()
                .map(|parameter| match &parameter.value_type {
                    Type::Resource(identity) => Some(identity.clone()),
                    _ => None,
                })
                .collect(),
            result: match &signature.result {
                Type::Resource(identity) => Some(identity.clone()),
                _ => None,
            },
        }
    }

    pub(crate) fn parameter_identity(&self, index: usize) -> Option<(Option<&str>, &str)> {
        self.parameters
            .get(index)
            .and_then(Option::as_ref)
            .map(|identity| (identity.explicit_runtime_module(), identity.name.as_str()))
    }

    pub(crate) fn result_identity(&self) -> Option<(Option<&str>, &str)> {
        self.result
            .as_ref()
            .map(|identity| (identity.explicit_runtime_module(), identity.name.as_str()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Opaque canonical input identity used by private callable dispatch metadata.
pub struct CallableIdentity {
    generic_arity: usize,
    parameters: Vec<CallableParameter>,
}

#[derive(Clone, Debug)]
pub(super) struct SemanticBinding {
    pub(super) value_type: Type,
    pub(super) callables: Vec<CallableSignature>,
    pub(super) members: HashMap<String, SemanticBinding>,
    pub(super) required_fields: HashSet<String>,
    pub(super) schema_identity: Option<SchemaIdentity>,
    pub(super) resource_identity: Option<ResourceIdentity>,
    pub(super) type_members: HashMap<String, TypeMember>,
    pub(super) module_binding: bool,
}

#[derive(Clone, Debug)]
pub(super) enum TypeMember {
    Resource(ResourceIdentity),
    Enum {
        identity: EnumIdentity,
        cases: Vec<String>,
    },
    Alias(Type),
}

impl TypeMember {
    pub(super) fn with_resource_runtime_module(&self, module: &str) -> Self {
        match self {
            Self::Resource(identity) => {
                let mut identity = identity.clone();
                identity.set_runtime_module(module.into());
                Self::Resource(identity)
            }
            Self::Enum { identity, cases } => {
                let mut identity = identity.clone();
                identity.set_runtime_module(module.into());
                Self::Enum {
                    identity,
                    cases: cases.clone(),
                }
            }
            Self::Alias(value_type) => Self::Alias(value_type.clone()),
        }
    }
}

impl SemanticBinding {
    pub(super) fn value(value_type: Type) -> Self {
        Self {
            value_type,
            callables: Vec::new(),
            members: HashMap::new(),
            required_fields: HashSet::new(),
            schema_identity: None,
            resource_identity: None,
            type_members: HashMap::new(),
            module_binding: false,
        }
    }

    pub(super) fn callable(signature: CallableSignature) -> Self {
        Self {
            value_type: function_value_type(&signature),
            callables: vec![signature],
            members: HashMap::new(),
            required_fields: HashSet::new(),
            schema_identity: None,
            resource_identity: None,
            type_members: HashMap::new(),
            module_binding: false,
        }
    }

    pub(super) fn module(
        members: HashMap<String, SemanticBinding>,
        type_members: HashMap<String, TypeMember>,
    ) -> Self {
        Self {
            value_type: Type::Map(None),
            callables: Vec::new(),
            members,
            required_fields: HashSet::new(),
            schema_identity: None,
            resource_identity: None,
            type_members,
            module_binding: true,
        }
    }

    pub(super) fn set_resource_runtime_module(&mut self, module: &str) {
        if let Some(identity) = &mut self.resource_identity {
            identity.set_runtime_module(module.into());
        }
        for member in self.members.values_mut() {
            member.set_resource_runtime_module(module);
        }
    }
}

pub(super) fn function_value_type(signature: &CallableSignature) -> Type {
    Type::Function(Some(
        std::iter::once(signature.result.clone())
            .chain(
                signature
                    .parameters
                    .iter()
                    .map(|parameter| parameter.value_type.clone()),
            )
            .collect(),
    ))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ModuleSnapshot {
    pub(super) exports: HashMap<String, SemanticBinding>,
    pub(super) types: HashMap<String, TypeMember>,
}

pub(super) type ImportSnapshots = HashMap<String, ModuleSnapshot>;

#[derive(Clone, Debug, Default)]
pub(super) struct SemanticAnalysis {
    pub(super) snapshot: ModuleSnapshot,
    pub(super) selected_calls: HashMap<SourceSpan, CallableIdentity>,
    pub(super) function_identities: HashMap<SourceSpan, CallableIdentity>,
    pub(super) foreign_identities: HashMap<SourceSpan, CallableIdentity>,
    pub(crate) foreign_resource_signatures: HashMap<SourceSpan, ForeignResourceSignature>,
    pub(super) match_constraints: HashMap<SourceSpan, Vec<Option<Type>>>,
}

#[derive(Clone, Debug, Default)]
struct SemanticRecords {
    selected_calls: HashMap<SourceSpan, CallableIdentity>,
    function_identities: HashMap<SourceSpan, CallableIdentity>,
    foreign_identities: HashMap<SourceSpan, CallableIdentity>,
    foreign_resource_signatures: HashMap<SourceSpan, ForeignResourceSignature>,
    match_constraints: HashMap<SourceSpan, Vec<Option<Type>>>,
}

#[derive(Clone, Debug)]
pub(super) struct Environment {
    scopes: Vec<HashMap<String, SemanticBinding>>,
    type_scopes: Vec<HashMap<String, TypeMember>>,
    imports: Rc<ImportSnapshots>,
    records: Rc<RefCell<SemanticRecords>>,
}

impl Environment {
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_imports(HashMap::new())
    }

    pub(super) fn with_imports(imports: ImportSnapshots) -> Self {
        Self {
            scopes: vec![HashMap::new()],
            type_scopes: vec![HashMap::new()],
            imports: Rc::new(imports),
            records: Rc::new(RefCell::new(SemanticRecords::default())),
        }
    }

    pub(super) fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.type_scopes.push(HashMap::new());
    }

    #[cfg(test)]
    pub(super) fn leave_scope(&mut self) {
        debug_assert!(self.scopes.len() > 1);
        self.scopes.pop();
        self.type_scopes.pop();
    }

    pub(super) fn declare(&mut self, name: String, binding: SemanticBinding) {
        self.scopes
            .last_mut()
            .expect("a semantic environment always has a scope")
            .insert(name, binding);
    }

    pub(super) fn declare_type(&mut self, name: String, member: TypeMember) {
        self.type_scopes
            .last_mut()
            .expect("a semantic environment always has a type scope")
            .insert(name, member);
    }

    pub(super) fn declare_callable(
        &mut self,
        name: String,
        signature: CallableSignature,
        span: &SourceSpan,
    ) -> Result<(), super::SourceError> {
        let scope = self
            .scopes
            .last_mut()
            .expect("a semantic environment always has a scope");
        let Some(existing) = scope.get_mut(&name) else {
            scope.insert(name, SemanticBinding::callable(signature));
            return Ok(());
        };
        if existing.callables.is_empty() {
            *existing = SemanticBinding::callable(signature);
            return Ok(());
        }
        if existing
            .callables
            .iter()
            .any(|candidate| candidate.has_same_input(&signature))
        {
            return Err(super::SourceError::semantic(
                format!("duplicate callable signature for `{name}`"),
                span.clone(),
            ));
        }
        existing.callables.push(signature);
        Ok(())
    }

    pub(super) fn update_callable_result(
        &mut self,
        name: &str,
        identity: &CallableIdentity,
        result: Type,
    ) {
        let Some(binding) = self.scopes.last_mut().and_then(|scope| scope.get_mut(name)) else {
            return;
        };
        if let Some(signature) = binding
            .callables
            .iter_mut()
            .find(|signature| signature.identity() == *identity)
        {
            signature.result = result;
            binding.value_type = function_value_type(signature);
        }
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

    pub(super) fn schema_by_identity(&self, identity: &SchemaIdentity) -> Option<SemanticBinding> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.values())
            .find_map(|binding| find_schema_binding(binding, identity))
    }

    pub(super) fn resource_type(&self, name: &str) -> Option<ResourceIdentity> {
        if let Some((module, type_name)) = name.split_once('.') {
            return self
                .lookup(module)
                .filter(|binding| binding.module_binding)
                .and_then(|binding| binding.type_members.get(type_name))
                .and_then(|member| match member {
                    TypeMember::Resource(identity) => Some(identity.clone()),
                    TypeMember::Enum { .. } | TypeMember::Alias(_) => None,
                });
        }
        self.type_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .and_then(|member| match member {
                TypeMember::Resource(identity) => Some(identity.clone()),
                TypeMember::Enum { .. } | TypeMember::Alias(_) => None,
            })
    }

    pub(super) fn type_member(&self, name: &str) -> Option<&TypeMember> {
        if let Some((module, type_name)) = name.split_once('.') {
            return self
                .lookup(module)
                .filter(|binding| binding.module_binding)
                .and_then(|binding| binding.type_members.get(type_name));
        }
        self.type_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    pub(super) fn enum_cases(&self, identity: &EnumIdentity) -> Option<Vec<String>> {
        self.type_scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.values())
            .chain(
                self.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.values())
                    .flat_map(|binding| binding.type_members.values()),
            )
            .find_map(|member| match member {
                TypeMember::Enum {
                    identity: candidate,
                    cases,
                } if candidate == identity => Some(cases.clone()),
                TypeMember::Resource(_) | TypeMember::Enum { .. } | TypeMember::Alias(_) => None,
            })
    }

    pub(super) fn resolve_resource_type(
        &self,
        name: &str,
        span: &SourceSpan,
    ) -> Result<ResourceIdentity, super::SourceError> {
        if let Some((module, type_name)) = name.split_once('.') {
            let binding = self.lookup(module).ok_or_else(|| {
                super::SourceError::semantic(
                    format!("unknown module binding `{module}`"),
                    span.clone(),
                )
            })?;
            if !binding.module_binding {
                return Err(super::SourceError::semantic(
                    format!("type prefix `{module}` is not a module binding"),
                    span.clone(),
                ));
            }
            return binding
                .type_members
                .get(type_name)
                .and_then(|member| match member {
                    TypeMember::Resource(identity) => Some(identity.clone()),
                    TypeMember::Enum { .. } | TypeMember::Alias(_) => None,
                })
                .ok_or_else(|| {
                    super::SourceError::semantic(format!("unknown type `{name}`"), span.clone())
                });
        }
        self.resource_type(name).ok_or_else(|| {
            super::SourceError::semantic(format!("unknown type `{name}`"), span.clone())
        })
    }

    pub(super) fn merge_compatible_types(&mut self, left: &Self, right: &Self) {
        for (index, scope) in self.scopes.iter_mut().enumerate() {
            let (Some(left_scope), Some(right_scope)) =
                (left.scopes.get(index), right.scopes.get(index))
            else {
                continue;
            };
            for (name, binding) in scope {
                let (Some(left), Some(right)) = (left_scope.get(name), right_scope.get(name))
                else {
                    continue;
                };
                if left.value_type == right.value_type {
                    binding.value_type = left.value_type.clone();
                }
            }
        }
    }

    pub(super) fn import(&self, name: &str) -> Option<&ModuleSnapshot> {
        self.imports.get(name)
    }

    pub(super) fn record_selected_call(&self, span: SourceSpan, identity: CallableIdentity) {
        self.records
            .borrow_mut()
            .selected_calls
            .insert(span, identity);
    }

    pub(super) fn record_function(&self, span: SourceSpan, identity: CallableIdentity) {
        self.records
            .borrow_mut()
            .function_identities
            .insert(span, identity);
    }

    pub(super) fn record_foreign(
        &self,
        span: SourceSpan,
        identity: CallableIdentity,
        resource_signature: ForeignResourceSignature,
    ) {
        let mut records = self.records.borrow_mut();
        records.foreign_identities.insert(span.clone(), identity);
        records
            .foreign_resource_signatures
            .insert(span, resource_signature);
    }

    pub(super) fn record_match_constraints(
        &self,
        span: SourceSpan,
        constraints: Vec<Option<Type>>,
    ) {
        self.records
            .borrow_mut()
            .match_constraints
            .insert(span, constraints);
    }

    pub(super) fn analysis(&self, snapshot: ModuleSnapshot) -> SemanticAnalysis {
        let records = self.records.borrow();
        SemanticAnalysis {
            snapshot,
            selected_calls: records.selected_calls.clone(),
            function_identities: records.function_identities.clone(),
            foreign_identities: records.foreign_identities.clone(),
            foreign_resource_signatures: records.foreign_resource_signatures.clone(),
            match_constraints: records.match_constraints.clone(),
        }
    }
}

fn find_schema_binding(
    binding: &SemanticBinding,
    identity: &SchemaIdentity,
) -> Option<SemanticBinding> {
    if binding
        .schema_identity
        .as_ref()
        .is_some_and(|schema| schema == identity)
    {
        return Some(binding.clone());
    }
    binding
        .members
        .values()
        .find_map(|member| find_schema_binding(member, identity))
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
