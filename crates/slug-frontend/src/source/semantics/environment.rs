use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use slug_vm::{CallableIdentity, ForeignResourceSignature, SourceSpan};

use super::semantic::{EnumIdentity, InferredAlternative, ResourceIdentity, SchemaIdentity, Type};

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
    pub(super) inferred_alternatives: Vec<InferredAlternative>,
}

impl CallableSignature {
    pub(super) fn has_same_input(&self, other: &Self) -> bool {
        self.generic_arity == other.generic_arity
            && self.parameters == other.parameters
            && self.inferred_alternatives == other.inferred_alternatives
    }

    pub(super) fn identity(&self) -> CallableIdentity {
        CallableIdentity::new(format!(
            "{:?}:{:?}:{:?}",
            self.generic_arity, self.parameters, self.inferred_alternatives
        ))
    }
}

pub(super) fn foreign_resource_signature(
    signature: &CallableSignature,
) -> ForeignResourceSignature {
    let resource = |identity: &ResourceIdentity| {
        (
            identity.explicit_runtime_module().map(str::to_owned),
            identity.name.clone(),
        )
    };
    ForeignResourceSignature::new(
        signature
            .parameters
            .iter()
            .map(|parameter| match &parameter.value_type {
                Type::Resource(identity) => Some(resource(identity)),
                _ => None,
            })
            .collect(),
        match &signature.result {
            Type::Resource(identity) => Some(resource(identity)),
            _ => None,
        },
    )
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
    /// For a `var`, the type every assignment must conform to, fixed at its
    /// declaration. `value_type` is then its current type, which assignments
    /// and nil tests refine for the code that follows.
    pub(super) declared: Option<Type>,
    /// For a `var` whose first value is `nil`, where it was declared and the
    /// assignments its declared type was inferred from.
    pub(super) nil_start: Option<NilStart>,
    /// An implicit builtin, such as `close`, which a declaration of the same
    /// name replaces rather than overloads, as it does at runtime.
    pub(super) implicit: bool,
    /// For an immutable binding, the narrowed types of string-keyed members a
    /// nil test established on this path, such as `r.error` after
    /// `r.error != nil`. The binding never changes, so neither do they.
    pub(super) member_facts: HashMap<String, Type>,
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
            declared: None,
            nil_start: None,
            implicit: false,
            member_facts: HashMap::new(),
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
            declared: None,
            nil_start: None,
            implicit: false,
            member_facts: HashMap::new(),
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
            declared: None,
            nil_start: None,
            implicit: false,
            member_facts: HashMap::new(),
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

impl ModuleSnapshot {
    /// The names of the module's exported values.
    pub(crate) fn export_names(&self) -> impl Iterator<Item = &str> {
        self.exports.keys().map(String::as_str)
    }
}

/// Semantic bindings retained by one interactive source session.
#[derive(Clone, Debug, Default)]
pub(crate) struct SessionSnapshot {
    bindings: HashMap<String, SemanticBinding>,
    types: HashMap<String, TypeMember>,
}

pub(super) type ImportSnapshots = HashMap<String, ModuleSnapshot>;

#[derive(Clone, Debug, Default)]
pub(super) struct SemanticAnalysis {
    pub(super) snapshot: ModuleSnapshot,
    pub(super) session_snapshot: SessionSnapshot,
    pub(super) selected_calls: HashMap<SourceSpan, CallableIdentity>,
    pub(super) function_identities: HashMap<SourceSpan, CallableIdentity>,
    pub(super) foreign_identities: HashMap<SourceSpan, CallableIdentity>,
    pub(crate) foreign_resource_signatures: HashMap<SourceSpan, ForeignResourceSignature>,
    pub(super) match_constraints: HashMap<SourceSpan, Vec<Option<Type>>>,
    pub(super) expression_types: HashMap<SourceSpan, Type>,
    /// The names `{*}` declarations define, or `None` when one selects from
    /// a map whose entries are not known until it runs.
    pub(super) selected_globals: Option<HashSet<String>>,
}

#[derive(Clone, Debug, Default)]
struct SemanticRecords {
    selected_calls: HashMap<SourceSpan, CallableIdentity>,
    function_identities: HashMap<SourceSpan, CallableIdentity>,
    foreign_identities: HashMap<SourceSpan, CallableIdentity>,
    foreign_resource_signatures: HashMap<SourceSpan, ForeignResourceSignature>,
    match_constraints: HashMap<SourceSpan, Vec<Option<Type>>>,
    expression_types: HashMap<SourceSpan, Type>,
    selected_globals: HashSet<String>,
    unknown_selection: bool,
}

/// A `var` whose first value is `nil`. Its declared type is `nil` with the
/// type of its first assignment written in its scope, which a trial check of
/// that scope finds; with none found, or one of unknown type, it is
/// `any|nil`. `assignments` holds that first assignment, if found.
#[derive(Clone, Debug)]
pub(super) struct NilStart {
    pub(super) site: NilStartSite,
    pub(super) assignments: Rc<Vec<(SourceSpan, Type)>>,
}

/// A `nil`-started `var`, by the span of its declaration and its name.
pub(super) type NilStartSite = (SourceSpan, String);

#[derive(Debug, Default)]
struct NilStarts {
    /// Present during a trial check: the assignments seen to each `var`.
    trial: Option<HashMap<NilStartSite, Vec<(SourceSpan, Type)>>>,
    /// The assignments trial checks found, for the real check to declare.
    inferred: HashMap<NilStartSite, Rc<Vec<(SourceSpan, Type)>>>,
}

#[derive(Clone, Debug)]
pub(super) struct Environment {
    scopes: Vec<HashMap<String, SemanticBinding>>,
    type_scopes: Vec<HashMap<String, TypeMember>>,
    imports: Rc<ImportSnapshots>,
    records: Rc<RefCell<SemanticRecords>>,
    /// The function body being checked, shared by every environment cloned
    /// within it, such as a match case's, and replaced inside a nested
    /// function. `None` outside any function.
    function: Option<Rc<RefCell<FunctionReturns>>>,
    nil_starts: Rc<RefCell<NilStarts>>,
}

/// What a function body's `return` expressions must and do produce.
#[derive(Clone, Debug)]
struct FunctionReturns {
    expected: Option<Type>,
    returned: Vec<Type>,
}

impl Environment {
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_imports_and_session(HashMap::new(), &SessionSnapshot::default())
    }

    pub(super) fn with_imports_and_session(
        imports: ImportSnapshots,
        session: &SessionSnapshot,
    ) -> Self {
        Self {
            scopes: vec![session.bindings.clone()],
            type_scopes: vec![session.types.clone()],
            imports: Rc::new(imports),
            records: Rc::new(RefCell::new(SemanticRecords::default())),
            function: None,
            nil_starts: Rc::new(RefCell::new(NilStarts::default())),
        }
    }

    /// A copy for a trial check, sharing nothing it could record into with
    /// this environment, in which `nil`-started vars are dynamic, except
    /// those `seed` gives assignments for, and their assignments are logged.
    pub(super) fn trial(&self, seed: HashMap<NilStartSite, Rc<Vec<(SourceSpan, Type)>>>) -> Self {
        let mut trial = self.clone();
        trial.records = Rc::new(RefCell::new(self.records.borrow().clone()));
        trial.function = self
            .function
            .as_ref()
            .map(|function| Rc::new(RefCell::new(function.borrow().clone())));
        trial.nil_starts = Rc::new(RefCell::new(NilStarts {
            trial: Some(HashMap::new()),
            inferred: seed,
        }));
        trial
    }

    pub(super) fn in_trial(&self) -> bool {
        self.nil_starts.borrow().trial.is_some()
    }

    /// The assignments a finished trial logged.
    pub(super) fn trial_assignments(&self) -> HashMap<NilStartSite, Vec<(SourceSpan, Type)>> {
        self.nil_starts.borrow().trial.clone().unwrap_or_default()
    }

    /// Records the assignments a `var` declared at `site` was found to have.
    pub(super) fn infer_nil_start(&self, site: NilStartSite, assignments: Vec<(SourceSpan, Type)>) {
        self.nil_starts
            .borrow_mut()
            .inferred
            .insert(site, Rc::new(assignments));
    }

    /// Logs, during a trial, an assignment of `value_type` to `name`.
    pub(super) fn log_assignment(&self, name: &str, span: &SourceSpan, value_type: &Type) {
        let Some(site) = self
            .lookup(name)
            .and_then(|binding| binding.nil_start.as_ref())
            .map(|start| start.site.clone())
        else {
            return;
        };
        if let Some(trial) = &mut self.nil_starts.borrow_mut().trial {
            trial
                .entry(site)
                .or_default()
                .push((span.clone(), value_type.clone()));
        }
    }

    /// Begins checking a function body whose result must conform to
    /// `expected`, when it is declared.
    pub(super) fn enter_function(&mut self, expected: Option<Type>) {
        self.function = Some(Rc::new(RefCell::new(FunctionReturns {
            expected,
            returned: Vec::new(),
        })));
    }

    /// The declared result of the function being checked, if any.
    pub(super) fn expected_return(&self) -> Option<Type> {
        self.function
            .as_ref()
            .and_then(|function| function.borrow().expected.clone())
    }

    /// Records the type a `return` in the current function produces.
    pub(super) fn record_return(&self, value_type: Type) {
        if let Some(function) = &self.function {
            function.borrow_mut().returned.push(value_type);
        }
    }

    /// The types every `return` in the current function produced.
    pub(super) fn returned_types(&self) -> Vec<Type> {
        self.function
            .as_ref()
            .map(|function| function.borrow().returned.clone())
            .unwrap_or_default()
    }

    pub(super) fn install_implicit_callable_export(&mut self, module: &ModuleSnapshot, name: &str) {
        let Some(binding) = module
            .exports
            .get(name)
            .filter(|binding| !binding.callables.is_empty())
        else {
            return;
        };
        let mut binding = binding.clone();
        binding.implicit = true;
        self.scopes
            .first_mut()
            .expect("a semantic environment always has a root scope")
            .insert(name.into(), binding);
    }

    pub(super) fn import_snapshot(&self, name: &str) -> Option<ModuleSnapshot> {
        self.imports.get(name).cloned()
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
        if existing.callables.is_empty() || existing.implicit {
            *existing = SemanticBinding::callable(signature);
            return Ok(());
        }
        if existing
            .callables
            .iter()
            .any(|candidate| candidate.has_same_input(&signature))
        {
            return Err(super::SourceError::semantic(
                super::SourceErrorCode::DuplicateSignature,
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

    /// Makes the named binding a `var` whose declared type is its current
    /// one, except that a `nil` initializer declares `nil` with the type of
    /// the first assignment a trial check found, or else `any|nil`: `nil`
    /// there means no value yet, not a value that is always `nil`.
    pub(super) fn declare_mutable(&mut self, name: &str, declaration: &SourceSpan) {
        let in_trial = self.in_trial();
        let site = (declaration.clone(), name.to_string());
        let inferred = self.nil_starts.borrow().inferred.get(&site).cloned();
        if let Some(binding) = self.scopes.last_mut().and_then(|scope| scope.get_mut(name)) {
            if binding.value_type == Type::Nil {
                let assignments = inferred.unwrap_or_default();
                binding.value_type = if assignments.is_empty() && in_trial {
                    Type::Unknown
                } else if assignments.is_empty() {
                    Type::universal()
                } else {
                    Type::union(
                        std::iter::once(Type::Nil)
                            .chain(assignments.iter().map(|(_, value_type)| value_type.clone())),
                    )
                };
                binding.nil_start = Some(NilStart { site, assignments });
            }
            binding.declared = Some(binding.value_type.clone());
        }
    }

    /// Forgets what assignments and nil tests established about every `var`
    /// in scope, as a nested function must: it may run after any later
    /// assignment.
    pub(super) fn forget_mutable_facts(&mut self) {
        for scope in &mut self.scopes {
            for binding in scope.values_mut() {
                if let Some(declared) = &binding.declared {
                    binding.value_type = declared.clone();
                }
            }
        }
    }

    /// The binding of `name` in the innermost scope only.
    pub(super) fn lookup_in_scope(&self, name: &str) -> Option<&SemanticBinding> {
        self.scopes.last()?.get(name)
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
                    super::SourceErrorCode::UnknownName,
                    format!("unknown module binding `{module}`"),
                    span.clone(),
                )
            })?;
            if !binding.module_binding {
                return Err(super::SourceError::semantic(
                    super::SourceErrorCode::UnknownType,
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
                    super::SourceError::semantic(
                        super::SourceErrorCode::UnknownType,
                        format!("unknown type `{name}`"),
                        span.clone(),
                    )
                });
        }
        self.resource_type(name).ok_or_else(|| {
            super::SourceError::semantic(
                super::SourceErrorCode::UnknownType,
                format!("unknown type `{name}`"),
                span.clone(),
            )
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
                // A member fact holds after the branches only when each one
                // that continues established it.
                binding.member_facts = left
                    .member_facts
                    .iter()
                    .filter(|(key, value_type)| right.member_facts.get(*key) == Some(*value_type))
                    .map(|(key, value_type)| (key.clone(), value_type.clone()))
                    .collect();
            }
        }
    }

    /// Merge the environments that can reach a control-flow continuation.
    /// Returns whether at least one input environment survives.
    pub(super) fn merge_reachable_types(
        &mut self,
        left: Option<&Self>,
        right: Option<&Self>,
    ) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => {
                self.merge_compatible_types(left, right);
                true
            }
            (Some(environment), None) | (None, Some(environment)) => {
                self.merge_compatible_types(environment, environment);
                true
            }
            (None, None) => false,
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

    /// The type last recorded for the expression at `span`.
    pub(super) fn recorded_expression_type(&self, span: &SourceSpan) -> Option<Type> {
        self.records.borrow().expression_types.get(span).cloned()
    }

    pub(super) fn record_expression_type(&self, span: SourceSpan, value_type: Type) {
        self.records
            .borrow_mut()
            .expression_types
            .insert(span, value_type);
    }

    /// Records the names a `{*}` declaration defines, or, with `None`, that
    /// they are not known until it runs.
    pub(super) fn record_selection<'a>(&self, names: Option<impl Iterator<Item = &'a String>>) {
        let mut records = self.records.borrow_mut();
        match names {
            Some(names) => records.selected_globals.extend(names.cloned()),
            None => records.unknown_selection = true,
        }
    }

    pub(super) fn analysis(&self, snapshot: ModuleSnapshot) -> SemanticAnalysis {
        let records = self.records.borrow();
        SemanticAnalysis {
            snapshot,
            session_snapshot: self.session_snapshot(),
            selected_calls: records.selected_calls.clone(),
            function_identities: records.function_identities.clone(),
            foreign_identities: records.foreign_identities.clone(),
            foreign_resource_signatures: records.foreign_resource_signatures.clone(),
            match_constraints: records.match_constraints.clone(),
            expression_types: records.expression_types.clone(),
            selected_globals: (!records.unknown_selection)
                .then(|| records.selected_globals.clone()),
        }
    }

    fn session_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            bindings: self.scopes.first().cloned().unwrap_or_default(),
            types: self.type_scopes.first().cloned().unwrap_or_default(),
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
            inferred_alternatives: Vec::new(),
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
