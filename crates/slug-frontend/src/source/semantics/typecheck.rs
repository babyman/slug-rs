use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use slug_vm::{CallableIdentity, DeferMode, SourceSpan, Value, ValueKind};

use super::{
    SourceEdit, SourceError, SourceErrorCode, SourceRange,
    ast::{
        Binary, CallArgument, CasePattern, Expr, ExprKind, ListElement, MapPatternKey, MatchCase,
        Parameter, Pattern, Prefix, SelectCaseKind, Tag, TypeAnnotation,
    },
    environment::{
        CallableParameter, CallableSignature, Environment, ImportSnapshots, ModuleSnapshot,
        NilStartSite, SemanticAnalysis, SemanticBinding, SessionSnapshot, function_value_type,
    },
    semantic::{
        AlternativeType, AlternativeVariable, InferredAlternative, ResourceIdentity,
        SchemaIdentity, Type, resolve_annotation, resolve_resource_references,
        resolve_static_annotation,
    },
};

mod type_relations;

use type_relations::{
    is_dynamic_operation_type, is_map_key_type, type_intersection, type_subtract,
};

pub(super) fn analyze_with_imports(
    expressions: &[Expr],
    imports: ImportSnapshots,
    module_name: Option<&str>,
) -> Result<SemanticAnalysis, SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    analyze_expressions(
        expressions,
        imports,
        &SessionSnapshot::default(),
        module_name,
    )
}

pub(super) fn analyze_with_imports_and_session(
    expressions: &[Expr],
    imports: ImportSnapshots,
    session: &SessionSnapshot,
) -> Result<SemanticAnalysis, SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    analyze_expressions(expressions, imports, session, None)
}

pub(super) fn static_import_names(expressions: &[Expr]) -> Vec<String> {
    import_scan(expressions).names
}

/// Literal module names passed to `import`, and the calls that pass anything
/// else. A dynamic call can reach modules the literal names do not list.
#[derive(Debug, Default)]
pub(crate) struct ImportScan {
    pub(crate) names: Vec<String>,
    pub(crate) dynamic: Vec<SourceSpan>,
}

pub(super) fn import_scan(expressions: &[Expr]) -> ImportScan {
    let mut scan = ImportScan::default();
    for expression in expressions {
        collect_imports(expression, &mut scan);
    }
    scan.names.sort();
    scan.names.dedup();
    scan
}

/// Whether evaluating an expression can leave control at the following source
/// expression. This is deliberately independent from the expression's value
/// type: a `return value` contributes `value` to function-result checking but
/// cannot fall through to its enclosing block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Continuation {
    FallsThrough,
    Terminates,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CheckedExpression {
    value_type: Type,
    continuation: Continuation,
}

/// Requirements gathered for one function's unannotated parameters.
///
/// This state is intentionally scoped to the declaration currently being
/// checked. In particular, a nested function gets its own state rather than
/// contributing evidence to an enclosing parameter.
#[derive(Debug)]
struct ParameterConstraints {
    /// What the path being collected requires of each parameter; once the
    /// body is collected, what every path that finishes requires.
    requirements: HashMap<String, ParameterRequirement>,
    /// Whether the path being collected still runs: `return` and `throw`
    /// end it.
    reachable: bool,
    /// The requirements of each path that left through `return`.
    exits: Vec<HashMap<String, ParameterRequirement>>,
    /// The requirements the operator collection found, which known-call
    /// propagation reads but does not change.
    facts: HashMap<String, Type>,
    plus_operands: Vec<PlusOperands>,
    subtract_operands: Vec<PlusOperands>,
    multiply_operands: Vec<PlusOperands>,
    nil_partitions: Vec<NilGuardPartition>,
    /// Whether the body leaves through `return`, so its result is not only
    /// its final expression.
    returns: bool,
}

#[derive(Clone, Debug)]
struct ParameterRequirement {
    value_type: Type,
    span: SourceSpan,
}

/// Where one path of a branch ended.
struct ConstraintPath {
    requirements: HashMap<String, ParameterRequirement>,
    reachable: bool,
}

#[derive(Debug)]
struct PlusOperands {
    left: String,
    right: String,
    span: SourceSpan,
}

/// A direct nil guard whose two branch bodies must be inferred as separate
/// correlated relations before their disjoint input domains can be joined.
#[derive(Debug)]
#[allow(dead_code)] // Consumed by the following guarded-relation inference stage.
struct NilGuardPartition {
    parameter: String,
    nil_branch: Expr,
    non_nil_branch: Expr,
}

impl ParameterConstraints {
    fn for_parameters(parameters: &[Parameter]) -> Self {
        Self {
            requirements: parameters
                .iter()
                .filter(|parameter| {
                    !parameter.discard && !parameter.variadic && parameter.annotation.is_none()
                })
                .map(|parameter| {
                    (
                        parameter.name.clone(),
                        ParameterRequirement {
                            value_type: Type::Unknown,
                            span: SourceSpan::new("<parameter>", 0, 0),
                        },
                    )
                })
                .collect(),
            reachable: true,
            exits: Vec::new(),
            facts: HashMap::new(),
            plus_operands: Vec::new(),
            subtract_operands: Vec::new(),
            multiply_operands: Vec::new(),
            nil_partitions: Vec::new(),
            returns: false,
        }
    }

    /// Collects one path of a branch from the current point, and returns
    /// where it ended, leaving the current point as it was.
    fn path(
        &mut self,
        collect: impl FnOnce(&mut Self) -> Result<(), SourceError>,
    ) -> Result<ConstraintPath, SourceError> {
        let requirements = self.requirements.clone();
        let reachable = self.reachable;
        collect(self)?;
        Ok(ConstraintPath {
            requirements: std::mem::replace(&mut self.requirements, requirements),
            reachable: std::mem::replace(&mut self.reachable, reachable),
        })
    }

    /// The current point as a path that takes no branch.
    fn untaken(&self) -> ConstraintPath {
        ConstraintPath {
            requirements: self.requirements.clone(),
            reachable: self.reachable,
        }
    }

    /// Continues from the paths of a branch that still run. A body may take
    /// any of them, so a parameter keeps a requirement only when every one
    /// has it; with none still running, neither does the current path.
    fn join(&mut self, paths: Vec<ConstraintPath>) {
        let running = paths
            .into_iter()
            .filter(|path| path.reachable)
            .map(|path| path.requirements);
        match joined_requirements(running) {
            Some(requirements) => {
                self.requirements = requirements;
                self.reachable = true;
            }
            None => self.reachable = false,
        }
    }

    /// Ends the body: its requirements become those every path that
    /// finishes, by its end or through `return`, has. A path that throws
    /// does not finish, so it adds none.
    fn finish(&mut self) {
        let mut paths = std::mem::take(&mut self.exits);
        if self.reachable {
            paths.push(self.requirements.clone());
        }
        if let Some(requirements) = joined_requirements(paths.into_iter()) {
            self.requirements = requirements;
        }
        self.reachable = true;
    }

    /// Starts known-call propagation, which collects its own paths and reads
    /// the requirements found so far as fixed facts.
    fn start_known_calls(&mut self) -> HashMap<String, ParameterRequirement> {
        self.facts = self
            .requirements
            .iter()
            .map(|(name, requirement)| (name.clone(), requirement.value_type.clone()))
            .collect();
        let unconstrained = self
            .requirements
            .iter()
            .map(|(name, requirement)| {
                (
                    name.clone(),
                    ParameterRequirement {
                        value_type: Type::Unknown,
                        span: requirement.span.clone(),
                    },
                )
            })
            .collect();
        std::mem::replace(&mut self.requirements, unconstrained)
    }

    /// Combines the operator requirements with known-call propagation's:
    /// each holds on every path that finishes, so both do.
    fn combine(
        &mut self,
        operators: HashMap<String, ParameterRequirement>,
    ) -> Result<(), SourceError> {
        let calls = std::mem::replace(&mut self.requirements, operators);
        for (name, requirement) in calls {
            if !matches!(requirement.value_type, Type::Unknown) {
                self.require_name(&name, requirement.value_type, &requirement.span, None)?;
            }
        }
        Ok(())
    }

    /// Records a requirement at the operand occurrence. If later inference
    /// domains introduce incompatible requirements, the later occurrence is
    /// the diagnostic span: it is where the function first becomes
    /// contradictory.
    fn require(&mut self, expression: &Expr, value_type: Type) -> Result<(), SourceError> {
        let ExprKind::Name(name) = &expression.kind else {
            return Ok(());
        };
        self.require_name(name, value_type, &expression.span, Some(expression))
    }

    fn require_name(
        &mut self,
        name: &str,
        value_type: Type,
        span: &SourceSpan,
        expression: Option<&Expr>,
    ) -> Result<(), SourceError> {
        let Some(requirement) = self.requirements.get_mut(name) else {
            return Ok(());
        };
        if matches!(requirement.value_type, Type::Unknown) {
            requirement.value_type = value_type;
            requirement.span = span.clone();
            return Ok(());
        }
        // Both requirements must hold, so when one type is assignable to the
        // other the narrower one is their intersection. Passing a `num`
        // parameter on to an unconstrained (`any|nil`) parameter adds nothing.
        if requirement.value_type.is_assignable_to(&value_type) {
            return Ok(());
        }
        if value_type.is_assignable_to(&requirement.value_type) {
            requirement.value_type = value_type;
            requirement.span = span.clone();
            return Ok(());
        }
        let error = SourceError::semantic(
            SourceErrorCode::TypeMismatch,
            format!(
                "parameter `{name}` has incompatible body requirements: {} and {}",
                requirement.value_type, value_type
            ),
            span.clone(),
        );
        Err(match expression {
            Some(expression) => error.with_extent(expression),
            None => error,
        })
    }

    /// Narrows requirements the body has by what its nested functions
    /// require of the same parameters, as in `defer tcp.close(c)` followed
    /// by a function that calls `tcp.read(c)`. A nested function may never
    /// run, so it only narrows a requirement and never adds one; its body is
    /// checked against the parameter's type, so a requirement it does not
    /// meet would be an error there anyway. `shadowed` holds the names
    /// enclosing nested functions bind.
    fn narrow_from_nested_functions(
        &mut self,
        expression: &Expr,
        environment: &Environment,
        shadowed: &HashSet<String>,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
            let mut result = Ok(());
            expression.for_each_child(|child| {
                if result.is_err() {
                    return;
                }
                result = match &child.kind {
                    ExprKind::Function {
                        parameters, body, ..
                    } => {
                        let mut shadowed = shadowed.clone();
                        shadowed.extend(parameters.iter().map(|parameter| parameter.name.clone()));
                        self.narrow_from_nested_function(body, environment, &shadowed)
                            .and_then(|()| {
                                self.narrow_from_nested_functions(body, environment, &shadowed)
                            })
                    }
                    _ => self.narrow_from_nested_functions(child, environment, shadowed),
                };
            });
            result
        })
    }

    fn narrow_from_nested_function(
        &mut self,
        body: &Expr,
        environment: &Environment,
        shadowed: &HashSet<String>,
    ) -> Result<(), SourceError> {
        let mut nested = Self::for_parameters(&[]);
        nested.requirements = self
            .requirements
            .iter()
            .filter(|(name, requirement)| {
                !shadowed.contains(*name) && !matches!(requirement.value_type, Type::Unknown)
            })
            .map(|(name, requirement)| {
                (
                    name.clone(),
                    ParameterRequirement {
                        value_type: Type::Unknown,
                        span: requirement.span.clone(),
                    },
                )
            })
            .collect();
        if nested.requirements.is_empty() {
            return Ok(());
        }
        nested.collect(body)?;
        nested.collect_known_calls(body, environment)?;
        nested.finish();
        for (name, requirement) in nested.requirements {
            let narrower = self.requirements.get(&name).is_some_and(|outer| {
                requirement.value_type.is_assignable_to(&outer.value_type)
                    && !outer.value_type.is_assignable_to(&requirement.value_type)
            });
            if narrower && !matches!(requirement.value_type, Type::Unknown) {
                self.require_name(&name, requirement.value_type, &requirement.span, None)?;
            }
        }
        Ok(())
    }

    fn include_nil_partition(&mut self, name: &str) {
        let Some(requirement) = self.requirements.get_mut(name) else {
            return;
        };
        if !matches!(requirement.value_type, Type::Unknown) {
            requirement.value_type = Type::union([requirement.value_type.clone(), Type::Nil]);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn collect(&mut self, expression: &Expr) -> Result<(), SourceError> {
        super::stack::grow(|| {
            match &expression.kind {
                ExprKind::Binary {
                    left,
                    operator: Binary::Add,
                    right,
                } => {
                    self.collect_plus_operands(left, right, &expression.span);
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Binary {
                    left,
                    operator: Binary::Subtract,
                    right,
                } => {
                    self.collect_subtract_operands(left, right, &expression.span);
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Binary {
                    left,
                    operator: Binary::Multiply,
                    right,
                } => {
                    self.collect_multiply_operands(left, right, &expression.span);
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Binary {
                    left,
                    operator: Binary::Divide | Binary::Modulo,
                    right,
                } => {
                    self.require(left, Type::Num)?;
                    self.require(right, Type::Num)?;
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Binary {
                    left,
                    operator:
                        Binary::Greater | Binary::GreaterEqual | Binary::Less | Binary::LessEqual,
                    right,
                } => {
                    // Ordering accepts two numbers or two strings, so only a
                    // literal on the other side decides which one a parameter is.
                    if let Some(family) = literal_family(right) {
                        self.require(left, family)?;
                    }
                    if let Some(family) = literal_family(left) {
                        self.require(right, family)?;
                    }
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Prefix { operators, value } => {
                    if operators
                        .iter()
                        .any(|(operator, _)| matches!(operator, Prefix::Negate))
                    {
                        self.require(value, Type::Num)?;
                    }
                    self.collect(value)
                }
                ExprKind::Binary {
                    left,
                    operator: operator @ (Binary::And | Binary::Or | Binary::Coalesce),
                    right,
                } => {
                    self.collect(left)?;
                    let mut skipped = self.untaken();
                    let mut evaluated = self.path(|constraints| constraints.collect(right))?;
                    assume_short_circuit(*operator, left, &mut skipped, &mut evaluated);
                    self.join(vec![skipped, evaluated]);
                    Ok(())
                }
                ExprKind::Binary { left, right, .. } => {
                    self.collect(left)?;
                    self.collect(right)
                }
                ExprKind::Declare {
                    pattern,
                    tags,
                    value,
                    ..
                } => {
                    self.collect_pattern(pattern)?;
                    self.collect_tags(tags)?;
                    self.collect(value)
                }
                ExprKind::Return { value } => {
                    self.returns = true;
                    self.collect(value)?;
                    if self.reachable {
                        self.exits.push(self.requirements.clone());
                    }
                    self.reachable = false;
                    Ok(())
                }
                ExprKind::Throw { value } => {
                    self.collect(value)?;
                    self.reachable = false;
                    Ok(())
                }
                ExprKind::Assign { value, .. }
                | ExprKind::Defer { value, .. }
                | ExprKind::Spawn(value)
                | ExprKind::TypeApply { callee: value, .. } => self.collect(value),
                ExprKind::Recur(arguments) => self.collect_arguments(arguments),
                ExprKind::Nursery { limit, body } => {
                    if let Some(limit) = limit {
                        self.collect(limit)?;
                    }
                    self.collect(body)
                }
                ExprKind::Select(cases) => {
                    for case in cases {
                        match &case.kind {
                            SelectCaseKind::Receive(value)
                            | SelectCaseKind::After(value)
                            | SelectCaseKind::Await(value) => self.collect(value)?,
                            SelectCaseKind::Send { channel, value } => {
                                self.collect(channel)?;
                                self.collect(value)?;
                            }
                            SelectCaseKind::Default => {}
                        }
                    }
                    let mut paths = Vec::new();
                    for case in cases {
                        paths.push(self.path(|constraints| {
                            case.handler
                                .as_ref()
                                .map_or(Ok(()), |handler| constraints.collect(handler))
                        })?);
                    }
                    self.join(paths);
                    Ok(())
                }
                ExprKind::Match { subject, cases } => {
                    if let Some(subject) = subject {
                        self.collect(subject)?;
                    }
                    let mut paths = Vec::new();
                    for case in cases {
                        let mut path = self.path(|constraints| {
                            for pattern in &case.patterns {
                                constraints.collect_pattern(&pattern.pattern)?;
                            }
                            if let Some(guard) = &case.guard {
                                constraints.collect(guard)?;
                            }
                            constraints.collect(&case.value)
                        })?;
                        assume_match_case(&mut path, case);
                        paths.push(path);
                    }
                    self.join(paths);
                    Ok(())
                }
                ExprKind::Call { callee, arguments } => {
                    self.collect(callee)?;
                    self.collect_arguments(arguments)
                }
                ExprKind::Block(values) => {
                    for value in values {
                        self.collect(value)?;
                    }
                    Ok(())
                }
                ExprKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.collect_nil_guard_partition(
                        condition,
                        then_branch,
                        else_branch.as_deref(),
                    );
                    self.collect(condition)?;
                    let mut then = self.path(|constraints| constraints.collect(then_branch))?;
                    let mut otherwise = self.path(|constraints| {
                        else_branch
                            .as_deref()
                            .map_or(Ok(()), |else_branch| constraints.collect(else_branch))
                    })?;
                    cover_nil_path(condition, &mut then, &mut otherwise);
                    assume(&mut then, condition, true);
                    assume(&mut otherwise, condition, false);
                    self.join(vec![then, otherwise]);
                    Ok(())
                }
                ExprKind::List(values) => {
                    for value in values {
                        self.collect(match value {
                            ListElement::Value(value) | ListElement::Spread(value) => value,
                        })?;
                    }
                    Ok(())
                }
                ExprKind::Map(entries) => {
                    for (key, value) in entries {
                        self.collect(key)?;
                        self.collect(value)?;
                    }
                    Ok(())
                }
                ExprKind::StructSchema(fields) => {
                    for field in fields {
                        if let Some(default) = &field.default {
                            self.collect(default)?;
                        }
                    }
                    Ok(())
                }
                ExprKind::StructInit { schema, fields } => {
                    self.collect(schema)?;
                    for (_, value) in fields {
                        self.collect(value)?;
                    }
                    Ok(())
                }
                ExprKind::StructCopy { value, fields } => {
                    self.collect(value)?;
                    for (_, replacement) in fields {
                        self.collect(replacement)?;
                    }
                    Ok(())
                }
                ExprKind::Index { collection, index } => {
                    self.collect(collection)?;
                    self.collect(index)
                }
                ExprKind::Slice {
                    collection,
                    start,
                    end,
                    step,
                } => {
                    self.collect(collection)?;
                    for bound in [start, end, step].into_iter().flatten() {
                        self.collect(bound)?;
                    }
                    Ok(())
                }
                ExprKind::Function { .. }
                | ExprKind::Foreign { .. }
                | ExprKind::Resource { .. }
                | ExprKind::Enum { .. }
                | ExprKind::TypeAlias { .. }
                | ExprKind::Value(_)
                | ExprKind::Interpolate(_)
                | ExprKind::Documentation(_)
                | ExprKind::NotImplemented
                | ExprKind::Name(_) => Ok(()),
            }
        })
    }

    fn collect_nil_guard_partition(
        &mut self,
        condition: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
    ) {
        let Some(else_branch) = else_branch else {
            return;
        };
        let ExprKind::Binary {
            left,
            operator,
            right,
        } = &condition.kind
        else {
            return;
        };
        let ((ExprKind::Name(parameter), ExprKind::Value(Value::Nil))
        | (ExprKind::Value(Value::Nil), ExprKind::Name(parameter))) = (&left.kind, &right.kind)
        else {
            return;
        };
        if !self.requirements.contains_key(parameter) {
            return;
        }
        let (nil_branch, non_nil_branch) = if matches!(operator, Binary::Equal) {
            (then_branch.clone(), else_branch.clone())
        } else if matches!(operator, Binary::NotEqual) {
            (else_branch.clone(), then_branch.clone())
        } else {
            return;
        };
        self.nil_partitions.push(NilGuardPartition {
            parameter: parameter.clone(),
            nil_branch,
            non_nil_branch,
        });
    }

    fn collect_plus_operands(&mut self, left: &Expr, right: &Expr, span: &SourceSpan) {
        let (ExprKind::Name(left), ExprKind::Name(right)) = (&left.kind, &right.kind) else {
            return;
        };
        if self.requirements.contains_key(left) && self.requirements.contains_key(right) {
            self.plus_operands.push(PlusOperands {
                left: left.clone(),
                right: right.clone(),
                span: span.clone(),
            });
        }
    }

    fn collect_subtract_operands(&mut self, left: &Expr, right: &Expr, span: &SourceSpan) {
        let (ExprKind::Name(left), ExprKind::Name(right)) = (&left.kind, &right.kind) else {
            return;
        };
        if self.requirements.contains_key(left) && self.requirements.contains_key(right) {
            self.subtract_operands.push(PlusOperands {
                left: left.clone(),
                right: right.clone(),
                span: span.clone(),
            });
        }
    }

    fn collect_multiply_operands(&mut self, left: &Expr, right: &Expr, span: &SourceSpan) {
        let (ExprKind::Name(left), ExprKind::Name(right)) = (&left.kind, &right.kind) else {
            return;
        };
        if self.requirements.contains_key(left) && self.requirements.contains_key(right) {
            self.multiply_operands.push(PlusOperands {
                left: left.clone(),
                right: right.clone(),
                span: span.clone(),
            });
        }
    }

    fn collect_tags(&mut self, tags: &[Tag]) -> Result<(), SourceError> {
        super::stack::grow(|| {
            for tag in tags {
                for argument in &tag.arguments {
                    self.collect(argument)?;
                }
            }
            Ok(())
        })
    }

    fn collect_arguments(&mut self, arguments: &[CallArgument]) -> Result<(), SourceError> {
        super::stack::grow(|| {
            for argument in arguments {
                self.collect(match argument {
                    CallArgument::Positional(value)
                    | CallArgument::Named { value, .. }
                    | CallArgument::Spread(value) => value,
                })?;
            }
            Ok(())
        })
    }

    /// Adds facts from calls whose callee metadata is already available in the
    /// lexical environment. The ordinary collector deliberately remains
    /// environment-free so its operator facts cannot accidentally depend on
    /// declaration order.
    #[allow(clippy::too_many_lines)]
    fn collect_known_calls(
        &mut self,
        expression: &Expr,
        environment: &Environment,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| match &expression.kind {
            ExprKind::Call { callee, arguments } => {
                if let Some(binding) = expression_binding(callee, environment)
                    && let [signature] = binding.callables.as_slice()
                    && signature.generic_arity == 0
                {
                    let shapes = arguments
                        .iter()
                        .map(|argument| match argument {
                            CallArgument::Positional(_) => ArgumentShape::Positional,
                            CallArgument::Named { name, .. } => ArgumentShape::Named(name),
                            CallArgument::Spread(_) => ArgumentShape::Spread,
                        })
                        .collect::<Vec<_>>();
                    let actuals = arguments
                        .iter()
                        .map(|argument| match argument {
                            CallArgument::Positional(argument)
                            | CallArgument::Named {
                                value: argument, ..
                            }
                            | CallArgument::Spread(argument) => self.body_fact_type(argument),
                        })
                        .collect::<Vec<_>>();
                    if let Some(bound) = bind_arguments(&signature.parameters, &shapes, &actuals) {
                        let selected = signature
                            .inferred_alternatives
                            .is_empty()
                            .then(|| {
                                bound
                                    .values
                                    .iter()
                                    .map(|(_, parameter, _)| parameter.value_type.clone())
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                (bound.values.len() == signature.parameters.len())
                                    .then(|| {
                                        let mut alternative_actuals =
                                            vec![Type::Unknown; signature.parameters.len()];
                                        for (index, _, actual) in &bound.values {
                                            alternative_actuals[*index] = (*actual).clone();
                                        }
                                        let alternatives = signature
                                            .inferred_alternatives
                                            .iter()
                                            .filter_map(|alternative| {
                                                alternative.instantiate(&alternative_actuals)
                                            })
                                            .collect::<Vec<_>>();
                                        (alternatives.len() == 1).then(|| {
                                            alternatives
                                                .into_iter()
                                                .next()
                                                .expect("one alternative")
                                                .0
                                        })
                                    })
                                    .flatten()
                            });
                        if let Some(selected) = selected {
                            for ((index, _, _), argument) in bound.values.iter().zip(arguments) {
                                let argument = match argument {
                                    CallArgument::Positional(argument)
                                    | CallArgument::Named {
                                        value: argument, ..
                                    } => argument,
                                    CallArgument::Spread(_) => continue,
                                };
                                self.require(argument, selected[*index].clone())?;
                            }
                        }
                    }
                }
                self.collect_known_calls(callee, environment)?;
                self.collect_known_call_arguments(arguments, environment)
            }
            ExprKind::Declare { value, tags, .. } => {
                self.collect_known_calls(value, environment)?;
                for tag in tags {
                    for argument in &tag.arguments {
                        self.collect_known_calls(argument, environment)?;
                    }
                }
                Ok(())
            }
            ExprKind::Return { value } => {
                self.collect_known_calls(value, environment)?;
                if self.reachable {
                    self.exits.push(self.requirements.clone());
                }
                self.reachable = false;
                Ok(())
            }
            ExprKind::Throw { value } => {
                self.collect_known_calls(value, environment)?;
                self.reachable = false;
                Ok(())
            }
            ExprKind::Assign { value, .. }
            | ExprKind::Defer { value, .. }
            | ExprKind::Spawn(value)
            | ExprKind::Prefix { value, .. }
            | ExprKind::TypeApply { callee: value, .. } => {
                self.collect_known_calls(value, environment)
            }
            ExprKind::Recur(arguments) => self.collect_known_call_arguments(arguments, environment),
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    self.collect_known_calls(limit, environment)?;
                }
                self.collect_known_calls(body, environment)
            }
            ExprKind::Select(cases) => {
                for case in cases {
                    match &case.kind {
                        SelectCaseKind::Receive(value)
                        | SelectCaseKind::After(value)
                        | SelectCaseKind::Await(value) => {
                            self.collect_known_calls(value, environment)?;
                        }
                        SelectCaseKind::Send { channel, value } => {
                            self.collect_known_calls(channel, environment)?;
                            self.collect_known_calls(value, environment)?;
                        }
                        SelectCaseKind::Default => {}
                    }
                }
                let mut paths = Vec::new();
                for case in cases {
                    paths.push(self.path(|constraints| {
                        case.handler.as_ref().map_or(Ok(()), |handler| {
                            constraints.collect_known_calls(handler, environment)
                        })
                    })?);
                }
                self.join(paths);
                Ok(())
            }
            ExprKind::Match { subject, cases } => {
                if let Some(subject) = subject {
                    self.collect_known_calls(subject, environment)?;
                }
                let mut paths = Vec::new();
                for case in cases {
                    let mut path = self.path(|constraints| {
                        if let Some(guard) = &case.guard {
                            constraints.collect_known_calls(guard, environment)?;
                        }
                        constraints.collect_known_calls(&case.value, environment)
                    })?;
                    assume_match_case(&mut path, case);
                    paths.push(path);
                }
                self.join(paths);
                Ok(())
            }
            ExprKind::Binary {
                left,
                operator: operator @ (Binary::And | Binary::Or | Binary::Coalesce),
                right,
            } => {
                self.collect_known_calls(left, environment)?;
                let mut skipped = self.untaken();
                let mut evaluated =
                    self.path(|constraints| constraints.collect_known_calls(right, environment))?;
                assume_short_circuit(*operator, left, &mut skipped, &mut evaluated);
                self.join(vec![skipped, evaluated]);
                Ok(())
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_known_calls(left, environment)?;
                self.collect_known_calls(right, environment)
            }
            ExprKind::Block(values) => {
                for value in values {
                    self.collect_known_calls(value, environment)?;
                }
                Ok(())
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_known_calls(condition, environment)?;
                let mut then = self.path(|constraints| {
                    constraints.collect_known_calls(then_branch, environment)
                })?;
                let mut otherwise = self.path(|constraints| {
                    else_branch.as_deref().map_or(Ok(()), |else_branch| {
                        constraints.collect_known_calls(else_branch, environment)
                    })
                })?;
                cover_nil_path(condition, &mut then, &mut otherwise);
                assume(&mut then, condition, true);
                assume(&mut otherwise, condition, false);
                self.join(vec![then, otherwise]);
                Ok(())
            }
            ExprKind::List(values) => {
                for value in values {
                    self.collect_known_calls(
                        match value {
                            ListElement::Value(value) | ListElement::Spread(value) => value,
                        },
                        environment,
                    )?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.collect_known_calls(key, environment)?;
                    self.collect_known_calls(value, environment)?;
                }
                Ok(())
            }
            ExprKind::StructInit { schema, fields } => {
                self.collect_known_calls(schema, environment)?;
                for (_, value) in fields {
                    self.collect_known_calls(value, environment)?;
                }
                Ok(())
            }
            ExprKind::StructCopy { value, fields } => {
                self.collect_known_calls(value, environment)?;
                for (_, replacement) in fields {
                    self.collect_known_calls(replacement, environment)?;
                }
                Ok(())
            }
            ExprKind::Index { collection, index } => {
                self.collect_known_calls(collection, environment)?;
                self.collect_known_calls(index, environment)
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                self.collect_known_calls(collection, environment)?;
                for bound in [start, end, step].into_iter().flatten() {
                    self.collect_known_calls(bound, environment)?;
                }
                Ok(())
            }
            ExprKind::Function { .. }
            | ExprKind::Foreign { .. }
            | ExprKind::StructSchema(_)
            | ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. }
            | ExprKind::Value(_)
            | ExprKind::Interpolate(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Name(_) => Ok(()),
        })
    }

    /// Returns only a fact collected independently from known-call
    /// propagation. In particular, an inferred alternative cannot select
    /// itself by first constraining one of this function's parameters.
    fn body_fact_type(&self, expression: &Expr) -> Type {
        let ExprKind::Name(name) = &expression.kind else {
            return Type::Unknown;
        };
        self.facts.get(name).cloned().unwrap_or(Type::Unknown)
    }

    fn collect_known_call_arguments(
        &mut self,
        arguments: &[CallArgument],
        environment: &Environment,
    ) -> Result<(), SourceError> {
        super::stack::grow(|| {
            for argument in arguments {
                self.collect_known_calls(
                    match argument {
                        CallArgument::Positional(value)
                        | CallArgument::Named { value, .. }
                        | CallArgument::Spread(value) => value,
                    },
                    environment,
                )?;
            }
            Ok(())
        })
    }

    fn collect_nil_partitions(&mut self, expression: &Expr) {
        super::stack::grow(|| match &expression.kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if let ExprKind::Binary {
                    left,
                    operator: Binary::Equal | Binary::NotEqual,
                    right,
                } = &condition.kind
                    && let (ExprKind::Name(name), ExprKind::Value(Value::Nil)) =
                        (&left.kind, &right.kind)
                {
                    self.include_nil_partition(name);
                }
                self.collect_nil_partitions(condition);
                self.collect_nil_partitions(then_branch);
                if let Some(else_branch) = else_branch {
                    self.collect_nil_partitions(else_branch);
                }
            }
            ExprKind::Block(values) => {
                for value in values {
                    self.collect_nil_partitions(value);
                }
            }
            ExprKind::Declare { value, .. }
            | ExprKind::Assign { value, .. }
            | ExprKind::Return { value }
            | ExprKind::Throw { value }
            | ExprKind::Defer { value, .. }
            | ExprKind::Spawn(value)
            | ExprKind::Prefix { value, .. }
            | ExprKind::TypeApply { callee: value, .. } => self.collect_nil_partitions(value),
            ExprKind::Call { callee, arguments } => {
                self.collect_nil_partitions(callee);
                for argument in arguments {
                    self.collect_nil_partitions(match argument {
                        CallArgument::Positional(value)
                        | CallArgument::Named { value, .. }
                        | CallArgument::Spread(value) => value,
                    });
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_nil_partitions(left);
                self.collect_nil_partitions(right);
            }
            ExprKind::List(values) => {
                for value in values {
                    self.collect_nil_partitions(match value {
                        ListElement::Value(value) | ListElement::Spread(value) => value,
                    });
                }
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.collect_nil_partitions(key);
                    self.collect_nil_partitions(value);
                }
            }
            ExprKind::Function { .. }
            | ExprKind::Foreign { .. }
            | ExprKind::Recur(_)
            | ExprKind::Nursery { .. }
            | ExprKind::Select(_)
            | ExprKind::Match { .. }
            | ExprKind::StructSchema(_)
            | ExprKind::StructInit { .. }
            | ExprKind::StructCopy { .. }
            | ExprKind::Index { .. }
            | ExprKind::Slice { .. }
            | ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. }
            | ExprKind::Value(_)
            | ExprKind::Interpolate(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Name(_) => {}
        });
    }

    fn collect_pattern(&mut self, pattern: &Pattern) -> Result<(), SourceError> {
        super::stack::grow(|| match pattern {
            Pattern::At { pattern, .. } => self.collect_pattern(pattern),
            Pattern::List { items, .. } => {
                for item in items {
                    self.collect_pattern(item)?;
                }
                Ok(())
            }
            Pattern::Map { entries, .. } => {
                for (key, value) in entries {
                    if let MapPatternKey::Computed(key) = key {
                        self.collect(key)?;
                    }
                    self.collect_pattern(value)?;
                }
                Ok(())
            }
            Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Binding(_)
            | Pattern::Pinned(_)
            | Pattern::MapAll
            | Pattern::EnumCase { .. } => Ok(()),
        })
    }
}

