use std::collections::{HashMap, HashSet};

use crate::{SourceSpan, Value};

use super::{
    SourceError,
    ast::{
        Binary, CallArgument, CasePattern, Expr, ExprKind, ListElement, MapPatternKey, Parameter,
        Pattern, Prefix, SelectCaseKind, Tag, TypeAnnotation,
    },
    environment::{
        CallableParameter, CallableSignature, Environment, ImportSnapshots, ModuleSnapshot,
        SemanticAnalysis, SemanticBinding, function_value_type,
    },
    semantic::{
        ResourceIdentity, SchemaIdentity, Type, resolve_annotation, resolve_resource_references,
        resolve_static_annotation,
    },
};

pub(super) fn analyze_with_imports(
    expressions: &[Expr],
    imports: ImportSnapshots,
) -> Result<SemanticAnalysis, SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    analyze_expressions(expressions, imports)
}

pub(super) fn static_import_names(expressions: &[Expr]) -> Vec<String> {
    let mut names = Vec::new();
    for expression in expressions {
        collect_import_names(expression, &mut names);
    }
    names.sort();
    names.dedup();
    names
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
fn collect_import_names(expression: &Expr, names: &mut Vec<String>) {
    match &expression.kind {
        ExprKind::Call { callee, arguments } => {
            if matches!(&callee.kind, ExprKind::Name(name) if name == "import") {
                for argument in arguments {
                    if let CallArgument::Positional(Expr {
                        kind: ExprKind::Value(Value::Str(name)),
                        ..
                    }) = argument
                    {
                        names.push(name.to_string());
                    }
                }
            }
            collect_import_names(callee, names);
            for argument in arguments {
                let value = match argument {
                    CallArgument::Positional(value)
                    | CallArgument::Named { value, .. }
                    | CallArgument::Spread(value) => value,
                };
                collect_import_names(value, names);
            }
        }
        ExprKind::Declare { value, tags, .. } => {
            for tag in tags {
                for argument in &tag.arguments {
                    collect_import_names(argument, names);
                }
            }
            collect_import_names(value, names);
        }
        ExprKind::Foreign {
            signature, tags, ..
        } => {
            for tag in tags {
                for argument in &tag.arguments {
                    collect_import_names(argument, names);
                }
            }
            for parameter in &signature.parameters {
                if let Some(default) = &parameter.default {
                    collect_import_names(default, names);
                }
            }
        }
        ExprKind::Function {
            parameters, body, ..
        } => {
            for parameter in parameters {
                if let Some(default) = &parameter.default {
                    collect_import_names(default, names);
                }
            }
            collect_import_names(body, names);
        }
        ExprKind::Assign { value, .. }
        | ExprKind::Return { value }
        | ExprKind::Throw { value }
        | ExprKind::Defer { value, .. }
        | ExprKind::Spawn(value)
        | ExprKind::Prefix { value, .. }
        | ExprKind::TypeApply { callee: value, .. } => collect_import_names(value, names),
        ExprKind::Recur(arguments) => {
            for argument in arguments {
                let value = match argument {
                    CallArgument::Positional(value)
                    | CallArgument::Named { value, .. }
                    | CallArgument::Spread(value) => value,
                };
                collect_import_names(value, names);
            }
        }
        ExprKind::Nursery { limit, body } => {
            if let Some(limit) = limit {
                collect_import_names(limit, names);
            }
            collect_import_names(body, names);
        }
        ExprKind::Select(cases) => {
            for case in cases {
                match &case.kind {
                    SelectCaseKind::Receive(value)
                    | SelectCaseKind::After(value)
                    | SelectCaseKind::Await(value) => collect_import_names(value, names),
                    SelectCaseKind::Send { channel, value } => {
                        collect_import_names(channel, names);
                        collect_import_names(value, names);
                    }
                    SelectCaseKind::Default => {}
                }
                if let Some(handler) = &case.handler {
                    collect_import_names(handler, names);
                }
            }
        }
        ExprKind::Match { subject, cases } => {
            if let Some(subject) = subject {
                collect_import_names(subject, names);
            }
            for case in cases {
                if let Some(guard) = &case.guard {
                    collect_import_names(guard, names);
                }
                collect_import_names(&case.value, names);
            }
        }
        ExprKind::Binary { left, right, .. } => {
            collect_import_names(left, names);
            collect_import_names(right, names);
        }
        ExprKind::Block(values) => {
            for value in values {
                collect_import_names(value, names);
            }
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_import_names(condition, names);
            collect_import_names(then_branch, names);
            if let Some(else_branch) = else_branch {
                collect_import_names(else_branch, names);
            }
        }
        ExprKind::List(values) => {
            for value in values {
                let value = match value {
                    ListElement::Value(value) | ListElement::Spread(value) => value,
                };
                collect_import_names(value, names);
            }
        }
        ExprKind::Map(entries) => {
            for (key, value) in entries {
                collect_import_names(key, names);
                collect_import_names(value, names);
            }
        }
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let Some(default) = &field.default {
                    collect_import_names(default, names);
                }
            }
        }
        ExprKind::StructInit { schema, fields } => {
            collect_import_names(schema, names);
            for (_, value) in fields {
                collect_import_names(value, names);
            }
        }
        ExprKind::StructCopy { value, fields } => {
            collect_import_names(value, names);
            for (_, replacement) in fields {
                collect_import_names(replacement, names);
            }
        }
        ExprKind::Index { collection, index } => {
            collect_import_names(collection, names);
            collect_import_names(index, names);
        }
        ExprKind::Slice {
            collection,
            start,
            end,
            step,
        } => {
            collect_import_names(collection, names);
            for bound in [start, end, step].into_iter().flatten() {
                collect_import_names(bound, names);
            }
        }
        ExprKind::Resource { .. }
        | ExprKind::Enum { .. }
        | ExprKind::TypeAlias { .. }
        | ExprKind::Value(_)
        | ExprKind::Interpolate(_)
        | ExprKind::Documentation(_)
        | ExprKind::NotImplemented
        | ExprKind::Name(_) => {}
    }
}