fn parameter_constraints(
    parameters: &[Parameter],
    body: &Expr,
    environment: &Environment,
) -> Result<ParameterConstraints, SourceError> {
    // A default runs whenever its argument is omitted, so a call that omits
    // it has the default's requirements on every path.
    let mut constraints = ParameterConstraints::for_parameters(parameters);
    for parameter in parameters {
        if let Some(default) = &parameter.default {
            constraints.collect(default)?;
        }
    }
    constraints.collect(body)?;
    constraints.finish();
    let operators = constraints.start_known_calls();
    for parameter in parameters {
        if let Some(default) = &parameter.default {
            constraints.collect_known_calls(default, environment)?;
        }
    }
    constraints.collect_known_calls(body, environment)?;
    constraints.finish();
    constraints.combine(operators)?;
    constraints.narrow_from_nested_functions(body, environment, &HashSet::new())?;
    constraints.collect_nil_partitions(body);
    Ok(constraints)
}

/// For a condition comparing a parameter with `nil`, gives the path it takes
/// when the parameter is `nil` the other path's requirement of it, if it has
/// none of its own: that path holds only `nil`, which the nil partition adds
/// to the requirement.
fn cover_nil_path(condition: &Expr, then: &mut ConstraintPath, otherwise: &mut ConstraintPath) {
    let ExprKind::Binary {
        left,
        operator,
        right,
    } = &condition.kind
    else {
        return;
    };
    let ((ExprKind::Name(parameter), ExprKind::Value(Value::Nil))
    | (ExprKind::Value(Value::Nil), ExprKind::Name(parameter))) = (&left.kind, &right.kind)
    else {
        return;
    };
    let (nil_path, other) = match operator {
        Binary::Equal => (then, otherwise),
        Binary::NotEqual => (otherwise, then),
        _ => return,
    };
    let Some(required) = other.requirements.get(parameter).cloned() else {
        return;
    };
    if let Some(requirement) = nil_path.requirements.get_mut(parameter)
        && matches!(requirement.value_type, Type::Unknown)
    {
        *requirement = required;
    }
}

/// Gives each parameter that `condition` evaluating to `holds` shows to equal
/// a literal that literal's type on `path`, if it has no requirement there:
/// on that path it is that type, so a requirement of it holds.
fn assume(path: &mut ConstraintPath, condition: &Expr, holds: bool) {
    let mut facts = Vec::new();
    literal_equalities(condition, holds, &mut facts);
    for (name, value_type) in facts {
        if let Some(requirement) = path.requirements.get_mut(name)
            && matches!(requirement.value_type, Type::Unknown)
        {
            requirement.value_type = value_type;
        }
    }
}

/// `&&` skips its right operand when the left one is false, and `||` when it
/// is true.
fn assume_short_circuit(
    operator: Binary,
    left: &Expr,
    skipped: &mut ConstraintPath,
    evaluated: &mut ConstraintPath,
) {
    let skips_when = match operator {
        Binary::And => false,
        Binary::Or => true,
        _ => return,
    };
    assume(skipped, left, skips_when);
    assume(evaluated, left, !skips_when);
}

/// A case's guard shows what its equalities do on the case's path.
fn assume_match_case(path: &mut ConstraintPath, case: &MatchCase) {
    if let Some(guard) = &case.guard {
        assume(path, guard, true);
    }
}

/// The parameters `condition` evaluating to `holds` shows to equal a
/// literal, with the literal's type.
fn literal_equalities<'a>(condition: &'a Expr, holds: bool, facts: &mut Vec<(&'a str, Type)>) {
    super::stack::grow(|| {
        let ExprKind::Binary {
            left,
            operator,
            right,
        } = &condition.kind
        else {
            return;
        };
        match (operator, holds) {
            (Binary::And, true) | (Binary::Or, false) => {
                literal_equalities(left, holds, facts);
                literal_equalities(right, holds, facts);
            }
            (Binary::Equal, true) | (Binary::NotEqual, false) => match (&left.kind, &right.kind) {
                (ExprKind::Name(name), _) => {
                    facts.extend(literal_family(right).map(|family| (name.as_str(), family)));
                }
                (_, ExprKind::Name(name)) => {
                    facts.extend(literal_family(left).map(|family| (name.as_str(), family)));
                }
                _ => {}
            },
            _ => {}
        }
    });
}

/// The requirements every path has: a parameter keeps one only when each
/// path requires the same type. `None` when there is no path.
fn joined_requirements(
    mut paths: impl Iterator<Item = HashMap<String, ParameterRequirement>>,
) -> Option<HashMap<String, ParameterRequirement>> {
    let mut joined = paths.next()?;
    for path in paths {
        for (name, requirement) in &mut joined {
            if path.get(name).map(|other| &other.value_type) != Some(&requirement.value_type) {
                requirement.value_type = Type::Unknown;
            }
        }
    }
    Some(joined)
}

fn solved_parameter_types(constraints: &ParameterConstraints) -> HashMap<String, Type> {
    constraints
        .requirements
        .iter()
        .filter(|(_, requirement)| !matches!(requirement.value_type, Type::Unknown))
        .map(|(name, requirement)| (name.clone(), requirement.value_type.clone()))
        .collect()
}

/// A private inference budget prevents control-flow composition from creating
/// an unbounded number of correlated callable schemes.
const MAX_INFERRED_ALTERNATIVES: usize = 16;

fn canonical_inferred_alternatives(
    mut alternatives: Vec<InferredAlternative>,
) -> Option<Vec<InferredAlternative>> {
    alternatives.sort_by_key(ToString::to_string);
    alternatives.dedup();
    (alternatives.len() <= MAX_INFERRED_ALTERNATIVES).then_some(alternatives)
}

/// Derives the function's correlated schemes from its first direct `+`, `-`,
/// or `*` of two parameters, narrowed by the operations composed with it. A
/// scheme's result is that operation's result, which the composed operations
/// share for every family that survives, so the schemes describe the function
/// only when every value it can return is one of those operations on the same
/// operands; otherwise the function keeps the result its body is checked to
/// produce.
fn inferred_overload_alternatives(
    parameters: &[Parameter],
    body: &Expr,
    constraints: &ParameterConstraints,
) -> Result<Vec<InferredAlternative>, SourceError> {
    let (operators, operands) = if let Some(operands) = constraints.plus_operands.first() {
        let mut operators = vec![Binary::Add];
        if !constraints.subtract_operands.is_empty() {
            operators.push(Binary::Subtract);
        }
        if !constraints.multiply_operands.is_empty() {
            operators.push(Binary::Multiply);
        }
        (operators, operands)
    } else if let Some(operands) = constraints.subtract_operands.first() {
        (vec![Binary::Subtract], operands)
    } else if let Some(operands) = constraints.multiply_operands.first() {
        (vec![Binary::Multiply], operands)
    } else {
        return Ok(Vec::new());
    };
    let alternatives = operation_alternatives(parameters, constraints)?;
    let results_are_operation =
        !constraints.returns && results_are_operation(body, &operators, operands, parameters);
    Ok(if results_are_operation {
        alternatives
    } else {
        Vec::new()
    })
}

/// Whether every value `expression` can produce as a function result is
/// `left operator right` over the collected operands, for one of `operators`. A `recur` produces the
/// function's own result, and the nil branch of a guard on an operand is
/// covered by its own scheme when that scheme can be derived.
fn results_are_operation(
    expression: &Expr,
    operators: &[Binary],
    operands: &PlusOperands,
    parameters: &[Parameter],
) -> bool {
    super::stack::grow(|| {
        let recurse =
            |expression| results_are_operation(expression, operators, operands, parameters);
        match &expression.kind {
            ExprKind::Binary {
                left,
                operator: actual,
                right,
            } if operators.iter().any(|operator| {
                std::mem::discriminant(actual) == std::mem::discriminant(operator)
            }) =>
            {
                matches!(
                    (&left.kind, &right.kind),
                    (ExprKind::Name(left), ExprKind::Name(right))
                        if *left == operands.left && *right == operands.right
                )
            }
            ExprKind::Recur(_) => true,
            ExprKind::Block(values) => values.last().is_some_and(recurse),
            ExprKind::If {
                condition,
                then_branch,
                else_branch: Some(else_branch),
            } => {
                let covered_nil_branch = matches!(operators.first(), Some(Binary::Add))
                    .then(|| operand_nil_branch(condition, then_branch, else_branch, operands))
                    .flatten()
                    .filter(|branch| alternative_branch_result(branch, parameters).is_some());
                match covered_nil_branch {
                    Some(branch) if std::ptr::eq(branch, then_branch.as_ref()) => {
                        recurse(else_branch)
                    }
                    Some(_) => recurse(then_branch),
                    None => recurse(then_branch) && recurse(else_branch),
                }
            }
            _ => false,
        }
    })
}

/// The branch taken when an operand is nil, for a condition comparing that
/// operand with `nil`.
fn operand_nil_branch<'a>(
    condition: &Expr,
    then_branch: &'a Expr,
    else_branch: &'a Expr,
    operands: &PlusOperands,
) -> Option<&'a Expr> {
    let ExprKind::Binary {
        left,
        operator,
        right,
    } = &condition.kind
    else {
        return None;
    };
    let ((ExprKind::Name(parameter), ExprKind::Value(Value::Nil))
    | (ExprKind::Value(Value::Nil), ExprKind::Name(parameter))) = (&left.kind, &right.kind)
    else {
        return None;
    };
    if *parameter != operands.left && *parameter != operands.right {
        return None;
    }
    match operator {
        Binary::Equal => Some(then_branch),
        Binary::NotEqual => Some(else_branch),
        _ => None,
    }
}

fn operation_alternatives(
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
) -> Result<Vec<InferredAlternative>, SourceError> {
    if !constraints.plus_operands.is_empty() {
        let Some(alternatives) =
            canonical_inferred_alternatives(inferred_plus_alternatives(parameters, constraints)?)
        else {
            return Ok(Vec::new());
        };
        let alternatives = compose_inferred_alternatives(
            alternatives,
            parameters,
            constraints,
            !constraints.subtract_operands.is_empty(),
            !constraints.multiply_operands.is_empty(),
        )?;
        return Ok(add_simple_nil_guard_alternatives(
            alternatives,
            parameters,
            constraints,
        ));
    }
    if !constraints.subtract_operands.is_empty() {
        return Ok(
            canonical_inferred_alternatives(inferred_subtract_alternatives(
                parameters,
                constraints,
            )?)
            .unwrap_or_default(),
        );
    }
    Ok(
        canonical_inferred_alternatives(inferred_multiply_alternatives(parameters, constraints)?)
            .unwrap_or_default(),
    )
}

fn add_simple_nil_guard_alternatives(
    mut alternatives: Vec<InferredAlternative>,
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
) -> Vec<InferredAlternative> {
    for partition in &constraints.nil_partitions {
        let Some(result) = alternative_branch_result(&partition.nil_branch, parameters) else {
            continue;
        };
        let inputs = parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                if parameter.name == partition.parameter {
                    AlternativeType::concrete(Type::Nil)
                } else {
                    AlternativeType::variable(AlternativeVariable::new(index))
                }
            })
            .collect();
        alternatives.push(InferredAlternative::new(inputs, result));
        let Some(canonical) = canonical_inferred_alternatives(alternatives) else {
            return Vec::new();
        };
        alternatives = canonical;
    }
    alternatives
}

fn alternative_branch_result(
    expression: &Expr,
    parameters: &[Parameter],
) -> Option<AlternativeType> {
    super::stack::grow(|| match &expression.kind {
        ExprKind::Value(value) => Some(AlternativeType::concrete(value_type(value))),
        ExprKind::Name(name) => parameters
            .iter()
            .position(|parameter| parameter.name == *name)
            .map(|index| AlternativeType::variable(AlternativeVariable::new(index))),
        ExprKind::Block(values) if values.len() == 1 => {
            alternative_branch_result(&values[0], parameters)
        }
        _ => None,
    })
}

fn compose_inferred_alternatives(
    mut alternatives: Vec<InferredAlternative>,
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
    include_subtract: bool,
    include_multiply: bool,
) -> Result<Vec<InferredAlternative>, SourceError> {
    let mut requirements = Vec::new();
    if include_subtract {
        requirements.push(inferred_subtract_alternatives(parameters, constraints)?);
    }
    if include_multiply {
        requirements.push(inferred_multiply_alternatives(parameters, constraints)?);
    }
    for required in requirements {
        alternatives.retain(|candidate| {
            let (actuals, _) = candidate.substitute(&HashMap::new());
            required.iter().any(|alternative| {
                alternative.accepts_constraints(&actuals.iter().map(Some).collect::<Vec<_>>())
            })
        });
        let Some(canonical) = canonical_inferred_alternatives(alternatives) else {
            return Ok(Vec::new());
        };
        alternatives = canonical;
    }
    Ok(alternatives)
}

/// Derives the finite `+` schemes for a direct pair of unannotated parameters.
///
/// The schemes are created from the body, then narrowed only with other
/// body-derived singleton facts. They remain private here; callable metadata
/// publishes them in the following implementation stage.
fn inferred_plus_alternatives(
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
) -> Result<Vec<InferredAlternative>, SourceError> {
    let Some(first) = constraints.plus_operands.first() else {
        return Ok(Vec::new());
    };
    let mut alternatives = plus_alternatives();
    for operands in &constraints.plus_operands {
        let left = constraints.requirements[&operands.left].value_type.clone();
        let right = constraints.requirements[&operands.right].value_type.clone();
        let compatible = plus_alternatives()
            .into_iter()
            .filter(|alternative| {
                alternative.accepts_constraints(&[
                    (!matches!(left, Type::Unknown)).then_some(&left),
                    (!matches!(right, Type::Unknown)).then_some(&right),
                ])
            })
            .collect::<Vec<_>>();
        if compatible.is_empty() {
            return Err(SourceError::semantic(
                SourceErrorCode::NoMatchingOverload,
                "no inferred `+` alternative satisfies the function body requirements",
                operands.span.clone(),
            ));
        }
        if operands.left == first.left && operands.right == first.right {
            alternatives.retain(|alternative| compatible.contains(alternative));
        }
    }
    let left_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.left)
        .expect("collected plus operand is a function parameter");
    let right_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.right)
        .expect("collected plus operand is a function parameter");
    Ok(alternatives
        .into_iter()
        .map(|alternative| {
            InferredAlternative::new(
                parameters
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        if index == left_index {
                            alternative.parameter_types()[0].clone()
                        } else if index == right_index {
                            alternative.parameter_types()[1].clone()
                        } else {
                            AlternativeType::concrete(Type::universal())
                        }
                    })
                    .collect(),
                alternative.result_type().clone(),
            )
        })
        .collect())
}

fn plus_alternatives() -> Vec<InferredAlternative> {
    let variable = |index| AlternativeType::variable(AlternativeVariable::new(index));
    vec![
        InferredAlternative::new(
            vec![
                AlternativeType::concrete(Type::Num),
                AlternativeType::concrete(Type::Num),
            ],
            AlternativeType::concrete(Type::Num),
        ),
        InferredAlternative::new(
            vec![AlternativeType::concrete(Type::Str), variable(0)],
            AlternativeType::concrete(Type::Str),
        ),
        InferredAlternative::new(
            vec![
                AlternativeType::list(variable(0)),
                AlternativeType::list(variable(1)),
            ],
            AlternativeType::list(AlternativeType::union([variable(0), variable(1)])),
        ),
        InferredAlternative::new(
            vec![
                AlternativeType::concrete(Type::Bytes),
                AlternativeType::concrete(Type::Bytes),
            ],
            AlternativeType::concrete(Type::Bytes),
        ),
        InferredAlternative::new(
            vec![
                AlternativeType::map(variable(0), variable(1)),
                AlternativeType::map(variable(2), variable(3)),
            ],
            AlternativeType::map(
                AlternativeType::union([variable(0), variable(2)]),
                AlternativeType::union([variable(1), variable(3)]),
            ),
        ),
    ]
}

fn inferred_subtract_alternatives(
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
) -> Result<Vec<InferredAlternative>, SourceError> {
    let Some(first) = constraints.subtract_operands.first() else {
        return Ok(Vec::new());
    };
    let mut alternatives = subtract_alternatives();
    for operands in &constraints.subtract_operands {
        let left = constraints.requirements[&operands.left].value_type.clone();
        let right = constraints.requirements[&operands.right].value_type.clone();
        let compatible = subtract_alternatives()
            .into_iter()
            .filter(|alternative| {
                alternative.accepts_constraints(&[
                    (!matches!(left, Type::Unknown)).then_some(&left),
                    (!matches!(right, Type::Unknown)).then_some(&right),
                ])
            })
            .collect::<Vec<_>>();
        if compatible.is_empty() {
            return Err(SourceError::semantic(
                SourceErrorCode::NoMatchingOverload,
                "no inferred `-` alternative satisfies the function body requirements",
                operands.span.clone(),
            ));
        }
        if operands.left == first.left && operands.right == first.right {
            alternatives.retain(|alternative| compatible.contains(alternative));
        }
    }
    let left_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.left)
        .expect("collected subtract operand is a function parameter");
    let right_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.right)
        .expect("collected subtract operand is a function parameter");
    Ok(alternatives
        .into_iter()
        .map(|alternative| {
            InferredAlternative::new(
                parameters
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        if index == left_index {
                            alternative.parameter_types()[0].clone()
                        } else if index == right_index {
                            alternative.parameter_types()[1].clone()
                        } else {
                            AlternativeType::concrete(Type::universal())
                        }
                    })
                    .collect(),
                alternative.result_type().clone(),
            )
        })
        .collect())
}

fn subtract_alternatives() -> Vec<InferredAlternative> {
    let variable = |index| AlternativeType::variable(AlternativeVariable::new(index));
    vec![
        InferredAlternative::new(
            vec![
                AlternativeType::concrete(Type::Num),
                AlternativeType::concrete(Type::Num),
            ],
            AlternativeType::concrete(Type::Num),
        ),
        InferredAlternative::new(
            vec![
                AlternativeType::map(variable(0), variable(1)),
                AlternativeType::hashable_map_key(),
            ],
            AlternativeType::map(variable(0), variable(1)),
        ),
    ]
}

fn inferred_multiply_alternatives(
    parameters: &[Parameter],
    constraints: &ParameterConstraints,
) -> Result<Vec<InferredAlternative>, SourceError> {
    let Some(first) = constraints.multiply_operands.first() else {
        return Ok(Vec::new());
    };
    let mut alternatives = multiply_alternatives();
    for operands in &constraints.multiply_operands {
        let left = constraints.requirements[&operands.left].value_type.clone();
        let right = constraints.requirements[&operands.right].value_type.clone();
        let compatible = multiply_alternatives()
            .into_iter()
            .filter(|alternative| {
                alternative.accepts_constraints(&[
                    (!matches!(left, Type::Unknown)).then_some(&left),
                    (!matches!(right, Type::Unknown)).then_some(&right),
                ])
            })
            .collect::<Vec<_>>();
        if compatible.is_empty() {
            return Err(SourceError::semantic(
                SourceErrorCode::NoMatchingOverload,
                "no inferred `*` alternative satisfies the function body requirements",
                operands.span.clone(),
            ));
        }
        if operands.left == first.left && operands.right == first.right {
            alternatives.retain(|alternative| compatible.contains(alternative));
        }
    }
    let left_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.left)
        .expect("collected multiply operand is a function parameter");
    let right_index = parameters
        .iter()
        .position(|parameter| parameter.name == first.right)
        .expect("collected multiply operand is a function parameter");
    Ok(alternatives
        .into_iter()
        .map(|alternative| {
            InferredAlternative::new(
                parameters
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        if index == left_index {
                            alternative.parameter_types()[0].clone()
                        } else if index == right_index {
                            alternative.parameter_types()[1].clone()
                        } else {
                            AlternativeType::concrete(Type::universal())
                        }
                    })
                    .collect(),
                alternative.result_type().clone(),
            )
        })
        .collect())
}

fn multiply_alternatives() -> Vec<InferredAlternative> {
    vec![
        InferredAlternative::new(
            vec![
                AlternativeType::concrete(Type::Num),
                AlternativeType::concrete(Type::Num),
            ],
            AlternativeType::concrete(Type::Num),
        ),
        InferredAlternative::new(
            vec![
                AlternativeType::concrete(Type::Str),
                AlternativeType::concrete(Type::Num),
            ],
            AlternativeType::concrete(Type::Str),
        ),
    ]
}

impl CheckedExpression {
    fn falls_through(value_type: Type) -> Self {
        Self {
            value_type,
            continuation: Continuation::FallsThrough,
        }
    }

    fn terminates(value_type: Type) -> Self {
        Self {
            value_type,
            continuation: Continuation::Terminates,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn collect_imports(expression: &Expr, scan: &mut ImportScan) {
    super::stack::grow(|| {
        if let ExprKind::Call { callee, arguments } = &expression.kind
            && matches!(&callee.kind, ExprKind::Name(name) if name == "import")
        {
            let mut dynamic = false;
            for argument in arguments {
                if let CallArgument::Positional(Expr {
                    kind: ExprKind::Value(Value::Str(name)),
                    ..
                }) = argument
                {
                    scan.names.push(name.to_string());
                } else {
                    dynamic = true;
                }
            }
            if dynamic {
                scan.dynamic.push(expression.span.clone());
            }
        }
        expression.for_each_child(|child| collect_imports(child, scan));
    });
}

fn analyze_expressions(
    expressions: &[Expr],
    imports: ImportSnapshots,
    session: &SessionSnapshot,
    module_name: Option<&str>,
) -> Result<SemanticAnalysis, SourceError> {
    // A module's own resource and enum types carry the name it is imported
    // by, as its native resources and enum values do at runtime, so its
    // matches on them agree with an importer's.
    let in_module = |mut identity: super::semantic::NominalIdentity| {
        if let Some(name) = module_name {
            identity.set_runtime_module(name.into());
        }
        identity
    };
    let mut environment = Environment::with_imports_and_session(imports, session);
    if let Some(builtins) = environment.import_snapshot("slug.builtin") {
        environment.install_implicit_callable_export(&builtins, "chan");
        environment.install_implicit_callable_export(&builtins, "close");
    }
    let mut exports = HashMap::new();
    let mut types = HashMap::new();
    let mut resource_names = HashSet::new();
    let mut enum_names = HashSet::new();
    for expression in expressions {
        if let ExprKind::Resource { exported, name, .. } = &expression.kind {
            if !resource_names.insert(name.clone()) {
                return Err(SourceError::semantic(
                    SourceErrorCode::DuplicateName,
                    format!("duplicate resource type `{name}`"),
                    expression.span.clone(),
                )
                .with_extent(expression));
            }
            let identity = in_module(ResourceIdentity::declared(
                expression.span.path.as_ref(),
                name.clone(),
            ));
            environment.declare_type(
                name.clone(),
                super::environment::TypeMember::Resource(identity),
            );
            let _ = exported;
        }
    }
    for expression in expressions {
        if let ExprKind::Enum { name, cases, .. } = &expression.kind {
            if !enum_names.insert(name.clone()) || resource_names.contains(name) {
                return Err(SourceError::semantic(
                    SourceErrorCode::DuplicateName,
                    format!("duplicate type `{name}`"),
                    expression.span.clone(),
                )
                .with_extent(expression));
            }
            let identity = in_module(super::semantic::EnumIdentity::declared(
                expression.span.path.as_ref(),
                name.clone(),
            ));
            environment.declare_type(
                name.clone(),
                super::environment::TypeMember::Enum {
                    identity: identity.clone(),
                    cases: cases.clone(),
                },
            );
            let mut binding = SemanticBinding::value(Type::Map(None));
            for case in cases {
                binding.members.insert(
                    case.clone(),
                    SemanticBinding::value(Type::Enum(identity.clone())),
                );
            }
            environment.declare(name.clone(), binding);
        }
    }
    resolve_type_aliases(expressions, &mut environment, &resource_names, &enum_names)?;
    for expression in expressions {
        let name = match &expression.kind {
            ExprKind::Declare {
                pattern: Pattern::Binding(name),
                ..
            }
            | ExprKind::Foreign { name, .. } => Some(name),
            _ => None,
        };
        if let Some(name) = name
            && (enum_names.contains(name) || resource_names.contains(name))
        {
            return Err(SourceError::semantic(
                SourceErrorCode::DuplicateName,
                format!("enum type `{name}` conflicts with a value declaration"),
                expression.span.clone(),
            )
            .with_extent(expression));
        }
    }
    infer_nil_starts(expressions, &environment, &[], false);
    for expression in expressions {
        let _ = check_expression_with_flow(expression, &mut environment, &[])?;
        record_exports(expression, &environment, &mut exports, &mut types);
    }
    let schemas = exported_schemas(&exports, &types, &environment);
    Ok(environment.analysis(ModuleSnapshot {
        exports,
        types,
        schemas,
    }))
}

/// The schemas that exported values and type aliases name, so importers can
/// check fields of a struct whose schema the module keeps private.
fn exported_schemas(
    exports: &HashMap<String, SemanticBinding>,
    types: &HashMap<String, super::environment::TypeMember>,
    environment: &Environment,
) -> Vec<SemanticBinding> {
    fn collect(value_type: &Type, identities: &mut Vec<super::semantic::SchemaIdentity>) {
        match value_type {
            Type::Struct(Some(identity)) => {
                if !identities.contains(identity) {
                    identities.push(identity.clone());
                }
            }
            Type::List(Some(element))
            | Type::Task(Some(element))
            | Type::Channel(Some(element)) => {
                collect(element, identities);
            }
            Type::Map(Some((key, value))) => {
                collect(key, identities);
                collect(value, identities);
            }
            Type::Function(Some(values)) | Type::Tuple(values) | Type::Union(values) => {
                for value in values {
                    collect(value, identities);
                }
            }
            _ => {}
        }
    }
    let mut identities = Vec::new();
    for binding in exports.values() {
        collect(&binding.value_type, &mut identities);
        for signature in &binding.callables {
            collect(&signature.result, &mut identities);
            for parameter in &signature.parameters {
                collect(&parameter.value_type, &mut identities);
            }
        }
    }
    for member in types.values() {
        if let super::environment::TypeMember::Alias(value_type) = member {
            collect(value_type, &mut identities);
        }
    }
    identities
        .iter()
        .filter_map(|identity| environment.schema_by_identity(identity))
        .collect()
}

fn resolve_type_aliases(
    expressions: &[Expr],
    environment: &mut Environment,
    resource_names: &HashSet<String>,
    enum_names: &HashSet<String>,
) -> Result<(), SourceError> {
    let mut aliases = HashMap::new();
    for expression in expressions {
        let ExprKind::TypeAlias {
            name, annotation, ..
        } = &expression.kind
        else {
            continue;
        };
        if aliases
            .insert(name.clone(), (annotation, expression.span.clone()))
            .is_some()
        {
            return Err(SourceError::semantic(
                SourceErrorCode::DuplicateName,
                format!("duplicate type `{name}`"),
                expression.span.clone(),
            )
            .with_extent(expression));
        }
    }
    for (name, (_, span)) in &aliases {
        if resource_names.contains(name) || enum_names.contains(name) {
            return Err(SourceError::semantic(
                SourceErrorCode::DuplicateName,
                format!("duplicate type `{name}`"),
                span.clone(),
            ));
        }
    }
    let mut visiting = Vec::new();
    let mut resolved = HashSet::new();
    for expression in expressions {
        let ExprKind::TypeAlias { name, .. } = &expression.kind else {
            continue;
        };
        resolve_type_alias(name, &aliases, environment, &mut visiting, &mut resolved)?;
    }
    Ok(())
}

fn resolve_type_alias(
    name: &str,
    aliases: &HashMap<String, (&TypeAnnotation, SourceSpan)>,
    environment: &mut Environment,
    visiting: &mut Vec<String>,
    resolved: &mut HashSet<String>,
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        if resolved.contains(name) {
            return Ok(());
        }
        let (annotation, span) = aliases
            .get(name)
            .expect("alias name came from the alias declaration table");
        if let Some(start) = visiting.iter().position(|entry| entry == name) {
            let mut chain = visiting[start..].to_vec();
            chain.push(name.into());
            return Err(SourceError::semantic(
                SourceErrorCode::RecursiveTypeAlias,
                format!("recursive type alias: {}", chain.join(" -> ")),
                span.clone(),
            ));
        }
        visiting.push(name.into());
        let mut dependencies = Vec::new();
        alias_dependencies(annotation, aliases, &mut dependencies)?;
        for dependency in dependencies {
            resolve_type_alias(&dependency, aliases, environment, visiting, resolved)?;
        }
        // Schema bindings are value declarations, so `struct<S>` must keep its
        // unresolved schema reference until the alias is used after `S` exists.
        // Resource and enum identities, by contrast, belong to the type namespace
        // and are available while aliases are collected.
        let value_type = resolve_resource_references(
            resolve_annotation(annotation, &[], span)?,
            span,
            environment,
        )?;
        environment.declare_type(
            name.into(),
            super::environment::TypeMember::Alias(value_type),
        );
        visiting.pop();
        resolved.insert(name.into());
        Ok(())
    })
}

fn alias_dependencies(
    annotation: &TypeAnnotation,
    aliases: &HashMap<String, (&TypeAnnotation, SourceSpan)>,
    dependencies: &mut Vec<String>,
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        match annotation {
            TypeAnnotation::Name(name) => {
                if aliases.contains_key(name) {
                    dependencies.push(name.clone());
                }
            }
            TypeAnnotation::Apply { name, arguments } => {
                if aliases.contains_key(name) {
                    return Err(SourceError::semantic(
                        SourceErrorCode::InvalidTypeArguments,
                        format!("type alias `{name}` cannot accept type arguments"),
                        aliases[name].1.clone(),
                    ));
                }
                for argument in arguments {
                    alias_dependencies(argument, aliases, dependencies)?;
                }
            }
            TypeAnnotation::Tuple(elements) | TypeAnnotation::Union(elements) => {
                for element in elements {
                    alias_dependencies(element, aliases, dependencies)?;
                }
            }
        }
        Ok(())
    })
}

fn function_type(
    type_parameters: &[String],
    parameters: &[Parameter],
    result: Option<&TypeAnnotation>,
    span: &SourceSpan,
    environment: &Environment,
    inferred_parameters: Option<&HashMap<String, Type>>,
) -> Result<CallableSignature, SourceError> {
    Ok(CallableSignature {
        generic_arity: type_parameters.len(),
        parameters: parameters
            .iter()
            .map(|parameter| {
                Ok(CallableParameter {
                    label: (!parameter.discard).then(|| parameter.name.clone()),
                    value_type: parameter
                        .annotation
                        .as_ref()
                        .map(|annotation| {
                            resolve_static_annotation(
                                annotation,
                                type_parameters,
                                span,
                                environment,
                            )
                        })
                        .transpose()?
                        .or_else(|| {
                            (!parameter.discard && !parameter.variadic)
                                .then(|| {
                                    inferred_parameters
                                        .and_then(|parameters| parameters.get(&parameter.name))
                                        .cloned()
                                })
                                .flatten()
                        })
                        .unwrap_or_else(Type::universal),
                    has_default: parameter.default.is_some(),
                    variadic: parameter.variadic,
                })
            })
            .collect::<Result<Vec<_>, SourceError>>()?,
        result: result
            .map(|annotation| {
                resolve_static_annotation(annotation, type_parameters, span, environment)
            })
            .transpose()?
            .unwrap_or(Type::Unknown),
        inferred_alternatives: Vec::new(),
    })
}

fn callable_signature(
    expression: &Expr,
    span: &SourceSpan,
    environment: &Environment,
) -> Result<Option<CallableSignature>, SourceError> {
    let ExprKind::Function {
        type_parameters,
        parameters,
        return_annotation,
        body,
    } = &expression.kind
    else {
        return Ok(None);
    };
    let constraints = parameter_constraints(parameters, body, environment)?;
    let inferred_alternatives = inferred_overload_alternatives(parameters, body, &constraints)?;
    let inferred_parameters = solved_parameter_types(&constraints);
    let mut signature = function_type(
        type_parameters,
        parameters,
        return_annotation.as_ref(),
        span,
        environment,
        Some(&inferred_parameters),
    )?;
    signature.inferred_alternatives = inferred_alternatives;
    Ok(Some(signature))
}

fn check_function_body(
    parameters: &[Parameter],
    signature: &CallableSignature,
    return_annotation: Option<&TypeAnnotation>,
    body: &Expr,
    environment: &Environment,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    super::stack::grow(|| {
        let mut scoped = environment.clone();
        scoped.forget_mutable_facts();
        scoped.enter_scope();
        for (parameter, signature_parameter) in parameters.iter().zip(&signature.parameters) {
            let parameter_type = &signature_parameter.value_type;
            if !parameter.discard {
                // An unannotated parameter whose body supplies no type accepts
                // `any|nil`, but inside the body that is the checker's uncertainty,
                // not a declared type: it stays dynamic, so a result derived from
                // it, such as an accumulator returned by a recursive helper, does
                // not fail a caller's annotation.
                let inferred_nothing =
                    parameter.annotation.is_none() && *parameter_type == Type::universal();
                let element_type = if inferred_nothing {
                    Type::Unknown
                } else {
                    parameter_type.clone()
                };
                let binding_type = if parameter.variadic {
                    Type::List(Some(Box::new(element_type)))
                } else {
                    element_type
                };
                scoped.declare(parameter.name.clone(), SemanticBinding::value(binding_type));
            }
            if let Some(default) = &parameter.default {
                let actual = check_expression(default, &mut scoped, type_parameters)?;
                require(parameter_type, &actual, &default.span)?;
            }
        }
        let expected = return_annotation
            .map(|annotation| {
                resolve_static_annotation(annotation, type_parameters, &body.span, environment)
            })
            .transpose()?;
        scoped.enter_function(expected.clone());
        let completed = check_expression_with_flow(body, &mut scoped, type_parameters)?.value_type;
        if let Some(expected) = &expected {
            let result = result_expression(body);
            let completed = as_expected_tuple(expected, result, completed.clone(), &scoped);
            require(expected, &completed, &result.span).map_err(|error| {
                let error = error.with_extent(result);
                if *expected == Type::Nil {
                    end_with_nil(error, result)
                } else {
                    error
                }
            })?;
        }
        // The function produces its body's value or any `return`'s, which were
        // each checked against `expected` where they occur.
        Ok(Type::union(
            std::iter::once(completed).chain(scoped.returned_types()),
        ))
    })
}

/// Offers to end a body declared to produce `nil` with `nil`. It is a fix
/// when the body's value comes only from assignments, which a reader may not
/// expect to have a value, and otherwise a suggestion, since the declared
/// result may be what is wrong.
fn end_with_nil(error: SourceError, result: &Expr) -> SourceError {
    let edit = SourceEdit {
        range: SourceRange {
            start: result.end,
            end: result.end,
        },
        text: "; nil".into(),
    };
    if produces_only_assigned_values(result) {
        error.with_fix(
            "end the body with `nil`: an assignment's value is the value assigned",
            edit,
        )
    } else {
        error.with_suggestion("end the body with `nil` to produce nothing", edit)
    }
}

/// Whether an expression's value can only be an assignment's value or `nil`.
fn produces_only_assigned_values(expression: &Expr) -> bool {
    super::stack::grow(|| match &expression.kind {
        ExprKind::Assign { .. } | ExprKind::Value(Value::Nil) => true,
        ExprKind::Block(values) => values.last().is_none_or(produces_only_assigned_values),
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            produces_only_assigned_values(then_branch)
                && else_branch
                    .as_deref()
                    .is_none_or(produces_only_assigned_values)
        }
        ExprKind::Match { cases, .. } => cases
            .iter()
            .all(|case| produces_only_assigned_values(&case.value)),
        _ => false,
    })
}

/// The expression whose value a body produces: the last expression of each
/// nested block.
fn result_expression(mut body: &Expr) -> &Expr {
    while let ExprKind::Block(values) = &body.kind
        && let Some(last) = values.last()
    {
        body = last;
    }
    body
}

fn record_exports(
    expression: &Expr,
    environment: &Environment,
    exports: &mut HashMap<String, SemanticBinding>,
    types: &mut HashMap<String, super::environment::TypeMember>,
) {
    match &expression.kind {
        ExprKind::Declare {
            exported: true,
            pattern,
            ..
        } => {
            let mut names = Vec::new();
            pattern_binding_names(pattern, &mut names);
            for name in names {
                if let Some(binding) = environment.lookup(name) {
                    exports.insert(name.clone(), binding.clone());
                }
            }
        }
        ExprKind::Foreign {
            exported: true,
            name,
            ..
        } => {
            if let Some(binding) = environment.lookup(name) {
                exports.insert(name.clone(), binding.clone());
            }
        }
        ExprKind::Resource {
            exported: true,
            name,
            ..
        } => {
            if let Some(identity) = environment.resource_type(name) {
                types.insert(
                    name.clone(),
                    super::environment::TypeMember::Resource(identity),
                );
            }
        }
        ExprKind::Enum {
            exported: true,
            name,
            cases,
            ..
        } => {
            if let Some(binding) = environment.lookup(name) {
                exports.insert(name.clone(), binding.clone());
            }
            if let Some(super::environment::TypeMember::Enum { identity, .. }) =
                environment.type_member(name)
            {
                types.insert(
                    name.clone(),
                    super::environment::TypeMember::Enum {
                        identity: identity.clone(),
                        cases: cases.clone(),
                    },
                );
            }
        }
        ExprKind::TypeAlias {
            exported: true,
            name,
            ..
        } => {
            // An alias's `struct<S>` stays unresolved until a use resolves
            // it, but an importer has no binding `S`, so the export carries
            // the schema identity resolved here.
            let member = match environment.type_member(name) {
                Some(super::environment::TypeMember::Alias(value_type)) => {
                    super::semantic::resolve_schema_references(
                        value_type.clone(),
                        &expression.span,
                        environment,
                    )
                    .ok()
                    .map(super::environment::TypeMember::Alias)
                }
                member => member.cloned(),
            };
            if let Some(member) = member {
                types.insert(name.clone(), member);
            }
        }
        _ => {}
    }
}