fn analyze_expressions(
    expressions: &[Expr],
    imports: ImportSnapshots,
) -> Result<SemanticAnalysis, SourceError> {
    let mut environment = Environment::with_imports(imports);
    let mut exports = HashMap::new();
    let mut types = HashMap::new();
    let mut resource_names = HashSet::new();
    let mut enum_names = HashSet::new();
    for expression in expressions {
        if let ExprKind::Resource { exported, name, .. } = &expression.kind {
            if !resource_names.insert(name.clone()) {
                return Err(SourceError::semantic(
                    format!("duplicate resource type `{name}`"),
                    expression.span.clone(),
                ));
            }
            let identity = ResourceIdentity::declared(expression.span.path.as_ref(), name.clone());
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
                    format!("duplicate type `{name}`"),
                    expression.span.clone(),
                ));
            }
            let identity = super::semantic::EnumIdentity::declared(
                expression.span.path.as_ref(),
                name.clone(),
            );
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
                format!("enum type `{name}` conflicts with a value declaration"),
                expression.span.clone(),
            ));
        }
    }
    for expression in expressions {
        let _ = check_expression_with_flow(expression, &mut environment, &[])?;
        record_exports(expression, &environment, &mut exports, &mut types);
    }
    Ok(environment.analysis(ModuleSnapshot { exports, types }))
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
                format!("duplicate type `{name}`"),
                expression.span.clone(),
            ));
        }
    }
    for (name, (_, span)) in &aliases {
        if resource_names.contains(name) || enum_names.contains(name) {
            return Err(SourceError::semantic(
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
    aliases: &HashMap<String, (&TypeAnnotation, crate::SourceSpan)>,
    environment: &mut Environment,
    visiting: &mut Vec<String>,
    resolved: &mut HashSet<String>,
) -> Result<(), SourceError> {
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
}

fn alias_dependencies(
    annotation: &TypeAnnotation,
    aliases: &HashMap<String, (&TypeAnnotation, crate::SourceSpan)>,
    dependencies: &mut Vec<String>,
) -> Result<(), SourceError> {
    match annotation {
        TypeAnnotation::Name(name) => {
            if aliases.contains_key(name) {
                dependencies.push(name.clone());
            }
        }
        TypeAnnotation::Apply { name, arguments } => {
            if aliases.contains_key(name) {
                return Err(SourceError::semantic(
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
}

fn function_type(
    type_parameters: &[String],
    parameters: &[Parameter],
    result: Option<&TypeAnnotation>,
    span: &crate::SourceSpan,
    environment: &Environment,
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
    })
}

fn callable_signature(
    expression: &Expr,
    span: &crate::SourceSpan,
    environment: &Environment,
) -> Result<Option<CallableSignature>, SourceError> {
    let ExprKind::Function {
        type_parameters,
        parameters,
        return_annotation,
        ..
    } = &expression.kind
    else {
        return Ok(None);
    };
    function_type(
        type_parameters,
        parameters,
        return_annotation.as_ref(),
        span,
        environment,
    )
    .map(Some)
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
            if let Some(member) = environment.type_member(name) {
                types.insert(name.clone(), member.clone());
            }
        }
        _ => {}
    }
}

fn pattern_binding_names<'a>(pattern: &'a Pattern, names: &mut Vec<&'a String>) {
    match pattern {
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
    }
}

#[allow(clippy::too_many_lines)]
fn check_expression(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    match &expression.kind {
        ExprKind::Declare {
            pattern,
            annotation,
            value,
            ..
        } => {
            let callable = if let Pattern::Binding(name) = pattern {
                callable_signature(value, &expression.span, environment)?
                    .map(|signature| (name.clone(), signature))
            } else {
                None
            };
            if let Some((name, signature)) = &callable {
                environment.declare_callable(name.clone(), signature.clone(), &expression.span)?;
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
                require(expected, &actual, &expression.span)?;
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
            )?;
            environment.record_foreign(
                expression.span.clone(),
                callable.identity(),
                super::environment::ForeignResourceSignature::from_callable(&callable),
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
            let mut signature = function_type(
                function_type_parameters,
                parameters,
                return_annotation.as_ref(),
                &expression.span,
                environment,
            )?;
            environment.record_function(expression.span.clone(), signature.identity());
            let mut scoped = environment.clone();
            scoped.enter_scope();
            for parameter in parameters {
                let parameter_type = parameter
                    .annotation
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
                    .unwrap_or_else(Type::universal);
                if !parameter.discard {
                    scoped.declare(
                        parameter.name.clone(),
                        SemanticBinding::value(parameter_type.clone()),
                    );
                }
                if let Some(default) = &parameter.default {
                    let actual = check_expression(default, &mut scoped, function_type_parameters)?;
                    require(&parameter_type, &actual, &default.span)?;
                }
            }
            let actual =
                check_expression_with_flow(body, &mut scoped, function_type_parameters)?.value_type;
            if let Some(return_annotation) = return_annotation {
                let expected = resolve_static_annotation(
                    return_annotation,
                    function_type_parameters,
                    &body.span,
                    environment,
                )?;
                require(&expected, &actual, &body.span)?;
            }
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
            for value in values {
                match value {
                    ListElement::Value(value) => {
                        elements.push(check_expression(value, environment, type_parameters)?);
                    }
                    ListElement::Spread(value) => {
                        let spread = check_expression(value, environment, type_parameters)?;
                        if let Type::List(Some(element)) = spread {
                            elements.push(*element);
                        } else {
                            lost_precision = true;
                        }
                    }
                }
            }
            Ok(Type::List(
                (!elements.is_empty() && !lost_precision).then(|| Box::new(Type::union(elements))),
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
            let mut scoped = environment.clone();
            scoped.enter_scope();
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
            if let Some(expected) = environment.lookup(name) {
                require(&expected.value_type, &actual, &value.span)?;
            }
            if let Some(binding) = environment.lookup_mut(name) {
                binding.callables.clear();
                if !matches!(actual, Type::Unknown) {
                    binding.value_type = actual.clone();
                }
            }
            Ok(actual)
        }
        ExprKind::Return { value } => check_expression(value, environment, type_parameters),
        ExprKind::Throw { value } => {
            check_expression(value, environment, type_parameters)?;
            Ok(Type::Never)
        }
        ExprKind::Defer { value, .. } => {
            check_expression(value, environment, type_parameters)?;
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
                            let handler = check_expression(handler, environment, type_parameters)?;
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
            for case in cases {
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
                        constraint.as_ref().unwrap_or(&subject_type),
                        &mut scoped,
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
                        apply_flow_facts(&mut scoped, vec![(name.clone(), narrowed.clone())]);
                        case_subject_type = Some(narrowed);
                    }
                }
                environment.record_match_constraints(case.span.clone(), constraints.clone());
                if let Some(remaining_cases) = &mut enum_remaining {
                    if remaining_cases.is_empty() {
                        return Err(SourceError::semantic(
                            "match case is unreachable",
                            case.span.clone(),
                        ));
                    }
                    let mut covered = Vec::new();
                    for (pattern, constraint) in case.patterns.iter().zip(&constraints) {
                        match &pattern.pattern {
                            Pattern::EnumCase {
                                path,
                                case: case_name,
                            } => covered.push(enum_case(path, case_name, environment, &case.span)?),
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
                            "match case cannot match remaining enum cases",
                            case.span.clone(),
                        ));
                    }
                    if case.guard.is_none() {
                        remaining_cases.retain(|candidate| !matching.contains(candidate));
                    }
                }
                if enum_remaining.is_none() && coverage_enabled && remaining.is_none() {
                    return Err(SourceError::semantic(
                        "match case is unreachable",
                        case.span.clone(),
                    ));
                }
                if let Some(current) = &remaining {
                    if constraints.iter().all(Option::is_some)
                        && constraints.iter().all(|constraint| {
                            type_intersection(current, constraint.as_ref().expect("checked above"))
                                .is_none()
                        })
                    {
                        return Err(SourceError::semantic(
                            format!("match case cannot match remaining type {current}"),
                            case.span.clone(),
                        ));
                    }
                    if case.guard.is_none() && !irrefutable_constraints.is_empty() {
                        let coverage = if irrefutable_constraints.iter().any(Option::is_none) {
                            current.clone()
                        } else {
                            Type::union(irrefutable_constraints.into_iter().flatten())
                        };
                        if type_intersection(current, &coverage).is_none() {
                            return Err(SourceError::semantic(
                                "match case is unreachable",
                                case.span.clone(),
                            ));
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
                if checked.continuation == Continuation::FallsThrough
                    && case.guard.is_none()
                    && let Some(value_type) = case_subject_type
                {
                    surviving_subject_types.push(value_type);
                }
                results.push(checked.value_type);
            }
            if coverage_enabled && let Some(remaining) = remaining {
                return Err(SourceError::semantic(
                    format!("non-exhaustive match; missing {remaining}"),
                    expression.span.clone(),
                ));
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
                    format!("non-exhaustive match; missing {missing}"),
                    expression.span.clone(),
                ));
            }
            if let Some(Expr {
                kind: ExprKind::Name(name),
                ..
            }) = subject.as_deref()
                && !surviving_subject_types.is_empty()
            {
                apply_flow_facts(
                    environment,
                    vec![(name.clone(), Type::union(surviving_subject_types))],
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
                        format!("duplicate struct field `{name}`"),
                        value.span.clone(),
                    ));
                }
                let actual = check_expression(value, environment, type_parameters)?;
                if let Some(schema) = &schema_binding {
                    let expected = schema.members.get(name).ok_or_else(|| {
                        SourceError::semantic(
                            format!("struct schema has no field `{name}`"),
                            value.span.clone(),
                        )
                    })?;
                    require(&expected.value_type, &actual, &value.span)?;
                }
            }
            if let Some(schema) = &schema_binding
                && let Some(name) = schema
                    .required_fields
                    .iter()
                    .find(|name| !provided.contains(*name))
            {
                return Err(SourceError::semantic(
                    format!("missing required struct field `{name}`"),
                    expression.span.clone(),
                ));
            }
            Ok(known_schema_identity(schema, environment)
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
                        format!("duplicate struct field `{name}`"),
                        replacement.span.clone(),
                    ));
                }
                let actual = check_expression(replacement, environment, type_parameters)?;
                map_replacements.push(actual.clone());
                if let Some(schema) = &schema {
                    let expected = schema.members.get(name).ok_or_else(|| {
                        SourceError::semantic(
                            format!("struct has no field `{name}`"),
                            replacement.span.clone(),
                        )
                    })?;
                    require(&expected.value_type, &actual, &replacement.span)?;
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
            let collection = check_expression(collection, environment, type_parameters)?;
            let index_type = check_expression(index, environment, type_parameters)?;
            let result = index_result(&collection, &index_type, &expression.span)?;
            if let Some(schema) = known_struct_schema(&collection, environment)
                && let ExprKind::Value(Value::Str(name)) = &index.kind
                && !schema.members.contains_key(name.as_ref())
            {
                return Err(SourceError::semantic(
                    format!("struct has no field `{name}`"),
                    expression.span.clone(),
                ));
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
}

/// Check an expression while retaining its source-level continuation outcome.
///
/// The existing type checker remains the authority for an expression's value
/// type. Flow-sensitive constructs progressively use this result to decide
/// which environments can reach their continuation.
fn check_expression_with_flow(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<CheckedExpression, SourceError> {
    if let ExprKind::Block(values) = &expression.kind {
        let mut scoped = environment.clone();
        scoped.enter_scope();
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
        let left = check_expression_with_flow(then_branch, &mut then_environment, type_parameters)?;
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
                environment.merge_reachable_types(Some(&then_environment), Some(&else_environment));
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
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if matches!(left, Type::Never) || matches!(right, Type::Never) {
        return Ok(Type::Never);
    }
    match operator {
        Binary::Or | Binary::And | Binary::Equal | Binary::NotEqual => Ok(Type::Bool),
        Binary::Greater | Binary::GreaterEqual | Binary::Less | Binary::LessEqual => {
            numeric_operands(left, right, span)?;
            Ok(Type::Bool)
        }
        Binary::BitOr | Binary::BitXor | Binary::BitAnd => bitwise_result(left, right, span),
        Binary::Subtract => subtract_result(left, right, span),
        Binary::ShiftLeft | Binary::ShiftRight | Binary::Divide | Binary::Modulo => {
            numeric_operands(left, right, span)?;
            Ok(Type::Num)
        }
        Binary::Append => list_append_result(left, right, span),
        Binary::Prepend => list_append_result(right, left, span),
        Binary::Add => add_result(left, right, span),
        Binary::Multiply => multiply_result(left, right, span),
        Binary::Pipeline => Ok(right.clone()),
    }
}

fn bitwise_result(
    left: &Type,
    right: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Bytes, Type::Bytes | Type::Num) | (Type::Num, Type::Bytes) => Ok(Type::Bytes),
        _ => invalid_operation("bitwise operator", left, right, span),
    }
}

fn prefix_result(
    operator: Prefix,
    value: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
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

fn bit_not_result(value: &Type, span: &crate::SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(value) {
        return Ok(Type::Unknown);
    }
    match value {
        Type::Num => Ok(Type::Num),
        Type::Bytes => Ok(Type::Bytes),
        _ => Err(SourceError::semantic(
            format!("operator `~` does not accept {value}"),
            span.clone(),
        )),
    }
}

fn numeric_operands(
    left: &Type,
    right: &Type,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    require_operation_operand(&Type::Num, left, span)?;
    require_operation_operand(&Type::Num, right, span)
}

fn add_result(left: &Type, right: &Type, span: &crate::SourceSpan) -> Result<Type, SourceError> {
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

fn subtract_result(
    left: &Type,
    right: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if matches!(left, Type::Map(_)) {
        if is_dynamic_operation_type(right) || is_map_key_type(right) {
            return Ok(left.clone());
        }
        return invalid_operation("-", left, right, span);
    }
    numeric_operands(left, right, span)?;
    Ok(Type::Num)
}

fn is_map_key_type(value: &Type) -> bool {
    match value {
        Type::Bool | Type::Num | Type::Str | Type::Bytes => true,
        Type::Union(members) => members.iter().all(is_map_key_type),
        _ => false,
    }
}

fn multiply_result(
    left: &Type,
    right: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Str, Type::Num) => Ok(Type::Str),
        _ => invalid_operation("*", left, right, span),
    }
}

fn list_append_result(
    list: &Type,
    value: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
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

fn index_result(
    collection: &Type,
    index: &Type,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
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
        other => invalid_operation("[]", other, index, span),
    }
}

fn slice_result(collection: &Type, span: &crate::SourceSpan) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(collection) {
        return Ok(Type::Unknown);
    }
    match collection {
        Type::List(element) => Ok(Type::List(element.clone())),
        Type::Bytes => Ok(Type::Bytes),
        Type::Str => Ok(Type::Str),
        other => Err(SourceError::semantic(
            format!("expected list, got {other}"),
            span.clone(),
        )),
    }
}

fn require_operation_operand(
    expected: &Type,
    actual: &Type,
    span: &crate::SourceSpan,
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
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    Err(SourceError::semantic(
        format!("operator `{operator}` does not accept {left} and {right}"),
        span.clone(),
    ))
}

fn is_dynamic_operation_type(value_type: &Type) -> bool {
    match value_type {
        Type::Unknown | Type::Any => true,
        Type::Union(members) => members.iter().any(is_dynamic_operation_type),
        _ => false,
    }
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
            format!("unknown enum `{path}`"),
            span.clone(),
        ));
    };
    if !cases.iter().any(|candidate| candidate == case) {
        return Err(SourceError::semantic(
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
    match pattern {
        Pattern::Wildcard | Pattern::Binding(_) => true,
        Pattern::At { pattern, .. } => is_irrefutable_pattern(pattern),
        Pattern::List { .. }
        | Pattern::Map { .. }
        | Pattern::Literal(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll
        | Pattern::EnumCase { .. } => false,
    }
}

fn type_intersection(left: &Type, right: &Type) -> Option<Type> {
    if let Type::Union(members) = left {
        return union_intersections(members, right);
    }
    if let Type::Union(members) = right {
        return union_intersections(members, left);
    }
    if left == right {
        return Some(left.clone());
    }
    match (left, right) {
        (Type::Struct(None), Type::Struct(Some(identity)))
        | (Type::Struct(Some(identity)), Type::Struct(None)) => {
            Some(Type::Struct(Some(identity.clone())))
        }
        _ => None,
    }
}

fn union_intersections(members: &[Type], other: &Type) -> Option<Type> {
    let intersections = members
        .iter()
        .filter_map(|member| type_intersection(member, other))
        .collect::<Vec<_>>();
    (!intersections.is_empty()).then(|| Type::union(intersections))
}

fn type_subtract(left: &Type, right: &Type) -> Option<Type> {
    if let Type::Union(members) = right {
        return members.iter().try_fold(left.clone(), |remaining, member| {
            type_subtract(&remaining, member)
        });
    }
    if let Type::Union(members) = left {
        let remaining = members
            .iter()
            .filter(|member| type_intersection(member, right).is_none())
            .cloned()
            .collect::<Vec<_>>();
        return (!remaining.is_empty()).then(|| Type::union(remaining));
    }
    if type_intersection(left, right).is_some() {
        None
    } else {
        Some(left.clone())
    }
}

/// Narrowed bindings known on one control-flow path. Facts retain their actual
/// `Type` values, including nominal identities, so match constraints can share
/// this representation with direct conditions.
type FlowFacts = Vec<(String, Type)>;

fn condition_facts(expression: &Expr, environment: &Environment) -> (FlowFacts, FlowFacts) {
    let ExprKind::Binary {
        left,
        operator,
        right,
    } = &expression.kind
    else {
        return (Vec::new(), Vec::new());
    };
    let ((ExprKind::Name(name), ExprKind::Value(Value::Nil))
    | (ExprKind::Value(Value::Nil), ExprKind::Name(name))) = (&left.kind, &right.kind)
    else {
        return (Vec::new(), Vec::new());
    };
    if !matches!(operator, Binary::Equal | Binary::NotEqual) {
        return (Vec::new(), Vec::new());
    }
    let Some(binding) = environment.lookup(name) else {
        return (Vec::new(), Vec::new());
    };
    let non_nil = binding.value_type.without_nil();
    let nil = Type::Nil;
    if matches!(operator, Binary::NotEqual) {
        (vec![(name.clone(), non_nil)], vec![(name.clone(), nil)])
    } else {
        (vec![(name.clone(), nil)], vec![(name.clone(), non_nil)])
    }
}

fn apply_flow_facts(environment: &mut Environment, facts: FlowFacts) {
    for (name, value_type) in facts {
        if let Some(binding) = environment.lookup_mut(&name) {
            binding.value_type = value_type;
        }
    }
}

fn expression_binding(expression: &Expr, environment: &Environment) -> Option<SemanticBinding> {
    match &expression.kind {
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
    }
}

fn value_binding(
    expression: &Expr,
    value_type: &Type,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<SemanticBinding, SourceError> {
    if let ExprKind::StructSchema(fields) = &expression.kind {
        return schema_binding(fields, environment, type_parameters, &expression.span);
    }
    if let Some(binding) = expression_binding(expression, environment) {
        return Ok(binding);
    }
    if let ExprKind::Map(entries) = &expression.kind {
        let mut binding = SemanticBinding::value(value_type.clone().widen_unknown());
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
        let mut binding = SemanticBinding::value(value_type.clone().widen_unknown());
        binding.members = schema.members;
        return Ok(binding);
    }
    Ok(SemanticBinding::value(value_type.clone().widen_unknown()))
}

fn static_map_member_binding(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<SemanticBinding, SourceError> {
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
            check_expression(default, environment, type_parameters)?.widen_unknown()
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
    }
    Some(SemanticBinding::module(result, type_members))
}

fn bind_semantic_pattern(
    pattern: &Pattern,
    binding: &SemanticBinding,
    environment: &mut Environment,
) {
    match pattern {
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
    }
}

fn check_pattern(
    pattern: &Pattern,
    environment: &mut Environment,
    type_parameters: &[String],
) -> Result<(), SourceError> {
    match pattern {
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
    }
}

fn check_case_pattern(
    pattern: &CasePattern,
    environment: &mut Environment,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<Option<Type>, SourceError> {
    let constraint = if let Some(constraint) = &pattern.constraint {
        let constraint = resolve_static_annotation(constraint, type_parameters, span, environment)?;
        if !constraint.is_reifiable_match_constraint() {
            return Err(SourceError::semantic(
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
}

fn bind_case_pattern(pattern: &Pattern, value_type: &Type, environment: &mut Environment) {
    match pattern {
        Pattern::Binding(name) => {
            environment.declare(name.clone(), SemanticBinding::value(value_type.clone()));
        }
        Pattern::At { name, pattern } => {
            environment.declare(name.clone(), SemanticBinding::value(value_type.clone()));
            bind_case_pattern(pattern, value_type, environment);
        }
        Pattern::List { items, rest } => {
            let element = match value_type {
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
                let value = if let (Some(schema), MapPatternKey::String(name)) = (&schema, map_key)
                {
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
}

fn check_call(
    callee: &Expr,
    arguments: &[CallArgument],
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
    piped: Option<Type>,
) -> Result<Type, SourceError> {
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
        return check_function_value_call(&binding.value_type, &shapes, &actuals, &expression.span);
    }
    let callables = binding.callables.clone();
    if callables.len() > 1
        && shapes
            .iter()
            .any(|shape| matches!(shape, ArgumentShape::Spread))
    {
        return Err(SourceError::semantic(
            format!("cannot resolve overload `{name}` with spread arguments"),
            expression.span.clone(),
        ));
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
        return Err(SourceError::semantic(
            format!("no matching overload for `{name}`"),
            expression.span.clone(),
        ));
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
            format!("ambiguous overload for `{name}`"),
            expression.span.clone(),
        ));
    }
    if callables.len() > 1 {
        environment
            .record_selected_call(expression.span.clone(), most_specific[0].identity.clone());
    }
    Ok(most_specific[0].result.clone())
}

fn check_function_value_call(
    value_type: &Type,
    shapes: &[ArgumentShape<'_>],
    actuals: &[Type],
    span: &crate::SourceSpan,
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
    let value = match argument {
        CallArgument::Positional(value)
        | CallArgument::Named { value, .. }
        | CallArgument::Spread(value) => value,
    };
    check_expression(value, environment, type_parameters)
}

struct InstantiatedCandidate {
    bound_types: Vec<Type>,
    generic_arity: usize,
    non_variadic: bool,
    uses_empty_variadic: bool,
    result: Type,
    identity: super::environment::CallableIdentity,
}

#[derive(Clone, Copy)]
enum ArgumentShape<'a> {
    Positional,
    Named(&'a str),
    Spread,
}

#[allow(clippy::too_many_arguments)]
fn instantiate_candidate(
    signature: &CallableSignature,
    arguments: &[ArgumentShape<'_>],
    actuals: &[Type],
    explicit: Option<&Vec<TypeAnnotation>>,
    type_parameters: &[String],
    span: &crate::SourceSpan,
    environment: &Environment,
    report_mismatch: bool,
) -> Result<Option<InstantiatedCandidate>, SourceError> {
    let mut substitutions = HashMap::new();
    if let Some(explicit) = explicit {
        if explicit.len() != signature.generic_arity {
            if report_mismatch {
                return Err(SourceError::semantic(
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
    for (parameter, actual) in &bound.values {
        let actual = (*actual).clone();
        if let Err(error) = infer(&parameter.value_type, &actual, &mut substitutions, span) {
            if report_mismatch {
                return Err(error);
            }
            return Ok(None);
        }
    }
    let bound_types = bound
        .values
        .iter()
        .map(|(parameter, _)| substitute(&parameter.value_type, &substitutions).widen_unknown())
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

struct BoundArguments<'a> {
    values: Vec<(&'a CallableParameter, &'a Type)>,
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
        return Some(BoundArguments {
            values: Vec::new(),
            uses_empty_variadic: parameters
                .last()
                .is_some_and(|parameter| parameter.variadic),
        });
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
        let parameter = match argument {
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
                &parameters[index]
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
                &parameters[index]
            }
            ArgumentShape::Spread => unreachable!("spread calls were handled above"),
        };
        bound.push((parameter, actual));
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
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    if let Type::Generic(index) = expected {
        if matches!(actual, Type::Unknown) {
            return Ok(());
        }
        if actual.includes_nil() {
            return Err(SourceError::semantic(
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
    span: &crate::SourceSpan,
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
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    infer_payload(expected, actual, substitutions, span, false)
}

fn infer_channel_payload(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    infer_payload(expected, actual, substitutions, span, true)
}

fn infer_payload(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<usize, Type>,
    span: &crate::SourceSpan,
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
    match value {
        Value::Nil => Type::Nil,
        Value::Bool(_) => Type::Bool,
        Value::Int(_) | Value::Float(_) => Type::Num,
        Value::Str(_) => Type::Str,
        Value::Bytes(_) => Type::Bytes,
        Value::List(_) => Type::List(None),
        Value::Map(_) => Type::Map(None),
        Value::StructSchema(_) => Type::Schema,
        Value::Struct(_) => Type::Struct(None),
        Value::Enum(value) => Type::Enum(super::semantic::EnumIdentity::declared(
            value.module.as_ref(),
            value.name.as_ref(),
        )),
        Value::Channel(_) => Type::Channel(None),
        Value::Closure(_)
        | Value::Native(_)
        | Value::DeclaredNative { .. }
        | Value::Builtin(_)
        | Value::Overloads(_) => Type::Function(None),
        Value::Task(_) => Type::Task(None),
        Value::NativeResource(_) | Value::Uninitialized | Value::Binding { .. } => Type::Unknown,
    }
}

fn require(expected: &Type, actual: &Type, span: &crate::SourceSpan) -> Result<(), SourceError> {
    if actual.is_assignable_to(expected) {
        Ok(())
    } else {
        let expected_display = expected.to_string();
        let actual_display = actual.to_string();
        let (expected_display, actual_display) = if expected_display == actual_display {
            (expected.diagnostic_display(), actual.diagnostic_display())
        } else {
            (expected_display, actual_display)
        };
        Err(SourceError::semantic(
            format!("expected {expected_display}, got {actual_display}"),
            span.clone(),
        ))
    }
}

#[allow(clippy::too_many_lines)]
fn validate_expression(expression: &Expr, type_parameters: &[String]) -> Result<(), SourceError> {
    match &expression.kind {
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
    }
}

fn validate_parameter(
    parameter: &Parameter,
    type_parameters: &[String],
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    if let Some(annotation) = &parameter.annotation {
        resolve_annotation(annotation, type_parameters, span)?;
    }
    validate_tags(&parameter.tags, type_parameters)?;
    if let Some(default) = &parameter.default {
        validate_expression(default, type_parameters)?;
    }
    Ok(())
}

fn validate_tags(tags: &[Tag], type_parameters: &[String]) -> Result<(), SourceError> {
    for tag in tags {
        for argument in &tag.arguments {
            validate_expression(argument, type_parameters)?;
        }
    }
    Ok(())
}

fn validate_arguments(
    arguments: &[CallArgument],
    type_parameters: &[String],
) -> Result<(), SourceError> {
    for argument in arguments {
        let value = match argument {
            CallArgument::Positional(value)
            | CallArgument::Named { value, .. }
            | CallArgument::Spread(value) => value,
        };
        validate_expression(value, type_parameters)?;
    }
    Ok(())
}

fn validate_case_pattern(
    pattern: &CasePattern,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<(), SourceError> {
    if let Some(constraint) = &pattern.constraint {
        let constraint = resolve_annotation(constraint, type_parameters, span)?;
        if !constraint.is_reifiable_match_constraint() {
            return Err(SourceError::semantic(
                "match type constraint is not runtime-checkable",
                span.clone(),
            ));
        }
    }
    validate_pattern(&pattern.pattern, type_parameters)
}

fn validate_pattern(pattern: &Pattern, type_parameters: &[String]) -> Result<(), SourceError> {
    match pattern {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            }
            .identity(),
        }
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
}