fn pattern_binding_names<'a>(pattern: &'a Pattern, names: &mut Vec<&'a String>) {
    super::stack::grow(|| match pattern {
        Pattern::Binding(name) | Pattern::At { name, .. } => names.push(name),
        Pattern::List { items, .. } => {
            for item in items {
                pattern_binding_names(item, names);
            }
        }
        Pattern::Map { entries, .. } => {
            for (_, value) in entries {
                pattern_binding_names(value, names);
            }
        }
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => {}
    });
}

fn check_expression(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    super::stack::grow(|| {
        let value_type = check_expression_inner(expression, environment, type_parameters)
            .map_err(|error| error.claimed_by(expression))?;
        environment.record_expression_type(expression.span.clone(), value_type.clone());
        Ok(value_type)
    })
}

#[allow(clippy::too_many_lines)]
fn check_expression_inner(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    super::stack::grow(|| {
        match &expression.kind {
            ExprKind::Declare {
                pattern,
                annotation,
                value,
                mutable,
                ..
            } => {
                let callable = if let Pattern::Binding(name) = pattern {
                    callable_signature(value, &expression.span, environment)?
                        .map(|signature| (name.clone(), signature))
                } else {
                    None
                };
                if let Some((name, signature)) = &callable {
                    environment.declare_callable(
                        name.clone(),
                        signature.clone(),
                        &expression.span,
                    )?;
                }
                let actual = check_expression(value, environment, type_parameters)?;
                if let Some((name, signature)) = &callable
                    && let Type::Function(Some(types)) = &actual
                    && let Some(result) = types.first()
                {
                    environment.update_callable_result(name, &signature.identity(), result.clone());
                }
                let declared = annotation
                    .as_ref()
                    .map(|annotation| {
                        resolve_static_annotation(
                            annotation,
                            type_parameters,
                            &expression.span,
                            environment,
                        )
                    })
                    .transpose()?;
                if let Some(expected) = &declared {
                    let actual = as_expected_tuple(expected, value, actual.clone(), environment);
                    require(expected, &actual, &expression.span)
                        .map_err(|error| explain_nil_start(error, value, environment))?;
                }
                if callable.is_none() {
                    let mut binding = value_binding(value, &actual, environment, type_parameters)?;
                    if matches!(value.kind, ExprKind::StructSchema(_))
                        && let Pattern::Binding(name) = pattern
                        && let Some(identity) = &mut binding.schema_identity
                    {
                        identity.name.clone_from(name);
                    }
                    if let Some(declared) = declared {
                        binding.value_type = declared;
                    }
                    bind_semantic_pattern(pattern, &binding, environment);
                    if *mutable {
                        let mut names = Vec::new();
                        pattern_binding_names(pattern, &mut names);
                        for name in names {
                            environment.declare_mutable(name, &expression.span);
                        }
                    }
                }
                Ok(actual)
            }
            ExprKind::Foreign {
                name, signature, ..
            } => {
                let callable = function_type(
                    &signature.type_parameters,
                    &signature.parameters,
                    signature.return_annotation.as_ref(),
                    &expression.span,
                    environment,
                    None,
                )?;
                environment.record_foreign(
                    expression.span.clone(),
                    callable.identity(),
                    super::environment::foreign_resource_signature(&callable),
                );
                let value_type = function_value_type(&callable);
                environment.declare_callable(name.clone(), callable, &expression.span)?;
                for parameter in &signature.parameters {
                    if let Some(default) = &parameter.default {
                        let actual =
                            check_expression(default, environment, &signature.type_parameters)?;
                        if let Some(annotation) = &parameter.annotation {
                            let expected = resolve_static_annotation(
                                annotation,
                                &signature.type_parameters,
                                &default.span,
                                environment,
                            )?;
                            require(&expected, &actual, &default.span)?;
                        }
                    }
                }
                Ok(value_type)
            }
            ExprKind::Function {
                type_parameters: function_type_parameters,
                parameters,
                return_annotation,
                body,
            } => {
                let constraints = parameter_constraints(parameters, body, environment)?;
                let inferred_alternatives =
                    inferred_overload_alternatives(parameters, body, &constraints)?;
                let inferred_parameters = solved_parameter_types(&constraints);
                let mut signature = function_type(
                    function_type_parameters,
                    parameters,
                    return_annotation.as_ref(),
                    &expression.span,
                    environment,
                    Some(&inferred_parameters),
                )?;
                signature.inferred_alternatives = inferred_alternatives;
                environment.record_function(expression.span.clone(), signature.identity());
                let actual = check_function_body(
                    parameters,
                    &signature,
                    return_annotation.as_ref(),
                    body,
                    environment,
                    function_type_parameters,
                )?;
                signature.result = return_annotation
                    .as_ref()
                    .map(|annotation| {
                        resolve_static_annotation(
                            annotation,
                            function_type_parameters,
                            &body.span,
                            environment,
                        )
                    })
                    .transpose()?
                    .unwrap_or(actual);
                Ok(function_value_type(&signature))
            }
            ExprKind::Call { callee, arguments } => check_call(
                callee,
                arguments,
                expression,
                environment,
                type_parameters,
                None,
            ),
            ExprKind::TypeApply { callee, .. } => {
                check_expression(callee, environment, type_parameters)
            }
            ExprKind::StructSchema(fields) => {
                for field in fields {
                    if let Some(default) = &field.default {
                        let actual = check_expression(default, environment, type_parameters)?;
                        if let Some(annotation) = &field.annotation {
                            let expected = resolve_static_annotation(
                                annotation,
                                type_parameters,
                                &default.span,
                                environment,
                            )?;
                            require(&expected, &actual, &default.span)?;
                        }
                    }
                }
                Ok(Type::Schema)
            }
            ExprKind::Value(value) => Ok(value_type(value)),
            ExprKind::List(values) => {
                let mut elements = Vec::new();
                let mut lost_precision = false;
                // Whether every element's position is known: no spread of a list
                // whose length is not.
                let mut positional = true;
                for value in values {
                    match value {
                        ListElement::Value(value) => {
                            elements.push(check_expression(value, environment, type_parameters)?);
                        }
                        ListElement::Spread(value) => {
                            match check_expression(value, environment, type_parameters)? {
                                Type::Tuple(spread) => elements.extend(spread),
                                Type::List(Some(element)) => {
                                    positional = false;
                                    elements.push(*element);
                                }
                                _ => lost_precision = true,
                            }
                        }
                    }
                }
                // A literal whose elements differ in type is a tuple, so each
                // keeps its own: `[parts, headers][1]` is the map. One whose
                // elements share a type is a list of it.
                let mut distinct = elements.clone();
                distinct.sort_by_key(ToString::to_string);
                distinct.dedup();
                if positional && !lost_precision && distinct.len() > 1 {
                    return Ok(Type::Tuple(elements));
                }
                Ok(Type::List(
                    (!elements.is_empty() && !lost_precision)
                        .then(|| Box::new(Type::union(elements))),
                ))
            }
            ExprKind::Map(entries) => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for (key, value) in entries {
                    keys.push(check_expression(key, environment, type_parameters)?);
                    values.push(check_expression(value, environment, type_parameters)?);
                }
                Ok(Type::Map((!entries.is_empty()).then(|| {
                    (Box::new(Type::union(keys)), Box::new(Type::union(values)))
                })))
            }
            ExprKind::Block(values) => {
                infer_nil_starts(values, environment, type_parameters, true);
                let mut scoped = environment.clone();
                scoped.enter_scope();
                predeclare_functions(values, &mut scoped);
                let mut result = Type::Nil;
                for value in values {
                    result = check_expression(value, &mut scoped, type_parameters)?;
                }
                environment.merge_compatible_types(&scoped, &scoped);
                Ok(result)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expression(condition, environment, type_parameters)?;
                let (then_facts, else_facts) = condition_facts(condition, environment);
                let mut then_environment = environment.clone();
                apply_flow_facts(&mut then_environment, then_facts);
                let left = check_expression(then_branch, &mut then_environment, type_parameters)?;
                let mut else_environment = environment.clone();
                apply_flow_facts(&mut else_environment, else_facts);
                let right = else_branch
                    .as_ref()
                    .map(|branch| check_expression(branch, &mut else_environment, type_parameters))
                    .transpose()?
                    .unwrap_or(Type::Nil);
                environment.merge_compatible_types(&then_environment, &else_environment);
                Ok(Type::union([left, right]))
            }
            ExprKind::Binary {
                left,
                operator,
                right,
            } => {
                let left_type = check_expression(left, environment, type_parameters)?;
                if matches!(operator, Binary::Pipeline) {
                    return match &right.kind {
                        ExprKind::Call { callee, arguments } => check_call(
                            callee,
                            arguments,
                            right,
                            environment,
                            type_parameters,
                            Some(left_type),
                        ),
                        ExprKind::Name(_) | ExprKind::TypeApply { .. } | ExprKind::Index { .. } => {
                            check_call(
                                right,
                                &[],
                                right,
                                environment,
                                type_parameters,
                                Some(left_type),
                            )
                        }
                        _ => check_expression(right, environment, type_parameters),
                    };
                }
                if matches!(operator, Binary::And | Binary::Or) {
                    let mut right_environment = environment.clone();
                    let (then_facts, else_facts) = condition_facts(left, environment);
                    apply_flow_facts(
                        &mut right_environment,
                        if matches!(operator, Binary::And) {
                            then_facts
                        } else {
                            else_facts
                        },
                    );
                    let right = check_expression(right, &mut right_environment, type_parameters)?;
                    return Ok(Type::union([left_type, right]));
                }
                let right = check_expression(right, environment, type_parameters)?;
                binary_result(*operator, &left_type, &right, &expression.span)
            }
            ExprKind::Prefix { operators, value } => {
                let mut result = check_expression(value, environment, type_parameters)?;
                for (operator, span) in operators.iter().rev() {
                    result = prefix_result(*operator, &result, span)?;
                }
                Ok(result)
            }
            ExprKind::Name(name) => Ok(environment
                .lookup(name)
                .map_or(Type::Unknown, |binding| binding.value_type.clone())),
            ExprKind::Assign { name, value } => {
                let actual = check_expression(value, environment, type_parameters)?;
                if environment.in_trial() {
                    environment.log_assignment(name, &value.span, &actual);
                }
                if let Some(binding) = environment
                    .lookup(name)
                    .filter(|binding| !(environment.in_trial() && binding.nil_start.is_some()))
                {
                    let expected = binding.declared.as_ref().unwrap_or(&binding.value_type);
                    require(expected, &actual, &value.span).map_err(|error| {
                        let error = error.with_extent(value);
                        if binding
                            .nil_start
                            .as_ref()
                            .is_some_and(|start| !start.assignments.is_empty())
                        {
                            explain_nil_start_assignment(error, name, binding, &actual)
                        } else {
                            explain_nil_start(error, value, environment)
                        }
                    })?;
                }
                if let Some(binding) = environment.lookup_mut(name) {
                    binding.callables.clear();
                    if !matches!(actual, Type::Unknown) {
                        binding.value_type = actual.clone();
                    }
                }
                Ok(actual)
            }
            ExprKind::Return { value } => {
                let actual = check_expression(value, environment, type_parameters)?;
                if let Some(expected) = environment.expected_return() {
                    let actual = as_expected_tuple(&expected, value, actual.clone(), environment);
                    require(&expected, &actual, &value.span).map_err(|error| {
                        explain_nil_start(error.with_extent(value), value, environment)
                    })?;
                }
                environment.record_return(actual.clone());
                Ok(actual)
            }
            ExprKind::Throw { value } => {
                check_expression(value, environment, type_parameters)?;
                Ok(Type::Never)
            }
            ExprKind::Defer { value, mode, .. } => {
                let handled = check_expression(value, environment, type_parameters)?;
                // An error handler that completes normally recovers, and its
                // value becomes the enclosing function's result.
                if matches!(mode, DeferMode::Error) {
                    if let Some(expected) = environment.expected_return() {
                        require(&expected, &handled, &value.span)
                            .map_err(|error| error.with_extent(value))?;
                    }
                    environment.record_return(handled);
                }
                Ok(Type::Nil)
            }
            ExprKind::Spawn(value) => {
                let result = check_expression(value, environment, type_parameters)?;
                Ok(Type::Task(Some(Box::new(result.widen_unknown()))))
            }
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    check_expression(limit, environment, type_parameters)?;
                }
                check_expression(body, environment, type_parameters)
            }
            ExprKind::Recur(arguments) => {
                for argument in arguments {
                    check_argument(argument, environment, type_parameters)?;
                }
                Ok(Type::Never)
            }
            ExprKind::Select(cases) => {
                let mut results = Vec::new();
                for case in cases {
                    match &case.kind {
                        SelectCaseKind::Receive(value)
                        | SelectCaseKind::After(value)
                        | SelectCaseKind::Await(value) => {
                            check_expression(value, environment, type_parameters)?;
                        }
                        SelectCaseKind::Send { channel, value } => {
                            check_expression(channel, environment, type_parameters)?;
                            check_expression(value, environment, type_parameters)?;
                        }
                        SelectCaseKind::Default => {}
                    }
                    results.push(
                        case.handler
                            .as_ref()
                            .map(|handler| {
                                let handler =
                                    check_expression(handler, environment, type_parameters)?;
                                Ok(callable_result_type(&handler))
                            })
                            .transpose()?
                            .unwrap_or(Type::Unknown),
                    );
                }
                Ok(Type::union(results))
            }
            ExprKind::Match { subject, cases } => {
                let subject_type = subject
                    .as_ref()
                    .map(|subject| check_expression(subject, environment, type_parameters))
                    .transpose()?
                    .unwrap_or_else(Type::universal);
                let mut enum_remaining = enum_cases(&subject_type, environment);
                let coverage_enabled =
                    enum_remaining.is_some() || is_closed_coverage_type(&subject_type);
                let mut remaining = coverage_enabled.then_some(subject_type.clone());
                if enum_remaining.is_some() {
                    remaining = None;
                }
                let mut results = Vec::new();
                let mut surviving_subject_types = Vec::new();
                // The type a value can still have on reaching each case, after
                // every earlier unguarded case that matches it whole.
                let mut unmatched = Some(subject_type.clone());
                for case in cases {
                    let reaching_type = unmatched.clone().unwrap_or_else(|| subject_type.clone());
                    let mut scoped = environment.clone();
                    scoped.enter_scope();
                    let mut constraints = Vec::new();
                    let mut irrefutable_constraints = Vec::new();
                    for pattern in &case.patterns {
                        let constraint =
                            check_case_pattern(pattern, &mut scoped, type_parameters, &case.span)?;
                        constraints.push(constraint.clone());
                        if is_irrefutable_pattern(&pattern.pattern) {
                            irrefutable_constraints.push(constraint.clone());
                        }
                        bind_case_pattern(
                            &pattern.pattern,
                            constraint.as_ref().unwrap_or(&reaching_type),
                            &mut scoped,
                        );
                    }
                    if case.guard.is_none() {
                        unmatched = case.patterns.iter().zip(&constraints).fold(
                            unmatched,
                            |unmatched, (pattern, constraint)| {
                                let unmatched = unmatched?;
                                match (&pattern.pattern, constraint) {
                                    (pattern, None) if is_irrefutable_pattern(pattern) => None,
                                    (pattern, Some(constraint))
                                        if is_irrefutable_pattern(pattern) =>
                                    {
                                        type_subtract(&unmatched, constraint)
                                    }
                                    (Pattern::Literal(value), _)
                                        if value.kind() == ValueKind::Nil =>
                                    {
                                        type_subtract(&unmatched, &Type::Nil)
                                    }
                                    _ => Some(unmatched),
                                }
                            },
                        );
                    }
                    let mut case_subject_type = None;
                    if let Some(Expr {
                        kind: ExprKind::Name(name),
                        ..
                    }) = subject.as_deref()
                    {
                        let narrowed = case
                            .patterns
                            .iter()
                            .zip(&constraints)
                            .filter_map(|(pattern, constraint)| {
                                constraint.clone().or_else(|| match &pattern.pattern {
                                    Pattern::Literal(value) => Some(value_type(value)),
                                    Pattern::Wildcard
                                    | Pattern::Binding(_)
                                    | Pattern::At { .. }
                                    | Pattern::List { .. }
                                    | Pattern::Map { .. }
                                    | Pattern::Pinned(_)
                                    | Pattern::MapAll
                                    | Pattern::EnumCase { .. } => None,
                                })
                            })
                            .collect::<Vec<_>>();
                        if !narrowed.is_empty() {
                            let narrowed = Type::union(narrowed);
                            apply_flow_facts(
                                &mut scoped,
                                vec![FlowFact::Binding(name.clone(), narrowed.clone())],
                            );
                            case_subject_type = Some(narrowed);
                        } else if reaching_type != subject_type
                            && case
                                .patterns
                                .iter()
                                .any(|pattern| is_irrefutable_pattern(&pattern.pattern))
                        {
                            apply_flow_facts(
                                &mut scoped,
                                vec![FlowFact::Binding(name.clone(), reaching_type.clone())],
                            );
                            case_subject_type = Some(reaching_type.clone());
                        }
                    }
                    environment.record_match_constraints(case.span.clone(), constraints.clone());
                    if let Some(remaining_cases) = &mut enum_remaining {
                        if remaining_cases.is_empty() {
                            return Err(SourceError::semantic(
                                SourceErrorCode::UnreachableMatchCase,
                                "match case is unreachable",
                                case.span.clone(),
                            )
                            .unreachable_case(case));
                        }
                        let mut covered = Vec::new();
                        for (pattern, constraint) in case.patterns.iter().zip(&constraints) {
                            match &pattern.pattern {
                                Pattern::EnumCase {
                                    path,
                                    case: case_name,
                                } => covered.push(enum_case(
                                    path,
                                    case_name,
                                    environment,
                                    &case.span,
                                )?),
                                pattern if is_irrefutable_pattern(pattern) => {
                                    covered.extend(enum_cases_for_constraint(
                                        constraint.as_ref(),
                                        &subject_type,
                                        environment,
                                    ));
                                }
                                _ => {}
                            }
                        }
                        covered.sort_by(|left, right| left.1.cmp(&right.1));
                        covered.dedup();
                        let matching = covered
                            .iter()
                            .filter(|candidate| remaining_cases.contains(candidate))
                            .cloned()
                            .collect::<Vec<_>>();
                        if matching.is_empty() {
                            return Err(SourceError::semantic(
                                SourceErrorCode::UnreachableMatchCase,
                                "match case cannot match remaining enum cases",
                                case.span.clone(),
                            )
                            .unreachable_case(case));
                        }
                        if case.guard.is_none() {
                            remaining_cases.retain(|candidate| !matching.contains(candidate));
                        }
                    }
                    if enum_remaining.is_none() && coverage_enabled && remaining.is_none() {
                        return Err(SourceError::semantic(
                            SourceErrorCode::UnreachableMatchCase,
                            "match case is unreachable",
                            case.span.clone(),
                        )
                        .unreachable_case(case));
                    }
                    if let Some(current) = &remaining {
                        if constraints.iter().all(Option::is_some)
                            && constraints.iter().all(|constraint| {
                                type_intersection(
                                    current,
                                    constraint.as_ref().expect("checked above"),
                                )
                                .is_none()
                            })
                        {
                            return Err(SourceError::semantic(
                                SourceErrorCode::UnreachableMatchCase,
                                format!("match case cannot match remaining type {current}"),
                                case.span.clone(),
                            )
                            .unreachable_case(case));
                        }
                        if case.guard.is_none() && !irrefutable_constraints.is_empty() {
                            let coverage = if irrefutable_constraints.iter().any(Option::is_none) {
                                current.clone()
                            } else {
                                Type::union(irrefutable_constraints.into_iter().flatten())
                            };
                            if type_intersection(current, &coverage).is_none() {
                                return Err(SourceError::semantic(
                                    SourceErrorCode::UnreachableMatchCase,
                                    "match case is unreachable",
                                    case.span.clone(),
                                )
                                .unreachable_case(case));
                            }
                            remaining = type_subtract(current, &coverage);
                        }
                    }
                    if let Some(guard) = &case.guard {
                        check_expression(guard, &mut scoped, type_parameters)?;
                        {
                            let (facts, _) = condition_facts(guard, &scoped);
                            apply_flow_facts(&mut scoped, facts);
                        }
                    }
                    let checked =
                        check_expression_with_flow(&case.value, &mut scoped, type_parameters)?;
                    // A case that falls through leaves the subject with the type
                    // it narrowed it to, or, narrowing nothing, the type that
                    // reached it.
                    if checked.continuation == Continuation::FallsThrough && case.guard.is_none() {
                        surviving_subject_types.push(case_subject_type.unwrap_or(reaching_type));
                    }
                    results.push(checked.value_type);
                }
                if coverage_enabled && let Some(remaining) = remaining {
                    return Err(SourceError::semantic(
                        SourceErrorCode::NonExhaustiveMatch,
                        format!("non-exhaustive match; missing {remaining}"),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                if let Some(remaining) = enum_remaining
                    && !remaining.is_empty()
                {
                    let missing = remaining
                        .iter()
                        .map(|(identity, case)| format!("{}.{}", identity.name, case))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(SourceError::semantic(
                        SourceErrorCode::NonExhaustiveMatch,
                        format!("non-exhaustive match; missing {missing}"),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                if let Some(Expr {
                    kind: ExprKind::Name(name),
                    ..
                }) = subject.as_deref()
                    && !surviving_subject_types.is_empty()
                {
                    apply_flow_facts(
                        environment,
                        vec![FlowFact::Binding(
                            name.clone(),
                            Type::union(surviving_subject_types),
                        )],
                    );
                }
                Ok(
                    if results
                        .iter()
                        .all(|value_type| matches!(value_type, Type::Never))
                    {
                        Type::Never
                    } else {
                        Type::union(results)
                    },
                )
            }
            ExprKind::StructInit { schema, fields } => {
                let schema_type = check_expression(schema, environment, type_parameters)?;
                require_operation_operand(&Type::Schema, &schema_type, &expression.span)?;
                let schema_binding = expression_binding(schema, environment)
                    .filter(|binding| binding.value_type == Type::Schema);
                let mut provided = std::collections::HashSet::new();
                for (name, value) in fields {
                    if !provided.insert(name) {
                        return Err(SourceError::semantic(
                            SourceErrorCode::DuplicateName,
                            format!("duplicate struct field `{name}`"),
                            value.span.clone(),
                        )
                        .with_extent(value));
                    }
                    let actual = check_expression(value, environment, type_parameters)?;
                    if let Some(schema) = &schema_binding {
                        let expected = schema.members.get(name).ok_or_else(|| {
                            SourceError::semantic(
                                SourceErrorCode::UnknownMember,
                                format!("struct schema has no field `{name}`"),
                                value.span.clone(),
                            )
                            .with_extent(value)
                        })?;
                        require(&expected.value_type, &actual, &value.span)
                            .map_err(|error| explain_nil_start(error, value, environment))?;
                    }
                }
                if let Some(schema) = &schema_binding
                    && let Some(name) = schema
                        .required_fields
                        .iter()
                        .find(|name| !provided.contains(*name))
                {
                    return Err(SourceError::semantic(
                        SourceErrorCode::MissingField,
                        format!("missing required struct field `{name}`"),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                // A schema reached through a module, as in `shapes.Point {...}`,
                // has its identity on the member's binding.
                Ok(schema_binding
                    .and_then(|binding| binding.schema_identity)
                    .or_else(|| known_schema_identity(schema, environment))
                    .map_or(Type::Struct(None), |identity| Type::Struct(Some(identity))))
            }
            ExprKind::StructCopy { value, fields } => {
                let result = check_expression(value, environment, type_parameters)?;
                let schema = known_struct_schema(&result, environment);
                let mut replaced = std::collections::HashSet::new();
                let mut map_replacements = Vec::new();
                for (name, replacement) in fields {
                    if !replaced.insert(name) {
                        return Err(SourceError::semantic(
                            SourceErrorCode::DuplicateName,
                            format!("duplicate struct field `{name}`"),
                            replacement.span.clone(),
                        )
                        .with_extent(replacement));
                    }
                    let actual = check_expression(replacement, environment, type_parameters)?;
                    map_replacements.push(actual.clone());
                    if let Some(schema) = &schema {
                        let expected = schema.members.get(name).ok_or_else(|| {
                            SourceError::semantic(
                                SourceErrorCode::UnknownMember,
                                format!("struct has no field `{name}`"),
                                replacement.span.clone(),
                            )
                            .with_extent(replacement)
                        })?;
                        require(&expected.value_type, &actual, &replacement.span)
                            .map_err(|error| explain_nil_start(error, replacement, environment))?;
                    }
                }
                if let Type::Map(Some((key, value))) = &result {
                    Ok(Type::Map(Some((
                        Box::new(Type::union([key.as_ref().clone(), Type::Str])),
                        Box::new(Type::union(
                            std::iter::once(value.as_ref().clone()).chain(map_replacements),
                        )),
                    ))))
                } else {
                    Ok(result)
                }
            }
            ExprKind::Index { collection, index } => {
                let collection_expression = collection;
                let collection = check_expression(collection, environment, type_parameters)?;
                let index_type = check_expression(index, environment, type_parameters)?;
                if let Type::Tuple(elements) = &collection
                    && let Some(element) = constant_index(index, elements.len())
                {
                    return Ok(elements[element].clone());
                }
                let result = index_result(&collection, &index_type, &expression.span)?;
                if let Some(schema) = known_struct_schema(&collection, environment)
                    && let ExprKind::Value(Value::Str(name)) = &index.kind
                    && !schema.members.contains_key(name.as_ref())
                {
                    return Err(SourceError::semantic(
                        SourceErrorCode::UnknownMember,
                        format!("struct has no field `{name}`"),
                        expression.span.clone(),
                    )
                    .with_extent(expression));
                }
                if let (ExprKind::Name(name), ExprKind::Value(Value::Str(key))) =
                    (&collection_expression.kind, &index.kind)
                    && let Some(narrowed) = environment
                        .lookup(name)
                        .and_then(|binding| binding.member_facts.get(key.as_ref()))
                {
                    return Ok(narrowed.clone());
                }
                Ok(expression_binding(expression, environment)
                    .map(|binding| binding.value_type)
                    .or_else(|| known_struct_field_type(&collection, &expression.kind, environment))
                    .unwrap_or(result))
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                let collection = check_expression(collection, environment, type_parameters)?;
                for bound in [start, end, step].into_iter().flatten() {
                    let bound = check_expression(bound, environment, type_parameters)?;
                    require_operation_operand(&Type::Num, &bound, &expression.span)?;
                }
                slice_result(&collection, &expression.span)
            }
            ExprKind::Interpolate(_) => Ok(Type::Str),
            ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. }
            | ExprKind::Documentation(_) => Ok(Type::Nil),
            ExprKind::NotImplemented => Ok(Type::Unknown),
        }
    })
}

/// Check an expression while retaining its source-level continuation outcome.
///
/// The existing type checker remains the authority for an expression's value
/// type. Flow-sensitive constructs progressively use this result to decide
/// which environments can reach their continuation.
/// Declares, as dynamic, each function a block declares by name, as the
/// compiler hoists them, so an earlier function's call to a later one is not
/// checked against an outer binding of that name. Each declaration then
/// replaces its entry with the function's signature.
fn predeclare_functions(values: &[Expr], environment: &mut Environment) {
    for value in values {
        if let ExprKind::Declare {
            pattern: Pattern::Binding(name),
            value: declared,
            ..
        } = &value.kind
            && matches!(declared.kind, ExprKind::Function { .. })
            && environment.lookup_in_scope(name).is_none()
        {
            environment.declare(name.clone(), SemanticBinding::value(Type::Unknown));
        }
    }
}

/// Infers the type of each `var` these statements start with `nil` from a
/// trial check of them, which logs every assignment to it, including in
/// nested functions and in later code: its declared type is `nil` with the
/// type of the first one written, which later assignments must then conform
/// to. A trial that fails because of such a `var`, or that finds no
/// assignment or a first one of unknown type, leaves the `var` `any|nil`. Within a trial every such `var` is
/// dynamic and no further trial runs, so nested blocks are checked a bounded
/// number of extra times.
fn infer_nil_starts(
    values: &[Expr],
    environment: &Environment,
    type_parameters: &[String],
    scoped: bool,
) {
    super::stack::grow(|| {
        if environment.in_trial() {
            return;
        }
        let sites = values
            .iter()
            .filter_map(|value| match &value.kind {
                ExprKind::Declare {
                    mutable: true,
                    pattern: Pattern::Binding(name),
                    annotation: None,
                    value: initial,
                    ..
                } if matches!(initial.kind, ExprKind::Value(Value::Nil)) => {
                    Some((value.span.clone(), name.clone()))
                }
                _ => None,
            })
            .collect::<Vec<NilStartSite>>();
        if sites.is_empty() {
            return;
        }
        // Each round seeds every `var` with the first assignment's type the last
        // one found, so an accumulator such as `count = count + 1` is typed once
        // `count` is. The first round is final when every first assignment's
        // type is known, since none then came from a dynamic `var`; a later one
        // when it finds exactly the types it was seeded with. A trial checks no
        // assignment to these vars, so a later assignment of another type is
        // reported by the real check rather than ending the trial.
        let mut seed = HashMap::new();
        for round in 0..MAX_NIL_START_ROUNDS {
            let mut trial = environment.trial(seed.clone());
            if scoped {
                trial.enter_scope();
                predeclare_functions(values, &mut trial);
            }
            // An error in the first round, in which these vars are dynamic, is
            // not theirs, so the assignments checked before it still count.
            let mut failed = false;
            for value in values {
                match check_expression_with_flow(value, &mut trial, type_parameters) {
                    Ok(checked) if checked.continuation == Continuation::Terminates => break,
                    Ok(_) => {}
                    Err(_) if round == 0 => {
                        failed = true;
                        break;
                    }
                    Err(_) => return,
                }
            }
            let mut assignments = trial.trial_assignments();
            let mut next = HashMap::new();
            let mut settled = true;
            for site in &sites {
                let mut found = assignments.remove(site).unwrap_or_default();
                found.truncate(1);
                let known = found
                    .iter()
                    .filter(|(_, value_type)| value_type.clone().widen_unknown() == *value_type)
                    .cloned()
                    .collect::<Vec<_>>();
                let seeded = seed.get(site).map(|types: &Rc<Vec<(SourceSpan, Type)>>| {
                    Type::union(types.iter().map(|(_, value_type)| value_type.clone()))
                });
                let complete = !found.is_empty() && known.len() == found.len();
                let union = Type::union(known.iter().map(|(_, value_type)| value_type.clone()));
                settled &= known.is_empty()
                    || if round == 0 {
                        complete
                    } else {
                        seeded.as_ref() == Some(&union)
                    };
                if !known.is_empty() {
                    next.insert(site.clone(), (complete, known));
                }
            }
            if settled || failed {
                for (site, (complete, known)) in next {
                    if complete {
                        environment.infer_nil_start(site, known);
                    }
                }
                return;
            }
            seed = next
                .into_iter()
                .map(|(site, (_, known))| (site, Rc::new(known)))
                .collect();
        }
    });
}

/// Trial rounds before a `nil`-started `var` whose assignments keep changing
/// its type is left `any|nil`.
const MAX_NIL_START_ROUNDS: usize = 3;

fn check_expression_with_flow(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<CheckedExpression, SourceError> {
    super::stack::grow(|| {
        if let ExprKind::Block(values) = &expression.kind {
            infer_nil_starts(values, environment, type_parameters, true);
            let mut scoped = environment.clone();
            scoped.enter_scope();
            predeclare_functions(values, &mut scoped);
            let mut result = Type::Nil;
            for value in values {
                let checked = check_expression_with_flow(value, &mut scoped, type_parameters)?;
                result = checked.value_type;
                if checked.continuation == Continuation::Terminates {
                    environment.merge_compatible_types(&scoped, &scoped);
                    return Ok(CheckedExpression::terminates(result));
                }
            }
            environment.merge_compatible_types(&scoped, &scoped);
            return Ok(CheckedExpression::falls_through(result));
        }
        if let ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } = &expression.kind
        {
            check_expression(condition, environment, type_parameters)?;
            let (then_facts, else_facts) = condition_facts(condition, environment);
            let mut then_environment = environment.clone();
            apply_flow_facts(&mut then_environment, then_facts);
            let left =
                check_expression_with_flow(then_branch, &mut then_environment, type_parameters)?;
            let mut else_environment = environment.clone();
            apply_flow_facts(&mut else_environment, else_facts);
            let right = else_branch
                .as_ref()
                .map(|branch| {
                    check_expression_with_flow(branch, &mut else_environment, type_parameters)
                })
                .transpose()?
                .unwrap_or_else(|| CheckedExpression::falls_through(Type::Nil));

            return match (left.continuation, right.continuation) {
                (Continuation::FallsThrough, Continuation::Terminates) => {
                    environment.merge_reachable_types(Some(&then_environment), None);
                    Ok(CheckedExpression::falls_through(left.value_type))
                }
                (Continuation::Terminates, Continuation::FallsThrough) => {
                    environment.merge_reachable_types(None, Some(&else_environment));
                    Ok(CheckedExpression::falls_through(right.value_type))
                }
                (Continuation::FallsThrough, Continuation::FallsThrough) => {
                    environment
                        .merge_reachable_types(Some(&then_environment), Some(&else_environment));
                    Ok(CheckedExpression::falls_through(Type::union([
                        left.value_type,
                        right.value_type,
                    ])))
                }
                (Continuation::Terminates, Continuation::Terminates) => {
                    debug_assert!(!environment.merge_reachable_types(None, None));
                    Ok(CheckedExpression::terminates(Type::union([
                        left.value_type,
                        right.value_type,
                    ])))
                }
            };
        }
        let value_type = check_expression(expression, environment, type_parameters)?;
        Ok(match &expression.kind {
            ExprKind::Return { .. } | ExprKind::Throw { .. } | ExprKind::Recur(_) => {
                CheckedExpression::terminates(value_type)
            }
            ExprKind::Match { .. } if matches!(value_type, Type::Never) => {
                CheckedExpression::terminates(value_type)
            }
            _ => CheckedExpression::falls_through(value_type),
        })
    })
}

fn callable_result_type(value_type: &Type) -> Type {
    match value_type {
        Type::Function(Some(signature)) => signature.first().cloned().unwrap_or(Type::Unknown),
        _ => Type::Unknown,
    }
}

fn binary_result(
    operator: Binary,
    left: &Type,
    right: &Type,
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    if matches!(left, Type::Never) || matches!(right, Type::Never) {
        return Ok(Type::Never);
    }
    match operator {
        Binary::Or | Binary::And | Binary::Equal | Binary::NotEqual => Ok(Type::Bool),
        Binary::Coalesce => Ok(coalesce_result(left, right)),
        Binary::Greater | Binary::GreaterEqual | Binary::Less | Binary::LessEqual => {
            ordering_operands(operator, left, right, span)?;
            Ok(Type::Bool)
        }
        Binary::BitOr | Binary::BitXor | Binary::BitAnd => bitwise_result(left, right, span),
        Binary::Subtract => subtract_result(left, right, span),
        Binary::ShiftLeft | Binary::ShiftRight | Binary::Divide | Binary::Modulo => {
            numeric_operands(left, right, span)?;
            Ok(Type::Num)
        }
        // A tuple takes part in list operations as the list it also is.
        Binary::Append => list_append_result(&left.as_list(), right, span),
        Binary::Prepend => list_append_result(&right.as_list(), left, span),
        Binary::Add => add_result(&left.as_list(), &right.as_list(), span),
        Binary::Multiply => multiply_result(left, right, span),
        Binary::Pipeline => Ok(right.clone()),
    }
}

/// `left ?? right` is `left` without `nil`, or `right` when `left` is `nil`.
/// When `left` can never be `nil`, `right` is never evaluated.
fn coalesce_result(left: &Type, right: &Type) -> Type {
    match type_subtract(left, &Type::Nil) {
        None => right.clone(),
        Some(present) if present == *left && !matches!(left, Type::Unknown) => present,
        Some(present) => Type::union([present, right.clone()]),
    }
}

fn bitwise_result(left: &Type, right: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Bytes, Type::Bytes | Type::Num) | (Type::Num, Type::Bytes) => Ok(Type::Bytes),
        _ => invalid_operation("bitwise operator", left, right, span),
    }
}

fn prefix_result(operator: Prefix, value: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if matches!(value, Type::Never) {
        return Ok(Type::Never);
    }
    match operator {
        Prefix::Not => Ok(Type::Bool),
        Prefix::Negate => {
            require_operation_operand(&Type::Num, value, span)?;
            Ok(Type::Num)
        }
        Prefix::BitNot => bit_not_result(value, span),
    }
}

fn bit_not_result(value: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(value) {
        return Ok(Type::Unknown);
    }
    match value {
        Type::Num => Ok(Type::Num),
        Type::Bytes => Ok(Type::Bytes),
        _ => Err(SourceError::semantic(
            SourceErrorCode::TypeMismatch,
            format!("operator `~` does not accept {value}"),
            span.clone(),
        )),
    }
}

/// Ordering compares two numbers or two strings.
/// The operand family of a number or string literal, including a negated
/// number.
fn literal_family(expression: &Expr) -> Option<Type> {
    super::stack::grow(|| match &expression.kind {
        ExprKind::Value(Value::Int(_) | Value::Float(_)) => Some(Type::Num),
        ExprKind::Value(Value::Str(_)) | ExprKind::Interpolate(_) => Some(Type::Str),
        ExprKind::Prefix { operators, value }
            if operators
                .iter()
                .all(|(operator, _)| matches!(operator, Prefix::Negate)) =>
        {
            literal_family(value).filter(|family| *family == Type::Num)
        }
        _ => None,
    })
}

/// Ordering compares two numbers or two strings.
fn ordering_operands(
    operator: Binary,
    left: &Type,
    right: &Type,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(());
    }
    let both = |family: &Type| left.is_assignable_to(family) && right.is_assignable_to(family);
    if both(&Type::Num) || both(&Type::Str) {
        return Ok(());
    }
    let symbol = match operator {
        Binary::Greater => ">",
        Binary::GreaterEqual => ">=",
        Binary::Less => "<",
        _ => "<=",
    };
    invalid_operation(symbol, left, right, span).map(|_| ())
}

fn numeric_operands(left: &Type, right: &Type, span: &SourceSpan) -> Result<(), SourceError> {
    require_operation_operand(&Type::Num, left, span)?;
    require_operation_operand(&Type::Num, right, span)
}

fn add_result(left: &Type, right: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if matches!(left, Type::Str) {
        return Ok(Type::Str);
    }
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Bytes, Type::Bytes) => Ok(Type::Bytes),
        (Type::Map(left), Type::Map(right)) => Ok(Type::Map(match (left, right) {
            (Some((left_key, left_value)), Some((right_key, right_value))) => Some((
                Box::new(Type::union([
                    left_key.as_ref().clone(),
                    right_key.as_ref().clone(),
                ])),
                Box::new(Type::union([
                    left_value.as_ref().clone(),
                    right_value.as_ref().clone(),
                ])),
            )),
            _ => None,
        })),
        (Type::List(left), Type::List(right)) => Ok(Type::List(match (left, right) {
            (Some(left), Some(right)) => Some(Box::new(Type::union([
                left.as_ref().clone(),
                right.as_ref().clone(),
            ]))),
            _ => None,
        })),
        _ => invalid_operation("+", left, right, span),
    }
}

fn subtract_result(left: &Type, right: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if matches!(left, Type::Map(_)) {
        if is_dynamic_operation_type(right) || is_map_key_type(right) {
            return Ok(left.clone());
        }
        return invalid_operation("-", left, right, span);
    }
    numeric_operands(left, right, span)?;
    Ok(Type::Num)
}

fn multiply_result(left: &Type, right: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Str, Type::Num) => Ok(Type::Str),
        _ => invalid_operation("*", left, right, span),
    }
}

fn list_append_result(list: &Type, value: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if matches!(list, Type::Bytes) {
        if is_dynamic_operation_type(value) || matches!(value, Type::Num | Type::Bytes) {
            return Ok(Type::Bytes);
        }
        return invalid_operation(":+", list, value, span);
    }
    if is_dynamic_operation_type(list) || is_dynamic_operation_type(value) {
        return Ok(Type::List(None));
    }
    match list {
        Type::List(element) => {
            Ok(Type::List(element.as_ref().map(|element| {
                Box::new(Type::union([element.as_ref().clone(), value.clone()]))
            })))
        }
        other => invalid_operation(":+", other, value, span),
    }
}

/// `actual`, the type of `value`, as the tuple `expected` asks for when
/// `value` is a list literal of that many elements, each with its own type:
/// `[lo, hi]` is a `[num, num]` where one is expected, though alone it is a
/// `list<num>`.
fn as_expected_tuple(
    expected: &Type,
    value: &Expr,
    actual: Type,
    environment: &Environment,
) -> Type {
    let ExprKind::List(items) = &value.kind else {
        return actual;
    };
    let wants = |length: usize| match expected {
        Type::Tuple(elements) => elements.len() == length,
        Type::Union(members) => members
            .iter()
            .any(|member| matches!(member, Type::Tuple(elements) if elements.len() == length)),
        _ => false,
    };
    if actual.is_assignable_to(expected) || !wants(items.len()) {
        return actual;
    }
    let elements = items
        .iter()
        .map(|item| match item {
            ListElement::Value(element) => environment.recorded_expression_type(&element.span),
            ListElement::Spread(_) => None,
        })
        .collect::<Option<Vec<_>>>();
    elements.map_or(actual, Type::Tuple)
}

/// The position a constant integer index, possibly negative, names in a
/// collection of `length` elements, when it is in range.
fn constant_index(index: &Expr, length: usize) -> Option<usize> {
    let value = match &index.kind {
        ExprKind::Value(Value::Int(value)) => *value,
        ExprKind::Prefix { operators, value } if operators.len() == 1 => {
            match (&operators[0].0, &value.kind) {
                (super::ast::Prefix::Negate, ExprKind::Value(Value::Int(value))) => {
                    value.checked_neg()?
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let length = i64::try_from(length).ok()?;
    let position = if value < 0 { length + value } else { value };
    (0..length)
        .contains(&position)
        .then(|| usize::try_from(position).ok())
        .flatten()
}

fn index_result(collection: &Type, index: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(collection) || is_dynamic_operation_type(index) {
        return Ok(Type::Unknown);
    }
    match collection {
        Type::List(element) => {
            require_operation_operand(&Type::Num, index, span)?;
            Ok(element.as_deref().cloned().unwrap_or(Type::Unknown))
        }
        Type::Bytes => {
            require_operation_operand(&Type::Num, index, span)?;
            Ok(Type::Num)
        }
        Type::Str => {
            require_operation_operand(&Type::Num, index, span)?;
            Ok(Type::Str)
        }
        Type::Map(entries) => match entries {
            Some((key, value)) => {
                require_operation_operand(key, index, span)?;
                Ok(Type::union([value.as_ref().clone(), Type::Nil]))
            }
            None => Ok(Type::Unknown),
        },
        Type::Struct(_) => {
            require_operation_operand(&Type::Str, index, span)?;
            Ok(Type::Unknown)
        }
        // A constant index into a tuple reads its element; the caller handles
        // that, so any other index reads one of the elements.
        Type::Tuple(_) => index_result(&collection.as_list(), index, span),
        // A value of a union type is one of its members, so indexing it
        // yields any of their results, when every member can be indexed.
        Type::Union(members) => members
            .iter()
            .map(|member| index_result(member, index, span))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::union)
            .or_else(|_| invalid_operation("[]", collection, index, span)),
        other => invalid_operation("[]", other, index, span),
    }
}

fn slice_result(collection: &Type, span: &SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(collection) {
        return Ok(Type::Unknown);
    }
    match collection {
        Type::List(element) => Ok(Type::List(element.clone())),
        Type::Tuple(_) => slice_result(&collection.as_list(), span),
        Type::Bytes => Ok(Type::Bytes),
        Type::Str => Ok(Type::Str),
        Type::Union(members)
            if members
                .iter()
                .all(|member| slice_result(member, span).is_ok()) =>
        {
            Ok(Type::union(
                members
                    .iter()
                    .filter_map(|member| slice_result(member, span).ok()),
            ))
        }
        other => Err(SourceError::semantic(
            SourceErrorCode::TypeMismatch,
            format!("expected list, got {other}"),
            span.clone(),
        )
        .with_expected_actual("list".into(), other.to_string())),
    }
}

fn require_operation_operand(
    expected: &Type,
    actual: &Type,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    if is_dynamic_operation_type(actual) || actual.is_assignable_to(expected) {
        return Ok(());
    }
    require(expected, actual, span)
}

fn invalid_operation(
    operator: &str,
    left: &Type,
    right: &Type,
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    Err(SourceError::semantic(
        SourceErrorCode::TypeMismatch,
        format!("operator `{operator}` does not accept {left} and {right}"),
        span.clone(),
    ))
}

fn is_closed_coverage_type(value_type: &Type) -> bool {
    match value_type {
        Type::Nil
        | Type::Bool
        | Type::Num
        | Type::Str
        | Type::Bytes
        | Type::Resource(_)
        | Type::Enum(_)
        | Type::Schema
        | Type::Struct(Some(_))
        | Type::Function(None)
        | Type::Task(None)
        | Type::Channel(None) => true,
        Type::Union(members) => members.iter().all(is_closed_coverage_type),
        Type::Never
        | Type::Unknown
        | Type::Any
        | Type::List(_)
        | Type::Map(_)
        | Type::Function(Some(_))
        | Type::Task(Some(_))
        | Type::Channel(Some(_))
        | Type::Struct(None)
        | Type::Tuple(_)
        | Type::Generic(_) => false,
    }
}

type EnumCase = (super::semantic::EnumIdentity, String);

fn enum_cases(value_type: &Type, environment: &Environment) -> Option<Vec<EnumCase>> {
    match value_type {
        Type::Enum(identity) => environment.enum_cases(identity).map(|cases| {
            cases
                .into_iter()
                .map(|case| (identity.clone(), case))
                .collect()
        }),
        Type::Union(members) => members
            .iter()
            .map(|member| enum_cases(member, environment))
            .collect::<Option<Vec<_>>>()
            .map(|groups| groups.into_iter().flatten().collect()),
        _ => None,
    }
}

fn enum_case(
    path: &str,
    case: &str,
    environment: &Environment,
    span: &SourceSpan,
) -> Result<EnumCase, SourceError> {
    let Some(super::environment::TypeMember::Enum { identity, cases }) =
        environment.type_member(path)
    else {
        return Err(SourceError::semantic(
            SourceErrorCode::UnknownType,
            format!("unknown enum `{path}`"),
            span.clone(),
        ));
    };
    if !cases.iter().any(|candidate| candidate == case) {
        return Err(SourceError::semantic(
            SourceErrorCode::UnknownMember,
            format!("enum `{path}` has no case `{case}`"),
            span.clone(),
        ));
    }
    Ok((identity.clone(), case.into()))
}

fn enum_cases_for_constraint(
    constraint: Option<&Type>,
    subject: &Type,
    environment: &Environment,
) -> Vec<EnumCase> {
    match constraint {
        None => enum_cases(subject, environment).unwrap_or_default(),
        Some(Type::Enum(identity)) => environment
            .enum_cases(identity)
            .unwrap_or_default()
            .into_iter()
            .map(|case| (identity.clone(), case))
            .collect(),
        Some(Type::Union(members)) => members
            .iter()
            .flat_map(|member| enum_cases_for_constraint(Some(member), subject, environment))
            .collect(),
        Some(_) => Vec::new(),
    }
}

fn is_irrefutable_pattern(pattern: &Pattern) -> bool {
    super::stack::grow(|| match pattern {
        Pattern::Wildcard | Pattern::Binding(_) => true,
        Pattern::At { pattern, .. } => is_irrefutable_pattern(pattern),
        Pattern::List { .. }
        | Pattern::Map { .. }
        | Pattern::Literal(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => false,
    })
}

/// Narrowed types known on one control-flow path: a binding's own, or that of
/// a string-keyed member of an immutable binding. Facts retain their actual
/// `Type` values, including nominal identities, so match constraints can share
/// this representation with direct conditions.
type FlowFacts = Vec<FlowFact>;

#[derive(Clone, Debug)]
enum FlowFact {
    Binding(String, Type),
    /// The member `key` of the binding `name`, as `name.key` or `name["key"]`.
    Member(String, String, Type),
}

/// The facts a condition establishes when it holds and when it does not.
/// Both sides of a holding `&&` hold, and neither side of a failing `||`
/// does; `!` swaps them.
fn condition_facts(expression: &Expr, environment: &Environment) -> (FlowFacts, FlowFacts) {
    super::stack::grow(|| {
        match &expression.kind {
            ExprKind::Binary {
                left,
                operator: operator @ (Binary::And | Binary::Or),
                right,
            } => {
                let and = matches!(operator, Binary::And);
                let (left_then, left_else) = condition_facts(left, environment);
                let left_known = if and { left_then } else { left_else };
                // The right side is evaluated only on that path, so it sees the
                // left side's facts.
                let mut narrowed = environment.clone();
                apply_flow_facts(&mut narrowed, left_known.clone());
                let (right_then, right_else) = condition_facts(right, &narrowed);
                let right_known = if and { right_then } else { right_else };
                let known = left_known.into_iter().chain(right_known).collect();
                return if and {
                    (known, Vec::new())
                } else {
                    (Vec::new(), known)
                };
            }
            ExprKind::Prefix { operators, value }
                if operators
                    .iter()
                    .all(|(operator, _)| matches!(operator, super::ast::Prefix::Not)) =>
            {
                let (then_facts, else_facts) = condition_facts(value, environment);
                return if operators.len() % 2 == 0 {
                    (then_facts, else_facts)
                } else {
                    (else_facts, then_facts)
                };
            }
            _ => {}
        }
        let ExprKind::Binary {
            left,
            operator,
            right,
        } = &expression.kind
        else {
            return (Vec::new(), Vec::new());
        };
        if !matches!(operator, Binary::Equal | Binary::NotEqual) {
            return (Vec::new(), Vec::new());
        }
        let tested = match (&left.kind, &right.kind) {
            (_, ExprKind::Value(Value::Nil)) => left,
            (ExprKind::Value(Value::Nil), _) => right,
            _ => return (Vec::new(), Vec::new()),
        };
        let fact = |value_type: Type| match &tested.kind {
            ExprKind::Name(name) => Some(FlowFact::Binding(name.clone(), value_type)),
            ExprKind::Index { collection, index } => match (&collection.kind, &index.kind) {
                (ExprKind::Name(name), ExprKind::Value(Value::Str(key))) => {
                    Some(FlowFact::Member(name.clone(), key.to_string(), value_type))
                }
                _ => None,
            },
            _ => None,
        };
        let current = match &tested.kind {
            ExprKind::Name(name) => environment
                .lookup(name)
                .map(|binding| binding.value_type.clone()),
            // Only an immutable binding's member keeps its value, so only its
            // members are narrowed; the condition's check recorded the type.
            ExprKind::Index { collection, .. } => match &collection.kind {
                ExprKind::Name(name)
                    if environment
                        .lookup(name)
                        .is_some_and(|binding| binding.declared.is_none()) =>
                {
                    environment.recorded_expression_type(&tested.span)
                }
                _ => None,
            },
            _ => None,
        };
        let Some(current) = current else {
            return (Vec::new(), Vec::new());
        };
        let non_nil = fact(current.without_nil()).into_iter().collect();
        let nil = fact(Type::Nil).into_iter().collect();
        if matches!(operator, Binary::NotEqual) {
            (non_nil, nil)
        } else {
            (nil, non_nil)
        }
    })
}

fn apply_flow_facts(environment: &mut Environment, facts: FlowFacts) {
    for fact in facts {
        match fact {
            FlowFact::Binding(name, value_type) => {
                if let Some(binding) = environment.lookup_mut(&name) {
                    binding.value_type = value_type;
                }
            }
            FlowFact::Member(name, key, value_type) => {
                if let Some(binding) = environment.lookup_mut(&name) {
                    binding.member_facts.insert(key, value_type);
                }
            }
        }
    }
}

fn expression_binding(expression: &Expr, environment: &Environment) -> Option<SemanticBinding> {
    super::stack::grow(|| match &expression.kind {
        ExprKind::Name(name) => environment.lookup(name).cloned(),
        ExprKind::TypeApply { callee, .. } => expression_binding(callee, environment),
        ExprKind::Index { collection, index } => {
            let collection = expression_binding(collection, environment)?;
            let ExprKind::Value(Value::Str(name)) = &index.kind else {
                return None;
            };
            collection.members.get(name.as_ref()).cloned()
        }
        ExprKind::Call { callee, arguments } if matches!(&callee.kind, ExprKind::Name(name) if name == "import") => {
            imported_modules(arguments, environment)
        }
        _ => None,
    })
}

/// The binding an unannotated declaration gives its value. It keeps the
/// value's type, including any part the checker could not infer, which stays
/// dynamic as it is where the value is used directly: naming a value never
/// changes its type.
fn value_binding(
    expression: &Expr,
    value_type: &Type,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<SemanticBinding, SourceError> {
    super::stack::grow(|| {
        if let ExprKind::StructSchema(fields) = &expression.kind {
            return schema_binding(fields, environment, type_parameters, &expression.span);
        }
        if let Some(binding) = expression_binding(expression, environment) {
            return Ok(binding);
        }
        if let ExprKind::Map(entries) = &expression.kind {
            let mut binding = SemanticBinding::value(value_type.clone());
            for (key, value) in entries {
                if let ExprKind::Value(Value::Str(name)) = &key.kind {
                    binding.members.insert(
                        name.to_string(),
                        static_map_member_binding(value, environment, type_parameters)?,
                    );
                }
            }
            return Ok(binding);
        }
        if let Some(schema) = known_struct_schema(value_type, environment) {
            let mut binding = SemanticBinding::value(value_type.clone());
            binding.members = schema.members;
            return Ok(binding);
        }
        Ok(SemanticBinding::value(value_type.clone()))
    })
}

fn static_map_member_binding(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<SemanticBinding, SourceError> {
    super::stack::grow(|| {
        if let Some(binding) = expression_binding(expression, environment) {
            return Ok(binding);
        }
        match &expression.kind {
            ExprKind::Value(value) => Ok(SemanticBinding::value(value_type(value))),
            ExprKind::Map(_) => {
                value_binding(expression, &Type::Map(None), environment, type_parameters)
            }
            _ => Ok(SemanticBinding::value(Type::Unknown)),
        }
    })
}

fn schema_binding(
    fields: &[super::ast::StructSchemaField],
    environment: &mut Environment,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<SemanticBinding, SourceError> {
    let mut binding = SemanticBinding::value(Type::Schema);
    binding.schema_identity = Some(SchemaIdentity::declared(
        format!("{}:{}:{}", span.path, span.line, span.column),
        "<schema>",
    ));
    for field in fields {
        let value_type = if let Some(annotation) = &field.annotation {
            resolve_static_annotation(
                annotation,
                type_parameters,
                &field.default.as_ref().map_or_else(
                    || SourceSpan::new("<schema>", 1, 1),
                    |default| default.span.clone(),
                ),
                environment,
            )?
        } else if let Some(default) = &field.default {
            // Construction and copying can replace a default, so like a
            // `var` initializer, a `nil` default means no value yet.
            match check_expression(default, environment, type_parameters)?.widen_unknown() {
                Type::Nil => Type::universal(),
                default_type => default_type,
            }
        } else {
            Type::Unknown
        };
        if field.default.is_none() {
            binding.required_fields.insert(field.name.clone());
        }
        binding
            .members
            .insert(field.name.clone(), SemanticBinding::value(value_type));
    }
    Ok(binding)
}

fn known_struct_schema(value_type: &Type, environment: &Environment) -> Option<SemanticBinding> {
    let Type::Struct(Some(identity)) = value_type else {
        return None;
    };
    environment.schema_by_identity(identity)
}

fn known_struct_field_type(
    collection_type: &Type,
    expression: &ExprKind,
    environment: &Environment,
) -> Option<Type> {
    let ExprKind::Index { index, .. } = expression else {
        return None;
    };
    let ExprKind::Value(Value::Str(name)) = &index.kind else {
        return None;
    };
    known_struct_schema(collection_type, environment)
        .and_then(|schema| schema.members.get(name.as_ref()).cloned())
        .map(|field| field.value_type)
}

fn imported_modules(
    arguments: &[CallArgument],
    environment: &Environment,
) -> Option<SemanticBinding> {
    let mut result: HashMap<String, SemanticBinding> = HashMap::new();
    let mut type_members = HashMap::new();
    let mut schemas = Vec::new();
    for argument in arguments {
        let CallArgument::Positional(Expr {
            kind: ExprKind::Value(Value::Str(module_name)),
            ..
        }) = argument
        else {
            return None;
        };
        let snapshot = environment.import(module_name.as_ref())?;
        for (name, incoming) in &snapshot.exports {
            let mut incoming = incoming.clone();
            incoming.set_resource_runtime_module(module_name.as_ref());
            let Some(existing) = result.get_mut(name) else {
                result.insert(name.clone(), incoming);
                continue;
            };
            if existing.callables.is_empty() || incoming.callables.is_empty() {
                continue;
            }
            for signature in &incoming.callables {
                if !existing
                    .callables
                    .iter()
                    .any(|existing| existing.has_same_input(signature))
                {
                    existing.callables.push(signature.clone());
                }
            }
        }
        for (name, member) in &snapshot.types {
            type_members
                .entry(name.clone())
                .or_insert_with(|| member.with_resource_runtime_module(module_name.as_ref()));
        }
        schemas.extend(snapshot.schemas.iter().cloned());
    }
    Some(SemanticBinding::module(result, type_members, schemas))
}

fn bind_semantic_pattern(
    pattern: &Pattern,
    binding: &SemanticBinding,
    environment: &mut Environment,
) {
    super::stack::grow(|| match pattern {
        Pattern::Binding(name) => environment.declare(name.clone(), binding.clone()),
        Pattern::At { name, pattern } => {
            environment.declare(name.clone(), binding.clone());
            bind_semantic_pattern(pattern, binding, environment);
        }
        Pattern::Map { entries, .. } => {
            let schema = known_struct_schema(&binding.value_type, environment);
            for (key, pattern) in entries {
                let MapPatternKey::String(key) = key else {
                    continue;
                };
                if let Some(member) = binding.members.get(key).cloned().or_else(|| {
                    schema
                        .as_ref()
                        .and_then(|schema| schema.members.get(key))
                        .cloned()
                }) {
                    bind_semantic_pattern(pattern, &member, environment);
                }
            }
        }
        Pattern::MapAll => {
            let members = if binding.members.is_empty() {
                known_struct_schema(&binding.value_type, environment)
                    .map(|schema| schema.members)
                    .unwrap_or_default()
            } else {
                binding.members.clone()
            };
            environment.record_selection((!members.is_empty()).then(|| members.keys()));
            for (name, member) in members {
                environment.declare(name, member);
            }
            for (name, member) in &binding.type_members {
                environment.declare_type(name.clone(), member.clone());
            }
        }
        Pattern::List { .. }
        | Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Pinned(_)
        | Pattern::EnumCase { .. } => {}
    });
}

fn check_pattern(
    pattern: &Pattern,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<(), SourceError> {
    super::stack::grow(|| match pattern {
        Pattern::At { pattern, .. } => check_pattern(pattern, environment, type_parameters),
        Pattern::List { items, .. } => {
            for item in items {
                check_pattern(item, environment, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Map { entries, .. } => {
            for (key, value) in entries {
                if let MapPatternKey::Computed(key) = key {
                    check_expression(key, environment, type_parameters)?;
                }
                check_pattern(value, environment, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Binding(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => Ok(()),
    })
}

fn check_case_pattern(
    pattern: &CasePattern,
    environment: &mut Environment,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<Option<Type>, SourceError> {
    super::stack::grow(|| {
        let constraint = if let Some(constraint) = &pattern.constraint {
            let constraint =
                resolve_static_annotation(constraint, type_parameters, span, environment)?;
            if !constraint.is_reifiable_match_constraint() {
                return Err(SourceError::semantic(
                    SourceErrorCode::InvalidMatch,
                    "match type constraint is not runtime-checkable",
                    span.clone(),
                ));
            }
            Some(constraint)
        } else {
            None
        };
        check_pattern(&pattern.pattern, environment, type_parameters)?;
        Ok(constraint)
    })
}

fn bind_case_pattern(pattern: &Pattern, value_type: &Type, environment: &mut Environment) {
    super::stack::grow(|| {
        match pattern {
            Pattern::Binding(name) => {
                environment.declare(name.clone(), SemanticBinding::value(value_type.clone()));
            }
            Pattern::At { name, pattern } => {
                environment.declare(name.clone(), SemanticBinding::value(value_type.clone()));
                bind_case_pattern(pattern, value_type, environment);
            }
            Pattern::List { items, rest } => {
                // Without a rest, a tuple's items bind its elements in order.
                if let Type::Tuple(elements) = value_type
                    && rest.is_none()
                    && elements.len() == items.len()
                {
                    for (item, element) in items.iter().zip(elements) {
                        bind_case_pattern(item, element, environment);
                    }
                    return;
                }
                let widened = value_type.as_list();
                let element = match &widened {
                    Type::List(Some(element)) => element.as_ref(),
                    Type::Bytes => &Type::Num,
                    _ => &Type::Unknown,
                };
                for item in items {
                    bind_case_pattern(item, element, environment);
                }
                if let Some(super::ast::RestPattern::Binding(name)) = rest {
                    let rest_type = if matches!(value_type, Type::Bytes) {
                        Type::Bytes
                    } else {
                        Type::List(Some(Box::new(element.clone())))
                    };
                    environment.declare(name.clone(), SemanticBinding::value(rest_type));
                }
            }
            Pattern::Map { entries, rest, .. } => {
                let (key, map_value) = match value_type {
                    Type::Map(Some((key, value))) => (key.as_ref(), value.as_ref()),
                    _ => (&Type::Unknown, &Type::Unknown),
                };
                let schema = known_struct_schema(value_type, environment);
                for (map_key, pattern) in entries {
                    let value =
                        if let (Some(schema), MapPatternKey::String(name)) = (&schema, map_key) {
                            schema
                                .members
                                .get(name)
                                .map_or(map_value, |member| &member.value_type)
                        } else {
                            map_value
                        };
                    bind_case_pattern(pattern, value, environment);
                }
                if let Some(super::ast::RestPattern::Binding(name)) = rest {
                    environment.declare(
                        name.clone(),
                        SemanticBinding::value(Type::Map(Some((
                            Box::new(key.clone()),
                            Box::new(map_value.clone()),
                        )))),
                    );
                }
            }
            Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Pinned(_)
            | Pattern::MapAll
            | Pattern::EnumCase { .. } => {}
        }
    });
}

#[allow(clippy::too_many_lines)]
fn check_call(
    callee: &Expr,
    arguments: &[CallArgument],
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
    piped: Option<Type>,
) -> Result<Type, SourceError> {
    super::stack::grow(|| {
        check_expression(callee, environment, type_parameters)?;
        let mut actuals = piped.into_iter().collect::<Vec<_>>();
        actuals.extend(
            arguments
                .iter()
                .map(|argument| check_argument(argument, environment, type_parameters))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut shapes = vec![ArgumentShape::Positional; actuals.len() - arguments.len()];
        shapes.extend(arguments.iter().map(|argument| match argument {
            CallArgument::Positional(_) => ArgumentShape::Positional,
            CallArgument::Named { name, .. } => ArgumentShape::Named(name),
            CallArgument::Spread(_) => ArgumentShape::Spread,
        }));
        let (name, binding, explicit) = match &callee.kind {
            ExprKind::Name(name) => (name.clone(), environment.lookup(name).cloned(), None),
            ExprKind::TypeApply { callee, arguments } => {
                let Some(binding) = expression_binding(callee, environment) else {
                    return Ok(Type::Unknown);
                };
                (callable_name(callee), Some(binding), Some(arguments))
            }
            _ => (
                callable_name(callee),
                expression_binding(callee, environment),
                None,
            ),
        };
        let Some(binding) = binding else {
            return Ok(Type::Unknown);
        };
        if binding.callables.is_empty() {
            return check_function_value_call(
                &binding.value_type,
                &shapes,
                &actuals,
                &expression.span,
            );
        }
        let callables = binding.callables.clone();
        if callables.len() > 1
            && shapes
                .iter()
                .any(|shape| matches!(shape, ArgumentShape::Spread))
        {
            return Err(SourceError::semantic(
                SourceErrorCode::NoMatchingOverload,
                format!("cannot resolve overload `{name}` with spread arguments"),
                expression.span.clone(),
            )
            .with_extent(expression));
        }
        let mut applicable = Vec::new();
        for signature in &callables {
            if let Some(candidate) = instantiate_candidate(
                signature,
                &shapes,
                &actuals,
                explicit,
                type_parameters,
                &expression.span,
                environment,
                callables.len() == 1,
            )? {
                applicable.push(candidate);
            }
        }
        if applicable.is_empty() {
            if callables.len() == 1
                && shapes
                    .iter()
                    .any(|shape| matches!(shape, ArgumentShape::Spread))
            {
                // The runtime binder remains responsible for a dynamic spread's
                // arity. It cannot prove parameter or generic relationships, so
                // do not report a successfully instantiated callable here.
                return Ok(Type::Unknown);
            }
            return Err(SourceError::semantic(
                SourceErrorCode::NoMatchingOverload,
                format!("no matching overload for `{name}`"),
                expression.span.clone(),
            )
            .with_extent(expression));
        }
        let most_specific = applicable
            .iter()
            .enumerate()
            .filter(|(index, candidate)| {
                !applicable.iter().enumerate().any(|(other_index, other)| {
                    index != &other_index && more_specific(other, candidate)
                })
            })
            .map(|(_, candidate)| candidate)
            .collect::<Vec<_>>();
        if most_specific.len() != 1 {
            return Err(SourceError::semantic(
                SourceErrorCode::AmbiguousOverload,
                format!("ambiguous overload for `{name}`"),
                expression.span.clone(),
            )
            .with_extent(expression));
        }
        if callables.len() > 1 {
            environment
                .record_selected_call(expression.span.clone(), most_specific[0].identity.clone());
        }
        Ok(most_specific[0].result.clone())
    })
}

fn check_function_value_call(
    value_type: &Type,
    shapes: &[ArgumentShape<'_>],
    actuals: &[Type],
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    let Type::Function(Some(signature)) = value_type else {
        return Ok(Type::Unknown);
    };
    let Some((result, parameters)) = signature.split_first() else {
        return Ok(Type::Unknown);
    };
    if shapes
        .iter()
        .any(|shape| !matches!(shape, ArgumentShape::Positional))
    {
        return Ok(Type::Unknown);
    }
    // Structural function types retain only a result and positional parameter
    // types. They cannot prove whether omitted inputs have defaults or whether
    // a variadic parameter accepts extras, so an arity mismatch stays dynamic.
    if actuals.len() != parameters.len() {
        return Ok(Type::Unknown);
    }
    for (expected, actual) in parameters.iter().zip(actuals) {
        require(expected, &actual.clone().widen_unknown(), span)?;
    }
    Ok(result.clone().widen_unknown())
}

fn callable_name(expression: &Expr) -> String {
    match &expression.kind {
        ExprKind::Name(name) => name.clone(),
        ExprKind::Index { index, .. } => match &index.kind {
            ExprKind::Value(Value::Str(name)) => name.to_string(),
            _ => "<computed>".into(),
        },
        _ => "<callable>".into(),
    }
}

fn check_argument(
    argument: &CallArgument,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    super::stack::grow(|| {
        let value = match argument {
            CallArgument::Positional(value)
            | CallArgument::Named { value, .. }
            | CallArgument::Spread(value) => value,
        };
        check_expression(value, environment, type_parameters)
    })
}

struct InstantiatedCandidate {
    bound_types: Vec<Type>,
    generic_arity: usize,
    non_variadic: bool,
    uses_empty_variadic: bool,
    result: Type,
    identity: CallableIdentity,
}

#[derive(Clone, Copy)]
enum ArgumentShape<'a> {
    Positional,
    Named(&'a str),
    Spread,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn instantiate_candidate(
    signature: &CallableSignature,
    arguments: &[ArgumentShape<'_>],
    actuals: &[Type],
    explicit: Option<&Vec<TypeAnnotation>>,
    type_parameters: &[String],
    span: &SourceSpan,
    environment: &Environment,
    report_mismatch: bool,
) -> Result<Option<InstantiatedCandidate>, SourceError> {
    let mut substitutions = HashMap::new();
    if let Some(explicit) = explicit {
        if explicit.len() != signature.generic_arity {
            if report_mismatch {
                return Err(SourceError::semantic(
                    SourceErrorCode::InvalidTypeArguments,
                    "wrong number of type arguments",
                    span.clone(),
                ));
            }
            return Ok(None);
        }
        for (index, value) in explicit.iter().enumerate() {
            let value = resolve_static_annotation(value, type_parameters, span, environment)?;
            if value.includes_nil() {
                return Err(SourceError::semantic(
                    SourceErrorCode::InvalidTypeArguments,
                    "generic type argument cannot include nil",
                    span.clone(),
                ));
            }
            substitutions.insert(index, value);
        }
    }
    let Some(bound) = bind_arguments(&signature.parameters, arguments, actuals) else {
        return Ok(None);
    };
    if !signature.inferred_alternatives.is_empty()
        && bound.values.len() == signature.parameters.len()
        && !bound
            .values
            .iter()
            .any(|(_, _, actual)| is_dynamic_operation_type(actual))
    {
        let mut alternative_actuals = vec![Type::Unknown; signature.parameters.len()];
        for (index, _, actual) in &bound.values {
            alternative_actuals[*index] = (*actual).clone();
        }
        let alternatives = signature
            .inferred_alternatives
            .iter()
            .filter_map(|alternative| alternative.instantiate(&alternative_actuals))
            .collect::<Vec<_>>();
        if alternatives.len() != 1 {
            if report_mismatch {
                return Err(SourceError::semantic(
                    SourceErrorCode::NoMatchingOverload,
                    if alternatives.is_empty() {
                        "no inferred overload alternative matches the call"
                    } else {
                        "ambiguous inferred overload alternative"
                    },
                    span.clone(),
                ));
            }
            return Ok(None);
        }
        let (bound_types, result) = alternatives.into_iter().next().expect("one alternative");
        return Ok(Some(InstantiatedCandidate {
            bound_types,
            generic_arity: signature.generic_arity,
            non_variadic: !signature
                .parameters
                .last()
                .is_some_and(|parameter| parameter.variadic),
            uses_empty_variadic: bound.uses_empty_variadic,
            result,
            identity: signature.identity(),
        }));
    }
    for (_, parameter, actual) in &bound.values {
        let actual = (*actual).clone();
        if is_dynamic_operation_type(&actual) {
            continue;
        }
        if let Err(error) = infer(&parameter.value_type, &actual, &mut substitutions, span) {
            if report_mismatch {
                if let Some(index) = first_generic_parameter(&parameter.value_type)
                    && let Some(inferred) = substitutions.get(&index)
                {
                    let message = format!(
                        "{} (generic T{index} was inferred as {})",
                        error.message,
                        inferred.diagnostic_display()
                    );
                    return Err(SourceError {
                        expected: error.expected,
                        actual: error.actual,
                        ..SourceError::semantic(error.code, message, span.clone())
                    });
                }
                return Err(error);
            }
            return Ok(None);
        }
    }
    let bound_types = bound
        .values
        .iter()
        .map(|(_, parameter, _)| substitute(&parameter.value_type, &substitutions).widen_unknown())
        .collect();
    Ok(Some(InstantiatedCandidate {
        bound_types,
        generic_arity: signature.generic_arity,
        non_variadic: !signature
            .parameters
            .last()
            .is_some_and(|parameter| parameter.variadic),
        uses_empty_variadic: bound.uses_empty_variadic,
        result: substitute(&signature.result, &substitutions),
        identity: signature.identity(),
    }))
}

fn first_generic_parameter(value_type: &Type) -> Option<usize> {
    match value_type {
        Type::Generic(index) => Some(*index),
        Type::List(Some(value)) | Type::Task(Some(value)) | Type::Channel(Some(value)) => {
            first_generic_parameter(value)
        }
        Type::Map(Some((key, value))) => {
            first_generic_parameter(key).or_else(|| first_generic_parameter(value))
        }
        Type::Function(Some(values)) | Type::Tuple(values) | Type::Union(values) => {
            values.iter().find_map(first_generic_parameter)
        }
        Type::Never
        | Type::Unknown
        | Type::Any
        | Type::Nil
        | Type::Bool
        | Type::Num
        | Type::Str
        | Type::Bytes
        | Type::Resource(_)
        | Type::Enum(_)
        | Type::List(None)
        | Type::Map(None)
        | Type::Function(None)
        | Type::Task(None)
        | Type::Channel(None)
        | Type::Schema
        | Type::Struct(_) => None,
    }
}

struct BoundArguments<'a> {
    values: Vec<(usize, &'a CallableParameter, &'a Type)>,
    uses_empty_variadic: bool,
}

fn bind_arguments<'a>(
    parameters: &'a [CallableParameter],
    arguments: &[ArgumentShape<'_>],
    actuals: &'a [Type],
) -> Option<BoundArguments<'a>> {
    if arguments
        .iter()
        .any(|argument| matches!(argument, ArgumentShape::Spread))
    {
        if parameters.len() == 1
            && parameters[0].variadic
            && matches!(arguments, [ArgumentShape::Spread])
            && let Type::List(Some(element)) = &actuals[0]
        {
            return Some(BoundArguments {
                values: vec![(0, &parameters[0], element.as_ref())],
                uses_empty_variadic: false,
            });
        }
        return None;
    }
    let variadic = parameters
        .last()
        .is_some_and(|parameter| parameter.variadic);
    let fixed = parameters.len() - usize::from(variadic);
    let mut assigned = vec![false; parameters.len()];
    let mut bound = Vec::new();
    let mut positional = 0usize;
    let mut variadic_supplied = false;
    for (argument, actual) in arguments.iter().zip(actuals) {
        let index = match argument {
            ArgumentShape::Positional => {
                let index = if positional < fixed {
                    positional
                } else if variadic {
                    parameters.len() - 1
                } else {
                    return None;
                };
                positional += 1;
                if index < fixed {
                    assigned[index] = true;
                } else {
                    variadic_supplied = true;
                }
                index
            }
            ArgumentShape::Named(name) => {
                let index = parameters
                    .iter()
                    .position(|parameter| parameter.label.as_deref() == Some(*name))?;
                if assigned[index] {
                    return None;
                }
                assigned[index] = true;
                if parameters[index].variadic {
                    variadic_supplied = true;
                }
                index
            }
            ArgumentShape::Spread => unreachable!("spread calls were handled above"),
        };
        bound.push((index, &parameters[index], actual));
    }
    if parameters
        .iter()
        .enumerate()
        .any(|(index, parameter)| !assigned[index] && !parameter.has_default && !parameter.variadic)
    {
        return None;
    }
    Some(BoundArguments {
        values: bound,
        uses_empty_variadic: variadic && !variadic_supplied,
    })
}

fn more_specific(left: &InstantiatedCandidate, right: &InstantiatedCandidate) -> bool {
    if left.bound_types.len() != right.bound_types.len()
        || !left
            .bound_types
            .iter()
            .zip(&right.bound_types)
            .all(|(left, right)| left.is_assignable_to(right))
    {
        return false;
    }
    let type_or_generic_more_specific = left
        .bound_types
        .iter()
        .zip(&right.bound_types)
        .any(|(left, right)| !right.is_assignable_to(left))
        || left.generic_arity < right.generic_arity;
    type_or_generic_more_specific || (left.non_variadic && right.uses_empty_variadic)
}

fn infer(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    if let Type::Generic(index) = expected {
        if matches!(actual, Type::Unknown) {
            return Ok(());
        }
        if actual.includes_nil() {
            return Err(SourceError::semantic(
                SourceErrorCode::InvalidTypeArguments,
                "generic type argument cannot include nil",
                span.clone(),
            ));
        }
        if let Some(previous) = substitutions.get(index) {
            return require(previous, actual, span);
        }
        substitutions.insert(*index, actual.clone());
        return Ok(());
    }
    match (expected, actual) {
        (Type::Union(expected), actual) => infer_union(expected, actual, substitutions, span),
        (Type::List(Some(expected)), Type::List(Some(actual))) => {
            infer(expected, actual, substitutions, span)
        }
        (Type::List(Some(_)), Type::Tuple(_)) => {
            infer(expected, &actual.as_list(), substitutions, span)
        }
        (Type::Channel(Some(expected)), Type::Channel(Some(actual))) => {
            infer_channel_payload(expected, actual, substitutions, span)
        }
        (Type::Task(Some(expected)), Type::Task(Some(actual))) => {
            infer_task_payload(expected, actual, substitutions, span)
        }
        (
            Type::Map(Some((expected_key, expected_value))),
            Type::Map(Some((actual_key, actual_value))),
        ) => {
            infer(expected_key, actual_key, substitutions, span)?;
            infer(expected_value, actual_value, substitutions, span)
        }
        (Type::Tuple(expected), Type::Tuple(actual))
        | (Type::Function(Some(expected)), Type::Function(Some(actual)))
            if expected.len() == actual.len() =>
        {
            for (expected, actual) in expected.iter().zip(actual) {
                infer(expected, actual, substitutions, span)?;
            }
            Ok(())
        }
        _ => require(&substitute(expected, substitutions), actual, span),
    }
}

fn infer_union(
    expected: &[Type],
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    let mut remaining = match actual {
        Type::Union(members) => members.clone(),
        actual => vec![actual.clone()],
    };
    for expected in expected {
        if matches!(expected, Type::Generic(_)) {
            continue;
        }
        let Some(index) = remaining.iter().position(|actual| {
            let mut trial = substitutions.clone();
            infer(expected, actual, &mut trial, span).is_ok()
        }) else {
            continue;
        };
        let actual = remaining.remove(index);
        infer(expected, &actual, substitutions, span)?;
    }
    for expected in expected {
        if let Type::Generic(index) = expected {
            let actual = Type::union(remaining.drain(..));
            if !matches!(actual, Type::Never) {
                infer(&Type::Generic(*index), &actual, substitutions, span)?;
            }
        }
    }
    require(
        &substitute(&Type::union(expected.iter().cloned()), substitutions),
        actual,
        span,
    )
}

fn infer_task_payload(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    infer_payload(expected, actual, substitutions, span, false)
}

fn infer_channel_payload(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &SourceSpan,
) -> Result<(), SourceError> {
    infer_payload(expected, actual, substitutions, span, true)
}

fn infer_payload(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &SourceSpan,
    preserve_unknown: bool,
) -> Result<(), SourceError> {
    let Type::Generic(index) = expected else {
        return infer(expected, actual, substitutions, span);
    };
    if matches!(actual, Type::Unknown) || (preserve_unknown && actual == &Type::universal()) {
        substitutions.entry(*index).or_insert(Type::Unknown);
        return Ok(());
    }
    if let Some(previous) = substitutions.get(index) {
        return require(previous, actual, span);
    }
    substitutions.insert(*index, actual.clone());
    Ok(())
}

fn substitute(value_type: &Type, substitutions: &HashMap<usize, Type>) -> Type {
    match value_type {
        Type::Generic(index) => substitutions.get(index).cloned().unwrap_or(Type::Unknown),
        Type::List(argument) => Type::List(
            argument
                .as_ref()
                .map(|argument| Box::new(substitute(argument, substitutions))),
        ),
        Type::Map(arguments) => Type::Map(arguments.as_ref().map(|(key, value)| {
            (
                Box::new(substitute(key, substitutions)),
                Box::new(substitute(value, substitutions)),
            )
        })),
        Type::Function(signature) => Type::Function(signature.as_ref().map(|signature| {
            signature
                .iter()
                .map(|value| substitute(value, substitutions))
                .collect()
        })),
        Type::Task(result) => Type::Task(
            result
                .as_ref()
                .map(|result| Box::new(substitute(result, substitutions))),
        ),
        Type::Channel(value) => Type::Channel(
            value
                .as_ref()
                .map(|value| Box::new(substitute(value, substitutions))),
        ),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute(element, substitutions))
                .collect(),
        ),
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|member| substitute(member, substitutions)),
        ),
        other => other.clone(),
    }
}

fn known_schema_identity(expression: &Expr, environment: &Environment) -> Option<SchemaIdentity> {
    let ExprKind::Name(name) = &expression.kind else {
        return None;
    };
    environment.lookup(name).and_then(|binding| {
        (binding.value_type == Type::Schema)
            .then(|| binding.schema_identity.clone())
            .flatten()
    })
}

fn value_type(value: &Value) -> Type {
    if let Value::Enum(value) = value {
        return Type::Enum(super::semantic::EnumIdentity::declared(
            value.module.as_ref(),
            value.name.as_ref(),
        ));
    }
    match value.kind() {
        ValueKind::Nil => Type::Nil,
        ValueKind::Bool => Type::Bool,
        ValueKind::Num => Type::Num,
        ValueKind::Str => Type::Str,
        ValueKind::Bytes => Type::Bytes,
        ValueKind::List => Type::List(None),
        ValueKind::Map => Type::Map(None),
        ValueKind::StructSchema => Type::Schema,
        ValueKind::Struct => Type::Struct(None),
        ValueKind::Enum => unreachable!("enum values are handled before kind conversion"),
        ValueKind::Channel => Type::Channel(None),
        ValueKind::Function => Type::Function(None),
        ValueKind::Task => Type::Task(None),
        ValueKind::Unknown => Type::Unknown,
    }
}

fn require(expected: &Type, actual: &Type, span: &SourceSpan) -> Result<(), SourceError> {
    if actual.is_assignable_to(expected) {
        Ok(())
    } else {
        // Where no tuple is expected, a tuple is shown as the list it also
        // is: `expected list<num>, got list<num|str>`.
        let shown = if expected.to_string().contains('[') {
            actual.clone()
        } else {
            actual.widen_tuples()
        };
        let actual = &shown;
        let expected_display = expected.to_string();
        let actual_display = actual.to_string();
        let (expected_display, actual_display) = if expected_display == actual_display {
            (expected.diagnostic_display(), actual.diagnostic_display())
        } else {
            (expected_display, actual_display)
        };
        Err(SourceError::semantic(
            SourceErrorCode::TypeMismatch,
            format!("expected {expected_display}, got {actual_display}"),
            span.clone(),
        )
        .with_expected_actual(expected_display, actual_display))
    }
}

/// Explains a mismatch whose value is a `var` that started as `nil`. Its
/// declared type then comes from its first assignment, which the message
/// names; with none of known type it is `any|nil`, and a nested function,
/// which may run after any assignment, sees only that, so the fix is to
/// declare the type it holds.
fn explain_nil_start(
    mut error: SourceError,
    value: &Expr,
    environment: &Environment,
) -> SourceError {
    let ExprKind::Name(name) = &value.kind else {
        return error;
    };
    if !matches!(error.code, SourceErrorCode::TypeMismatch) {
        return error;
    }
    let Some(binding) = environment.lookup(name) else {
        return error;
    };
    if binding.nil_start.is_none() {
        return error;
    }
    if let Some(origin) = nil_start_origin(name, binding) {
        error.message = format!("{}: {origin}", error.message);
        return error;
    }
    let Some(expected) = error.expected.clone() else {
        return error;
    };
    let declared = if expected.split('|').any(|member| member == "nil") {
        expected.to_string()
    } else {
        format!("{expected}|nil")
    };
    error.message = format!(
        "{}: `{name}` starts as `nil`, so its type is `any|nil`; declare the type it holds, as in `var {name}:{declared} = nil`",
        error.message
    );
    error
}

/// Explains a mismatch in an assignment to a `var` that started as `nil`,
/// whose declared type its first assignment set: it names that assignment
/// and the declaration that would hold both types.
fn explain_nil_start_assignment(
    mut error: SourceError,
    name: &str,
    binding: &SemanticBinding,
    actual: &Type,
) -> SourceError {
    let Some(origin) = nil_start_origin(name, binding) else {
        return error;
    };
    if !matches!(error.code, SourceErrorCode::TypeMismatch) {
        return error;
    }
    let declared = binding.declared.clone().unwrap_or(Type::universal());
    let both = Type::union([declared, actual.clone()]);
    error.message = format!(
        "{}: {origin}; to hold both, declare `var {name}:{both} = nil`",
        error.message
    );
    error
}

/// Where a `nil`-started `var`'s declared type came from, when its first
/// assignment set it.
fn nil_start_origin(name: &str, binding: &SemanticBinding) -> Option<String> {
    let (span, _) = binding.nil_start.as_ref()?.assignments.first()?;
    let declared = binding.declared.clone().unwrap_or(Type::universal());
    Some(format!(
        "`{name}` is `{declared}`, from its first value `nil` and its first assignment at {}:{}",
        span.line, span.column
    ))
}

fn validate_expression(expression: &Expr, type_parameters: &[String]) -> Result<(), SourceError> {
    super::stack::grow(|| {
        validate_expression_inner(expression, type_parameters)
            .map_err(|error| error.claimed_by(expression))
    })
}

#[allow(clippy::too_many_lines)]
fn validate_expression_inner(
    expression: &Expr,
    type_parameters: &[String],
) -> Result<(), SourceError> {
    super::stack::grow(|| match &expression.kind {
        ExprKind::Declare {
            annotation,
            value,
            tags,
            ..
        } => {
            if let Some(annotation) = annotation {
                resolve_annotation(annotation, type_parameters, &expression.span)?;
            }
            validate_tags(tags, type_parameters)?;
            validate_expression(value, type_parameters)
        }
        ExprKind::Foreign {
            signature, tags, ..
        } => {
            validate_tags(tags, type_parameters)?;
            for parameter in &signature.parameters {
                validate_parameter(parameter, &signature.type_parameters, &expression.span)?;
            }
            if let Some(result) = &signature.return_annotation {
                resolve_annotation(result, &signature.type_parameters, &expression.span)?;
            }
            Ok(())
        }
        ExprKind::Function {
            type_parameters: function_type_parameters,
            parameters,
            return_annotation,
            body,
        } => {
            for parameter in parameters {
                validate_parameter(parameter, function_type_parameters, &expression.span)?;
            }
            if let Some(result) = return_annotation {
                resolve_annotation(result, function_type_parameters, &expression.span)?;
            }
            validate_expression(body, function_type_parameters)
        }
        ExprKind::TypeApply { callee, arguments } => {
            validate_expression(callee, type_parameters)?;
            for argument in arguments {
                resolve_annotation(argument, type_parameters, &expression.span)?;
            }
            Ok(())
        }
        ExprKind::Assign { value, .. }
        | ExprKind::Return { value }
        | ExprKind::Throw { value }
        | ExprKind::Defer { value, .. }
        | ExprKind::Spawn(value)
        | ExprKind::Prefix { value, .. } => validate_expression(value, type_parameters),
        ExprKind::Recur(arguments) => validate_arguments(arguments, type_parameters),
        ExprKind::Nursery { limit, body } => {
            if let Some(limit) = limit {
                validate_expression(limit, type_parameters)?;
            }
            validate_expression(body, type_parameters)
        }
        ExprKind::Select(cases) => {
            for case in cases {
                match &case.kind {
                    SelectCaseKind::Receive(value)
                    | SelectCaseKind::After(value)
                    | SelectCaseKind::Await(value) => {
                        validate_expression(value, type_parameters)?;
                    }
                    SelectCaseKind::Send { channel, value } => {
                        validate_expression(channel, type_parameters)?;
                        validate_expression(value, type_parameters)?;
                    }
                    SelectCaseKind::Default => {}
                }
                if let Some(handler) = &case.handler {
                    validate_expression(handler, type_parameters)?;
                }
            }
            Ok(())
        }
        ExprKind::Match { subject, cases } => {
            if let Some(subject) = subject {
                validate_expression(subject, type_parameters)?;
            }
            for case in cases {
                for pattern in &case.patterns {
                    validate_case_pattern(pattern, type_parameters, &case.span)?;
                }
                if let Some(guard) = &case.guard {
                    validate_expression(guard, type_parameters)?;
                }
                validate_expression(&case.value, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::Binary { left, right, .. } => {
            validate_expression(left, type_parameters)?;
            validate_expression(right, type_parameters)
        }
        ExprKind::Call { callee, arguments } => {
            validate_expression(callee, type_parameters)?;
            validate_arguments(arguments, type_parameters)
        }
        ExprKind::Block(values) => {
            for value in values {
                validate_expression(value, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            validate_expression(condition, type_parameters)?;
            validate_expression(then_branch, type_parameters)?;
            if let Some(else_branch) = else_branch {
                validate_expression(else_branch, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::List(values) => {
            for value in values {
                let value = match value {
                    ListElement::Value(value) | ListElement::Spread(value) => value,
                };
                validate_expression(value, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::Map(entries) => {
            for (key, value) in entries {
                validate_expression(key, type_parameters)?;
                validate_expression(value, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let Some(annotation) = &field.annotation {
                    resolve_annotation(annotation, type_parameters, &expression.span)?;
                }
                if let Some(default) = &field.default {
                    validate_expression(default, type_parameters)?;
                }
            }
            Ok(())
        }
        ExprKind::StructInit { schema, fields } => {
            validate_expression(schema, type_parameters)?;
            for (_, value) in fields {
                validate_expression(value, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::StructCopy { value, fields } => {
            validate_expression(value, type_parameters)?;
            for (_, replacement) in fields {
                validate_expression(replacement, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::Index { collection, index } => {
            validate_expression(collection, type_parameters)?;
            validate_expression(index, type_parameters)
        }
        ExprKind::Slice {
            collection,
            start,
            end,
            step,
        } => {
            validate_expression(collection, type_parameters)?;
            for bound in [start, end, step].into_iter().flatten() {
                validate_expression(bound, type_parameters)?;
            }
            Ok(())
        }
        ExprKind::Resource { .. }
        | ExprKind::Enum { .. }
        | ExprKind::TypeAlias { .. }
        | ExprKind::Value(_)
        | ExprKind::Interpolate(_)
        | ExprKind::Documentation(_)
        | ExprKind::NotImplemented
        | ExprKind::Name(_) => Ok(()),
    })
}

fn validate_parameter(
    parameter: &Parameter,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        if let Some(annotation) = &parameter.annotation {
            resolve_annotation(annotation, type_parameters, span)?;
        }
        validate_tags(&parameter.tags, type_parameters)?;
        if let Some(default) = &parameter.default {
            validate_expression(default, type_parameters)?;
        }
        Ok(())
    })
}

fn validate_tags(tags: &[Tag], type_parameters: &[String]) -> Result<(), SourceError> {
    super::stack::grow(|| {
        for tag in tags {
            for argument in &tag.arguments {
                validate_expression(argument, type_parameters)?;
            }
        }
        Ok(())
    })
}

fn validate_arguments(
    arguments: &[CallArgument],
    type_parameters: &[String],
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        for argument in arguments {
            let value = match argument {
                CallArgument::Positional(value)
                | CallArgument::Named { value, .. }
                | CallArgument::Spread(value) => value,
            };
            validate_expression(value, type_parameters)?;
        }
        Ok(())
    })
}

fn validate_case_pattern(
    pattern: &CasePattern,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        if let Some(constraint) = &pattern.constraint {
            let constraint = resolve_annotation(constraint, type_parameters, span)?;
            if !constraint.is_reifiable_match_constraint() {
                return Err(SourceError::semantic(
                    SourceErrorCode::InvalidMatch,
                    "match type constraint is not runtime-checkable",
                    span.clone(),
                ));
            }
        }
        validate_pattern(&pattern.pattern, type_parameters)
    })
}

fn validate_pattern(pattern: &Pattern, type_parameters: &[String]) -> Result<(), SourceError> {
    super::stack::grow(|| match pattern {
        Pattern::At { pattern, .. } => validate_pattern(pattern, type_parameters),
        Pattern::List { items, .. } => {
            for item in items {
                validate_pattern(item, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Map { entries, .. } => {
            for (key, value) in entries {
                if let MapPatternKey::Computed(key) = key {
                    validate_expression(key, type_parameters)?;
                }
                validate_pattern(value, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Binding(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => Ok(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> SourceSpan {
        SourceSpan::new("test.slug", 1, 1)
    }

    fn test_end() -> crate::source::SourcePosition {
        crate::source::SourcePosition { line: 1, column: 2 }
    }

    fn name(name: &str) -> Expr {
        Expr::new(ExprKind::Name(name.into()), span(), test_end())
    }

    fn parameter(name: &str) -> Parameter {
        Parameter {
            name: name.into(),
            discard: false,
            tags: Vec::new(),
            annotation: None,
            default: None,
            variadic: false,
        }
    }

    fn analyzed_signatures(source: &str) -> HashMap<String, CallableSignature> {
        let tokens = super::super::Lexer::new("test.slug", source)
            .tokens()
            .expect("source lexes");
        let expressions = super::super::Parser::new(tokens)
            .parse()
            .expect("source parses");
        analyze_with_imports(&expressions, HashMap::new(), None)
            .expect("source analyzes")
            .snapshot
            .exports
            .into_iter()
            .map(|(name, binding)| {
                let signature = binding
                    .callables
                    .into_iter()
                    .next()
                    .expect("exported function has one signature");
                (name, signature)
            })
            .collect()
    }

    #[test]
    fn inferred_signatures_propagate_direct_calls() {
        let signatures = analyzed_signatures(
            "export val divide = fn(value) { value / 10 }\n\
             export val apply = fn(value) { divide(value) }\n",
        );

        for name in ["divide", "apply"] {
            let signature = &signatures[name];
            assert_eq!(signature.parameters[0].value_type, Type::Num);
            assert_eq!(signature.result, Type::Num);
        }
    }

    #[test]
    fn inferred_signatures_join_nil_partitions() {
        let signatures = analyzed_signatures(
            "export val divide = fn(value) { value / 10 }\n\
             export val nil_first = fn(value) { if (value == nil) { 0 } else { value / 10 } }\n\
             export val non_nil_first = fn(value) { if (value != nil) { value / 10 } else { 0 } }\n\
             export val wrapped = fn(value) { if (value == nil) { 0 } else { divide(value) } }\n",
        );

        for name in ["nil_first", "non_nil_first", "wrapped"] {
            let signature = &signatures[name];
            assert_eq!(
                signature.parameters[0].value_type,
                Type::union([Type::Nil, Type::Num])
            );
            assert_eq!(signature.result, Type::Num);
        }
    }

    #[test]
    fn cyclic_unannotated_calls_do_not_create_body_derived_constraints() {
        let signatures = analyzed_signatures(
            "export val self = fn(value) { self(value) }\n\
             export val left = fn(value) { right(value) }\n\
             export val right = fn(value) { left(value) }\n",
        );

        for name in ["self", "left", "right"] {
            let signature = &signatures[name];
            assert_eq!(signature.parameters[0].value_type, Type::universal());
            assert_eq!(signature.result, Type::Unknown);
        }
    }

    fn binary(left: Expr, operator: Binary, right: Expr) -> Expr {
        Expr::new(
            ExprKind::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            },
            span(),
            test_end(),
        )
    }

    fn candidate(bound_types: Vec<Type>) -> InstantiatedCandidate {
        InstantiatedCandidate {
            bound_types,
            generic_arity: 0,
            non_variadic: true,
            uses_empty_variadic: false,
            result: Type::Unknown,
            identity: CallableSignature {
                generic_arity: 0,
                parameters: Vec::new(),
                result: Type::Unknown,
                inferred_alternatives: Vec::new(),
            }
            .identity(),
        }
    }

    #[test]
    fn parameter_constraints_follow_control_flow_paths() {
        let negate = || {
            Expr::new(
                ExprKind::Prefix {
                    operators: vec![(Prefix::Negate, span())],
                    value: Box::new(name("value")),
                },
                span(),
                test_end(),
            )
        };
        let body = |else_branch: Option<Expr>| {
            Expr::new(
                ExprKind::If {
                    condition: Box::new(binary(name("value"), Binary::Less, name("limit"))),
                    then_branch: Box::new(negate()),
                    else_branch: else_branch.map(Box::new),
                },
                span(),
                test_end(),
            )
        };
        let parameters = [parameter("value"), parameter("limit")];

        // Both paths negate `value`, so it must be a number.
        let constraints =
            parameter_constraints(&parameters, &body(Some(negate())), &Environment::new())
                .expect("numeric body requirements collect");
        assert_eq!(constraints.requirements["value"].value_type, Type::Num);
        // Ordering two parameters may compare numbers or strings, so only the
        // negation constrains `value`, and `limit` stays unconstrained.
        assert_eq!(constraints.requirements["limit"].value_type, Type::Unknown);

        // Without an `else`, a path that does not negate `value` finishes, so
        // nothing requires it to be a number.
        let constraints = parameter_constraints(&parameters, &body(None), &Environment::new())
            .expect("numeric body requirements collect");
        assert_eq!(constraints.requirements["value"].value_type, Type::Unknown);
    }

    #[test]
    fn inferred_plus_alternatives_cover_the_five_runtime_families() {
        let body = binary(name("left"), Binary::Add, name("right"));
        let constraints = parameter_constraints(
            &[parameter("left"), parameter("right")],
            &body,
            &Environment::new(),
        )
        .expect("plus operands collect");
        let alternatives =
            inferred_plus_alternatives(&[parameter("left"), parameter("right")], &constraints)
                .expect("unconstrained plus body retains every family");

        assert_eq!(
            alternatives
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec![
                "(num, num) -> num",
                "(str, A0) -> str",
                "(list<A0>, list<A1>) -> list<A0|A1>",
                "(bytes, bytes) -> bytes",
                "(map<A0, A1>, map<A2, A3>) -> map<A0|A2, A1|A3>",
            ]
        );
    }

    #[test]
    fn inferred_plus_alternatives_intersect_numeric_body_facts() {
        let body = Expr::new(
            ExprKind::Block(vec![
                binary(name("left"), Binary::Add, name("right")),
                binary(name("left"), Binary::Divide, name("right")),
            ]),
            span(),
            test_end(),
        );
        let constraints = parameter_constraints(
            &[parameter("left"), parameter("right")],
            &body,
            &Environment::new(),
        )
        .expect("numeric body requirements collect");
        let alternatives =
            inferred_plus_alternatives(&[parameter("left"), parameter("right")], &constraints)
                .expect("numeric requirement retains the numeric alternative");

        assert_eq!(
            alternatives
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["(num, num) -> num"]
        );
    }

    #[test]
    fn inferred_subtract_alternatives_preserve_map_keys_without_unifying_them() {
        let body = binary(name("map"), Binary::Subtract, name("key"));
        let constraints = parameter_constraints(
            &[parameter("map"), parameter("key")],
            &body,
            &Environment::new(),
        )
        .expect("subtract operands collect");
        let alternatives =
            inferred_subtract_alternatives(&[parameter("map"), parameter("key")], &constraints)
                .expect("unconstrained subtraction body retains both families");

        assert_eq!(
            alternatives
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["(num, num) -> num", "(map<A0, A1>, map-key) -> map<A0, A1>"]
        );
        let map = Type::Map(Some((Box::new(Type::Num), Box::new(Type::Str))));
        assert!(
            alternatives[1]
                .instantiate(&[map.clone(), Type::union([Type::Num, Type::Str])])
                .is_some()
        );
        assert!(
            alternatives[1]
                .instantiate(&[map, Type::List(Some(Box::new(Type::Num)))])
                .is_none()
        );
    }

    #[test]
    fn parameter_constraints_include_defaults_and_recur_arguments() {
        let mut value = parameter("value");
        value.default = Some(Expr::new(
            ExprKind::Binary {
                left: Box::new(name("value")),
                operator: Binary::Divide,
                right: Box::new(name("other")),
            },
            span(),
            test_end(),
        ));
        let body = Expr::new(
            ExprKind::Recur(vec![CallArgument::Positional(Expr::new(
                ExprKind::Binary {
                    left: Box::new(name("other")),
                    operator: Binary::Modulo,
                    right: Box::new(name("value")),
                },
                span(),
                test_end(),
            ))]),
            span(),
            test_end(),
        );
        let constraints =
            parameter_constraints(&[value, parameter("other")], &body, &Environment::new())
                .expect("defaults and recur expressions collect");

        assert_eq!(constraints.requirements["value"].value_type, Type::Num);
        assert_eq!(constraints.requirements["other"].value_type, Type::Num);
    }

    #[test]
    fn parameter_constraints_exclude_nested_and_ineligible_parameters() {
        let mut explicit = parameter("explicit");
        explicit.annotation = Some(TypeAnnotation::Name("str".into()));
        let mut rest = parameter("rest");
        rest.variadic = true;
        let mut discard = parameter("_");
        discard.discard = true;
        let body = Expr::new(
            ExprKind::Block(vec![
                Expr::new(
                    ExprKind::Binary {
                        left: Box::new(name("explicit")),
                        operator: Binary::Divide,
                        right: Box::new(name("rest")),
                    },
                    span(),
                    test_end(),
                ),
                Expr::new(
                    ExprKind::Function {
                        type_parameters: Vec::new(),
                        parameters: vec![parameter("outer")],
                        return_annotation: None,
                        body: Box::new(Expr::new(
                            ExprKind::Binary {
                                left: Box::new(name("outer")),
                                operator: Binary::Divide,
                                right: Box::new(name("outer")),
                            },
                            span(),
                            test_end(),
                        )),
                    },
                    span(),
                    test_end(),
                ),
            ]),
            span(),
            test_end(),
        );
        let constraints = parameter_constraints(
            &[parameter("outer"), explicit, rest, discard],
            &body,
            &Environment::new(),
        )
        .expect("ineligible and nested requirements are ignored");

        assert!(matches!(
            constraints.requirements["outer"].value_type,
            Type::Unknown
        ));
        assert!(!constraints.requirements.contains_key("explicit"));
        assert!(!constraints.requirements.contains_key("rest"));
        assert!(!constraints.requirements.contains_key("_"));
    }

    #[test]
    fn solved_parameter_signature_rejects_an_incompatible_direct_call() {
        let error = super::super::compile(
            "test.slug",
            "val divide = fn(value) { value / 10 }\ndivide(\"no\")",
        )
        .expect_err("a known string argument cannot satisfy the inferred num parameter");

        assert_eq!(error.kind, super::super::SourceErrorKind::Semantic);
        assert!(error.message.contains("expected num, got str"));
    }

    #[test]
    fn solved_parameter_facts_select_numeric_lowering_opcodes() {
        let program = super::super::compile(
            "test.slug",
            // A constant on the left keeps each operator a standalone
            // instruction; `value / 10` would fuse into `LocalConstantBinary`.
            "val divide = fn(value) { 10 / value }\nval before = fn(value) { 10 < value }",
        )
        .expect("numeric body constraints compile");
        let opcodes = (0..program.chunk_count())
            .flat_map(|index| {
                program
                    .chunk(index)
                    .expect("existing chunk")
                    .code
                    .iter()
                    .map(|instruction| instruction.opcode)
            })
            .collect::<Vec<_>>();

        assert!(opcodes.contains(&slug_vm::PackedOpcode::DivideNum));
        assert!(opcodes.contains(&slug_vm::PackedOpcode::LessNum));
    }

    #[test]
    fn local_constant_operations_fuse_unless_a_jump_lands_inside() {
        let opcodes = |source: &str| {
            let program = super::super::compile("test.slug", source).expect("source compiles");
            program
                .chunk(0)
                .expect("function chunk")
                .code
                .iter()
                .map(|instruction| instruction.opcode)
                .collect::<Vec<_>>()
        };
        let fused = opcodes("val f = fn(x) { x - 1 }\nf(2)");
        assert!(fused.contains(&slug_vm::PackedOpcode::LocalConstantBinary));
        assert!(!fused.contains(&slug_vm::PackedOpcode::GetLocal));

        // The `else` branch's jump lands on `1`, so its `x` must stay a
        // separate instruction.
        let branched = opcodes("val f = fn(c, x) { (if (c) { 0 } else { x }) - 1 }\nf(true, 2)");
        assert!(!branched.contains(&slug_vm::PackedOpcode::LocalConstantBinary));
    }

    #[test]
    fn constant_index_keys_lower_to_one_instruction() {
        let opcodes = |source: &str| {
            let program = super::super::compile("test.slug", source).expect("indexing compiles");
            (0..program.chunk_count())
                .flat_map(|index| {
                    program
                        .chunk(index)
                        .expect("existing chunk")
                        .code
                        .iter()
                        .map(|instruction| instruction.opcode)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };

        let member = opcodes("val record = {\"name\": 1}\nrecord.name\n");
        assert!(member.contains(&slug_vm::PackedOpcode::GetIndexConstant));
        assert!(!member.contains(&slug_vm::PackedOpcode::GetIndex));

        let dynamic = opcodes("val values = [1, 2]\nval at = fn(index) { values[index] }\n");
        assert!(dynamic.contains(&slug_vm::PackedOpcode::GetIndex));
        assert!(!dynamic.contains(&slug_vm::PackedOpcode::GetIndexConstant));
    }

    #[test]
    fn narrower_parameter_types_are_more_specific() {
        let narrow = candidate(vec![Type::Str]);
        let broad = candidate(vec![Type::universal()]);
        assert!(more_specific(&narrow, &broad));
        assert!(!more_specific(&broad, &narrow));
    }

    #[test]
    fn incomparable_parameter_types_do_not_break_ties_by_order() {
        let string = candidate(vec![Type::Str]);
        let number = candidate(vec![Type::Num]);
        assert!(!more_specific(&string, &number));
        assert!(!more_specific(&number, &string));
    }

    #[test]
    fn lower_generic_arity_breaks_equivalent_instantiation_ties() {
        let concrete = candidate(vec![Type::Str]);
        let mut generic = candidate(vec![Type::Str]);
        generic.generic_arity = 1;
        assert!(more_specific(&concrete, &generic));
        assert!(!more_specific(&generic, &concrete));
    }

    #[test]
    fn generic_list_element_inference_preserves_known_result_types() {
        let signature = CallableSignature {
            generic_arity: 1,
            parameters: vec![CallableParameter {
                label: Some("values".into()),
                value_type: Type::List(Some(Box::new(Type::Generic(0)))),
                has_default: false,
                variadic: false,
            }],
            result: Type::union([Type::Generic(0), Type::Nil]),
            inferred_alternatives: Vec::new(),
        };
        let database = Type::Resource(ResourceIdentity::declared("db.slug", "Database"));
        let status = Type::Enum(super::super::semantic::EnumIdentity::declared(
            "status.slug",
            "Status",
        ));
        let user = Type::Struct(Some(super::super::semantic::SchemaIdentity::declared(
            "user.slug",
            "User",
        )));

        for element in [Type::Str, database, status, user] {
            let actual = Type::List(Some(Box::new(element.clone())));
            let mut substitutions = HashMap::new();
            infer(
                &signature.parameters[0].value_type,
                &actual,
                &mut substitutions,
                &SourceSpan::new("test", 1, 1),
            )
            .expect("infer list element type");
            assert_eq!(
                substitute(&signature.result, &substitutions),
                Type::union([element, Type::Nil])
            );
        }
    }

    #[test]
    fn generic_inference_descends_through_structured_container_positions() {
        let span = SourceSpan::new("test", 1, 1);
        let cases = [
            (
                Type::List(Some(Box::new(Type::Generic(0)))),
                Type::List(Some(Box::new(Type::Str))),
            ),
            (
                Type::Map(Some((
                    Box::new(Type::Generic(0)),
                    Box::new(Type::Generic(1)),
                ))),
                Type::Map(Some((Box::new(Type::Str), Box::new(Type::Num)))),
            ),
            (
                Type::Channel(Some(Box::new(Type::Generic(0)))),
                Type::Channel(Some(Box::new(Type::Str))),
            ),
            (
                Type::Task(Some(Box::new(Type::Generic(0)))),
                Type::Task(Some(Box::new(Type::Num))),
            ),
            (
                Type::Function(Some(vec![Type::Generic(0), Type::Generic(1)])),
                Type::Function(Some(vec![Type::Str, Type::Num])),
            ),
            (
                Type::Tuple(vec![Type::Generic(0), Type::Generic(1)]),
                Type::Tuple(vec![Type::Str, Type::Num]),
            ),
            (
                Type::union([Type::Generic(0), Type::Bool]),
                Type::union([Type::Str, Type::Bool]),
            ),
        ];

        for (expected, actual) in cases {
            let mut substitutions = HashMap::new();
            infer(&expected, &actual, &mut substitutions, &span)
                .expect("infer through structured generic position");
            assert_ne!(substitute(&expected, &substitutions), expected);
        }
    }

    #[test]
    fn repeated_generic_parameters_require_one_exact_non_nil_type() {
        let span = SourceSpan::new("test", 1, 1);
        for (expected, actuals) in [
            (
                vec![Type::Generic(0), Type::Generic(0)],
                vec![Type::Str, Type::Str],
            ),
            (
                vec![
                    Type::List(Some(Box::new(Type::Generic(0)))),
                    Type::Generic(0),
                ],
                vec![Type::List(Some(Box::new(Type::Str))), Type::Str],
            ),
        ] {
            let mut substitutions = HashMap::new();
            for (expected, actual) in expected.iter().zip(&actuals) {
                infer(expected, actual, &mut substitutions, &span)
                    .expect("matching occurrences infer the same type");
            }
            assert_eq!(substitutions.get(&0), Some(&Type::Str));
        }

        let mut substitutions = HashMap::new();
        infer(&Type::Generic(0), &Type::Str, &mut substitutions, &span)
            .expect("first occurrence infers string");
        let error = infer(&Type::Generic(0), &Type::Num, &mut substitutions, &span)
            .expect_err("incompatible occurrence is rejected instead of widened");
        assert!(error.to_string().starts_with("expected str, got num"));
    }

    #[test]
    fn nullable_generic_positions_extract_the_non_nil_type() {
        let span = SourceSpan::new("test", 1, 1);
        for (expected, actual) in [
            (
                Type::union([Type::Generic(0), Type::Nil]),
                Type::union([Type::Str, Type::Nil]),
            ),
            (
                Type::List(Some(Box::new(Type::union([Type::Generic(0), Type::Nil])))),
                Type::List(Some(Box::new(Type::union([Type::Str, Type::Nil])))),
            ),
        ] {
            let mut substitutions = HashMap::new();
            infer(&expected, &actual, &mut substitutions, &span)
                .expect("nullable position infers its non-nil component");
            assert_eq!(substitutions.get(&0), Some(&Type::Str));
        }

        let mut substitutions = HashMap::new();
        assert!(
            infer(
                &Type::Generic(0),
                &Type::union([Type::Str, Type::Nil]),
                &mut substitutions,
                &span
            )
            .is_err()
        );
    }

    #[test]
    fn independent_generic_parameters_do_not_contaminate_each_other() {
        let span = SourceSpan::new("test", 1, 1);
        let expected = Type::Map(Some((
            Box::new(Type::Generic(0)),
            Box::new(Type::Generic(1)),
        )));
        let actual = Type::Map(Some((Box::new(Type::Str), Box::new(Type::Num))));
        let mut substitutions = HashMap::new();
        infer(&expected, &actual, &mut substitutions, &span)
            .expect("independent map key and value generics infer");
        assert_eq!(substitutions.get(&0), Some(&Type::Str));
        assert_eq!(substitutions.get(&1), Some(&Type::Num));
    }

    #[test]
    fn explicit_generic_arguments_match_inferred_substitutions() {
        let signature = CallableSignature {
            generic_arity: 1,
            parameters: vec![CallableParameter {
                label: Some("values".into()),
                value_type: Type::List(Some(Box::new(Type::Generic(0)))),
                has_default: false,
                variadic: false,
            }],
            result: Type::union([Type::Generic(0), Type::Nil]),
            inferred_alternatives: Vec::new(),
        };
        let actual = Type::List(Some(Box::new(Type::Str)));
        let mut inferred = HashMap::new();
        infer(
            &signature.parameters[0].value_type,
            &actual,
            &mut inferred,
            &SourceSpan::new("test", 1, 1),
        )
        .expect("infer explicit-equivalent substitution");
        let explicit = HashMap::from([(0, Type::Str)]);
        assert_eq!(
            substitute(&signature.result, &inferred),
            substitute(&signature.result, &explicit)
        );
    }

    #[test]
    fn known_list_spreads_infer_a_sole_variadic_generic_parameter() {
        let parameter = CallableParameter {
            label: Some("values".into()),
            value_type: Type::Generic(0),
            has_default: false,
            variadic: true,
        };
        let actual = Type::List(Some(Box::new(Type::Str)));
        let parameters = [parameter];
        let actuals = [actual];
        let bound = bind_arguments(&parameters, &[ArgumentShape::Spread], &actuals)
            .expect("known spread binds");
        let mut substitutions = HashMap::new();
        infer(
            &bound.values[0].1.value_type,
            bound.values[0].2,
            &mut substitutions,
            &SourceSpan::new("test", 1, 1),
        )
        .expect("spread element infers generic");
        assert_eq!(substitutions.get(&0), Some(&Type::Str));
    }

    #[test]
    fn unprovable_spreads_do_not_bypass_argument_binding() {
        let parameters = [
            CallableParameter {
                label: Some("prefix".into()),
                value_type: Type::Str,
                has_default: false,
                variadic: false,
            },
            CallableParameter {
                label: Some("values".into()),
                value_type: Type::Generic(0),
                has_default: false,
                variadic: true,
            },
        ];
        let actuals = [Type::List(Some(Box::new(Type::Str)))];
        assert!(bind_arguments(&parameters, &[ArgumentShape::Spread], &actuals).is_none());
    }

    #[test]
    fn nullable_generic_lists_preserve_invariant_nominal_elements() {
        let file = Type::Resource(ResourceIdentity::declared("files.slug", "File"));
        let socket = Type::Resource(ResourceIdentity::declared("sockets.slug", "Socket"));
        let expected = Type::List(Some(Box::new(Type::union([Type::Generic(0), Type::Nil]))));
        let file_list = Type::List(Some(Box::new(Type::union([file.clone(), Type::Nil]))));
        let socket_list = Type::List(Some(Box::new(Type::union([socket, Type::Nil]))));
        let span = SourceSpan::new("test", 1, 1);
        let mut substitutions = HashMap::new();
        infer(&expected, &file_list, &mut substitutions, &span)
            .expect("nullable file list infers File");
        assert_eq!(substitutions.get(&0), Some(&file));
        assert!(infer(&expected, &socket_list, &mut substitutions, &span).is_err());
    }

    #[test]
    fn named_generic_arguments_contribute_without_defaulted_parameters() {
        let parameters = [
            CallableParameter {
                label: Some("value".into()),
                value_type: Type::Generic(0),
                has_default: false,
                variadic: false,
            },
            CallableParameter {
                label: Some("fallback".into()),
                value_type: Type::Generic(0),
                has_default: true,
                variadic: false,
            },
        ];
        let actuals = [Type::Str];
        let bound = bind_arguments(&parameters, &[ArgumentShape::Named("value")], &actuals)
            .expect("named required argument binds with omitted default");
        let mut substitutions = HashMap::new();
        infer(
            &bound.values[0].1.value_type,
            bound.values[0].2,
            &mut substitutions,
            &SourceSpan::new("test", 1, 1),
        )
        .expect("named argument infers generic");
        assert_eq!(substitutions.get(&0), Some(&Type::Str));
    }

    #[test]
    fn unresolved_generic_substitutions_remain_unknown_not_any() {
        let mut substitutions = HashMap::new();
        infer(
            &Type::Generic(0),
            &Type::Unknown,
            &mut substitutions,
            &SourceSpan::new("test", 1, 1),
        )
        .expect("dynamic evidence does not resolve a generic");
        assert!(substitutions.is_empty());
        assert_eq!(substitute(&Type::Generic(0), &substitutions), Type::Unknown);
    }

    #[test]
    fn generic_mismatch_diagnostics_include_the_inferred_type() {
        let signature = CallableSignature {
            generic_arity: 1,
            parameters: vec![
                CallableParameter {
                    label: None,
                    value_type: Type::Generic(0),
                    has_default: false,
                    variadic: false,
                },
                CallableParameter {
                    label: None,
                    value_type: Type::Generic(0),
                    has_default: false,
                    variadic: false,
                },
            ],
            result: Type::Generic(0),
            inferred_alternatives: Vec::new(),
        };
        let result = instantiate_candidate(
            &signature,
            &[ArgumentShape::Positional, ArgumentShape::Positional],
            &[Type::Str, Type::Num],
            None,
            &[],
            &SourceSpan::new("test", 1, 1),
            &Environment::new(),
            true,
        );
        let Err(error) = result else {
            panic!("incompatible generic arguments must fail");
        };
        assert!(error.message.contains("generic T0 was inferred as str"));
    }
}
