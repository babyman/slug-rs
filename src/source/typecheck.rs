use std::collections::HashMap;

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
    semantic::{SchemaIdentity, Type, resolve_annotation, resolve_static_annotation},
};

pub(super) fn validate(expressions: &[Expr]) -> Result<SemanticAnalysis, SourceError> {
    validate_with_imports(expressions, ImportSnapshots::new())
}

pub(super) fn validate_with_imports(
    expressions: &[Expr],
    imports: ImportSnapshots,
) -> Result<SemanticAnalysis, SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    analyze(expressions, false, imports)
}

pub(super) fn check_with_imports(
    expressions: &[Expr],
    imports: ImportSnapshots,
) -> Result<SemanticAnalysis, SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    analyze(expressions, true, imports)
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
        ExprKind::Value(_)
        | ExprKind::Interpolate(_)
        | ExprKind::Documentation(_)
        | ExprKind::NotImplemented
        | ExprKind::Name(_) => {}
    }
}

fn analyze(
    expressions: &[Expr],
    strict: bool,
    imports: ImportSnapshots,
) -> Result<SemanticAnalysis, SourceError> {
    let mut environment = Environment::with_imports(imports);
    let mut exports = HashMap::new();
    for expression in expressions {
        check_expression(expression, &mut environment, &[], strict)?;
        record_exports(expression, &environment, &mut exports);
    }
    Ok(environment.analysis(ModuleSnapshot { exports }))
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
        Pattern::Literal(_) | Pattern::Wildcard | Pattern::Pinned(_) | Pattern::MapAll => {}
    }
}

#[allow(clippy::too_many_lines)]
fn check_expression(
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
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
            let actual = check_expression(value, environment, type_parameters, strict)?;
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
            if strict && let Some(expected) = &declared {
                require(expected, &actual, &expression.span)?;
            }
            if callable.is_none() {
                let mut binding =
                    value_binding(value, &actual, environment, type_parameters, strict)?;
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
            environment.record_foreign(expression.span.clone(), callable.identity());
            let value_type = function_value_type(&callable);
            environment.declare_callable(name.clone(), callable, &expression.span)?;
            for parameter in &signature.parameters {
                if let Some(default) = &parameter.default {
                    let actual =
                        check_expression(default, environment, &signature.type_parameters, strict)?;
                    if strict && let Some(annotation) = &parameter.annotation {
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
                    let actual =
                        check_expression(default, &mut scoped, function_type_parameters, strict)?;
                    if strict {
                        require(&parameter_type, &actual, &default.span)?;
                    }
                }
            }
            let actual = check_expression(body, &mut scoped, function_type_parameters, strict)?;
            if strict && let Some(return_annotation) = return_annotation {
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
            strict,
            None,
        ),
        ExprKind::TypeApply { callee, .. } => {
            check_expression(callee, environment, type_parameters, strict)
        }
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let Some(default) = &field.default {
                    let actual = check_expression(default, environment, type_parameters, strict)?;
                    if strict && let Some(annotation) = &field.annotation {
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
                    ListElement::Value(value) => elements.push(check_expression(
                        value,
                        environment,
                        type_parameters,
                        strict,
                    )?),
                    ListElement::Spread(value) => {
                        let spread = check_expression(value, environment, type_parameters, strict)?;
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
                keys.push(check_expression(key, environment, type_parameters, strict)?);
                values.push(check_expression(
                    value,
                    environment,
                    type_parameters,
                    strict,
                )?);
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
                result = check_expression(value, &mut scoped, type_parameters, strict)?;
            }
            if strict {
                environment.merge_compatible_types(&scoped, &scoped);
            }
            Ok(result)
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            check_expression(condition, environment, type_parameters, strict)?;
            let (then_facts, else_facts) = if strict {
                nil_condition_facts(condition, environment)
            } else {
                Default::default()
            };
            let mut then_environment = environment.clone();
            apply_type_facts(&mut then_environment, then_facts);
            let left =
                check_expression(then_branch, &mut then_environment, type_parameters, strict)?;
            let mut else_environment = environment.clone();
            apply_type_facts(&mut else_environment, else_facts);
            let right = else_branch
                .as_ref()
                .map(|branch| {
                    check_expression(branch, &mut else_environment, type_parameters, strict)
                })
                .transpose()?
                .unwrap_or(Type::Nil);
            if strict {
                environment.merge_compatible_types(&then_environment, &else_environment);
            }
            Ok(Type::union([left, right]))
        }
        ExprKind::Binary {
            left,
            operator,
            right,
        } => {
            let left_type = check_expression(left, environment, type_parameters, strict)?;
            if matches!(operator, Binary::Pipeline) {
                return match &right.kind {
                    ExprKind::Call { callee, arguments } => check_call(
                        callee,
                        arguments,
                        right,
                        environment,
                        type_parameters,
                        strict,
                        Some(left_type),
                    ),
                    ExprKind::Name(_) | ExprKind::TypeApply { .. } | ExprKind::Index { .. } => {
                        check_call(
                            right,
                            &[],
                            right,
                            environment,
                            type_parameters,
                            strict,
                            Some(left_type),
                        )
                    }
                    _ => check_expression(right, environment, type_parameters, strict),
                };
            }
            if matches!(operator, Binary::And | Binary::Or) {
                let mut right_environment = environment.clone();
                let (then_facts, else_facts) = if strict {
                    nil_condition_facts(left, environment)
                } else {
                    Default::default()
                };
                apply_type_facts(
                    &mut right_environment,
                    if matches!(operator, Binary::And) {
                        then_facts
                    } else {
                        else_facts
                    },
                );
                let right =
                    check_expression(right, &mut right_environment, type_parameters, strict)?;
                return Ok(Type::union([left_type, right]));
            }
            let right = check_expression(right, environment, type_parameters, strict)?;
            binary_result(*operator, &left_type, &right, strict, &expression.span)
        }
        ExprKind::Prefix { operators, value } => {
            let mut result = check_expression(value, environment, type_parameters, strict)?;
            for (operator, span) in operators.iter().rev() {
                result = prefix_result(*operator, &result, strict, span)?;
            }
            Ok(result)
        }
        ExprKind::Name(name) => Ok(environment
            .lookup(name)
            .map_or(Type::Unknown, |binding| binding.value_type.clone())),
        ExprKind::Assign { name, value } => {
            let actual = check_expression(value, environment, type_parameters, strict)?;
            if strict && let Some(expected) = environment.lookup(name) {
                require(&expected.value_type, &actual, &value.span)?;
            }
            if let Some(binding) = environment.lookup_mut(name) {
                binding.callables.clear();
                if strict && !matches!(actual, Type::Unknown) {
                    binding.value_type = actual.clone();
                }
            }
            Ok(actual)
        }
        ExprKind::Return { value } => check_expression(value, environment, type_parameters, strict),
        ExprKind::Throw { value } => {
            check_expression(value, environment, type_parameters, strict)?;
            Ok(Type::Unknown)
        }
        ExprKind::Defer { value, .. } => {
            check_expression(value, environment, type_parameters, strict)?;
            Ok(Type::Nil)
        }
        ExprKind::Spawn(value) => {
            let result = check_expression(value, environment, type_parameters, strict)?;
            Ok(Type::Task(Some(Box::new(result.widen_unknown()))))
        }
        ExprKind::Nursery { limit, body } => {
            if let Some(limit) = limit {
                check_expression(limit, environment, type_parameters, strict)?;
            }
            check_expression(body, environment, type_parameters, strict)
        }
        ExprKind::Recur(arguments) => {
            for argument in arguments {
                check_argument(argument, environment, type_parameters, strict)?;
            }
            Ok(Type::Unknown)
        }
        ExprKind::Select(cases) => {
            let mut results = Vec::new();
            for case in cases {
                match &case.kind {
                    SelectCaseKind::Receive(value)
                    | SelectCaseKind::After(value)
                    | SelectCaseKind::Await(value) => {
                        check_expression(value, environment, type_parameters, strict)?;
                    }
                    SelectCaseKind::Send { channel, value } => {
                        check_expression(channel, environment, type_parameters, strict)?;
                        check_expression(value, environment, type_parameters, strict)?;
                    }
                    SelectCaseKind::Default => {}
                }
                results.push(
                    case.handler
                        .as_ref()
                        .map(|handler| {
                            check_expression(handler, environment, type_parameters, strict)
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
                .map(|subject| check_expression(subject, environment, type_parameters, strict))
                .transpose()?
                .unwrap_or_else(Type::universal);
            let coverage_enabled = strict && is_closed_coverage_type(&subject_type);
            let mut remaining = coverage_enabled.then_some(subject_type.clone());
            let mut results = Vec::new();
            for case in cases {
                let mut scoped = environment.clone();
                scoped.enter_scope();
                let mut constraints = Vec::new();
                let mut irrefutable_constraints = Vec::new();
                for pattern in &case.patterns {
                    let constraint = check_case_pattern(
                        pattern,
                        &mut scoped,
                        type_parameters,
                        strict,
                        &case.span,
                    )?;
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
                if coverage_enabled && remaining.is_none() {
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
                    check_expression(guard, &mut scoped, type_parameters, strict)?;
                    if strict {
                        let (facts, _) = nil_condition_facts(guard, &scoped);
                        apply_type_facts(&mut scoped, facts);
                    }
                }
                results.push(check_expression(
                    &case.value,
                    &mut scoped,
                    type_parameters,
                    strict,
                )?);
            }
            if coverage_enabled && let Some(remaining) = remaining {
                return Err(SourceError::semantic(
                    format!("non-exhaustive match; missing {remaining}"),
                    expression.span.clone(),
                ));
            }
            Ok(Type::union(results))
        }
        ExprKind::StructInit { schema, fields } => {
            let schema_type = check_expression(schema, environment, type_parameters, strict)?;
            require_operation_operand(&Type::Schema, &schema_type, strict, &expression.span)?;
            let schema_binding = expression_binding(schema, environment)
                .filter(|binding| binding.value_type == Type::Schema);
            let mut provided = std::collections::HashSet::new();
            for (name, value) in fields {
                if strict && !provided.insert(name) {
                    return Err(SourceError::semantic(
                        format!("duplicate struct field `{name}`"),
                        value.span.clone(),
                    ));
                }
                let actual = check_expression(value, environment, type_parameters, strict)?;
                if strict && let Some(schema) = &schema_binding {
                    let expected = schema.members.get(name).ok_or_else(|| {
                        SourceError::semantic(
                            format!("struct schema has no field `{name}`"),
                            value.span.clone(),
                        )
                    })?;
                    require(&expected.value_type, &actual, &value.span)?;
                }
            }
            if strict
                && let Some(schema) = &schema_binding
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
            let result = check_expression(value, environment, type_parameters, strict)?;
            let schema = known_struct_schema(&result, environment);
            let mut replaced = std::collections::HashSet::new();
            let mut map_replacements = Vec::new();
            for (name, replacement) in fields {
                if strict && !replaced.insert(name) {
                    return Err(SourceError::semantic(
                        format!("duplicate struct field `{name}`"),
                        replacement.span.clone(),
                    ));
                }
                let actual = check_expression(replacement, environment, type_parameters, strict)?;
                map_replacements.push(actual.clone());
                if strict && let Some(schema) = &schema {
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
            let collection = check_expression(collection, environment, type_parameters, strict)?;
            let index_type = check_expression(index, environment, type_parameters, strict)?;
            let result = index_result(&collection, &index_type, strict, &expression.span)?;
            if strict
                && let Some(schema) = known_struct_schema(&collection, environment)
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
            let collection = check_expression(collection, environment, type_parameters, strict)?;
            for bound in [start, end, step].into_iter().flatten() {
                let bound = check_expression(bound, environment, type_parameters, strict)?;
                require_operation_operand(&Type::Num, &bound, strict, &expression.span)?;
            }
            slice_result(&collection, strict, &expression.span)
        }
        ExprKind::Interpolate(_) => Ok(Type::Str),
        ExprKind::Documentation(_) => Ok(Type::Nil),
        ExprKind::NotImplemented => Ok(Type::Unknown),
    }
}

fn binary_result(
    operator: Binary,
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    match operator {
        Binary::Or | Binary::And | Binary::Equal | Binary::NotEqual => Ok(Type::Bool),
        Binary::Greater | Binary::GreaterEqual | Binary::Less | Binary::LessEqual => {
            numeric_operands(left, right, strict, span)?;
            Ok(Type::Bool)
        }
        Binary::BitOr | Binary::BitXor | Binary::BitAnd => {
            bitwise_result(left, right, strict, span)
        }
        Binary::Subtract => subtract_result(left, right, strict, span),
        Binary::ShiftLeft | Binary::ShiftRight | Binary::Divide | Binary::Modulo => {
            numeric_operands(left, right, strict, span)?;
            Ok(Type::Num)
        }
        Binary::Append => list_append_result(left, right, strict, span),
        Binary::Prepend => list_append_result(right, left, strict, span),
        Binary::Add => add_result(left, right, strict, span),
        Binary::Multiply => multiply_result(left, right, strict, span),
        Binary::Pipeline => Ok(right.clone()),
    }
}

fn bitwise_result(
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Bytes, Type::Bytes | Type::Num) | (Type::Num, Type::Bytes) => Ok(Type::Bytes),
        _ => invalid_operation("bitwise operator", left, right, strict, span),
    }
}

fn prefix_result(
    operator: Prefix,
    value: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    match operator {
        Prefix::Not => Ok(Type::Bool),
        Prefix::Negate => {
            require_operation_operand(&Type::Num, value, strict, span)?;
            Ok(Type::Num)
        }
        Prefix::BitNot => bit_not_result(value, strict, span),
    }
}

fn bit_not_result(
    value: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(value) {
        return Ok(Type::Unknown);
    }
    match value {
        Type::Num => Ok(Type::Num),
        Type::Bytes => Ok(Type::Bytes),
        _ if strict => Err(SourceError::semantic(
            format!("operator `~` does not accept {value}"),
            span.clone(),
        )),
        _ => Ok(Type::Unknown),
    }
}

fn numeric_operands(
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    require_operation_operand(&Type::Num, left, strict, span)?;
    require_operation_operand(&Type::Num, right, strict, span)
}

fn add_result(
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
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
        _ => invalid_operation("+", left, right, strict, span),
    }
}

fn subtract_result(
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if matches!(left, Type::Map(_)) {
        if !strict || is_dynamic_operation_type(right) || is_map_key_type(right) {
            return Ok(left.clone());
        }
        return invalid_operation("-", left, right, strict, span);
    }
    numeric_operands(left, right, strict, span)?;
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
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(left) || is_dynamic_operation_type(right) {
        return Ok(Type::Unknown);
    }
    match (left, right) {
        (Type::Num, Type::Num) => Ok(Type::Num),
        (Type::Str, Type::Num) => Ok(Type::Str),
        _ => invalid_operation("*", left, right, strict, span),
    }
}

fn list_append_result(
    list: &Type,
    value: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if matches!(list, Type::Bytes) {
        if !strict || is_dynamic_operation_type(value) || matches!(value, Type::Num | Type::Bytes) {
            return Ok(Type::Bytes);
        }
        return invalid_operation(":+", list, value, strict, span);
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
        other => invalid_operation(":+", other, value, strict, span),
    }
}

fn index_result(
    collection: &Type,
    index: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(collection) || is_dynamic_operation_type(index) {
        return Ok(Type::Unknown);
    }
    match collection {
        Type::List(element) => {
            require_operation_operand(&Type::Num, index, strict, span)?;
            Ok(element.as_deref().cloned().unwrap_or(Type::Unknown))
        }
        Type::Bytes => {
            require_operation_operand(&Type::Num, index, strict, span)?;
            Ok(Type::Num)
        }
        Type::Str => {
            require_operation_operand(&Type::Num, index, strict, span)?;
            Ok(Type::Str)
        }
        Type::Map(entries) => match entries {
            Some((key, value)) => {
                require_operation_operand(key, index, strict, span)?;
                Ok(Type::union([value.as_ref().clone(), Type::Nil]))
            }
            None => Ok(Type::Unknown),
        },
        Type::Struct(_) => {
            require_operation_operand(&Type::Str, index, strict, span)?;
            Ok(Type::Unknown)
        }
        other => invalid_operation("[]", other, index, strict, span),
    }
}

fn slice_result(
    collection: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if is_dynamic_operation_type(collection) {
        return Ok(Type::Unknown);
    }
    match collection {
        Type::List(element) => Ok(Type::List(element.clone())),
        Type::Bytes => Ok(Type::Bytes),
        Type::Str => Ok(Type::Str),
        other => {
            if strict {
                return Err(SourceError::semantic(
                    format!("expected list, got {other}"),
                    span.clone(),
                ));
            }
            Ok(Type::Unknown)
        }
    }
}

fn require_operation_operand(
    expected: &Type,
    actual: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    if !strict || is_dynamic_operation_type(actual) || actual.is_assignable_to(expected) {
        return Ok(());
    }
    require(expected, actual, span)
}

fn invalid_operation(
    operator: &str,
    left: &Type,
    right: &Type,
    strict: bool,
    span: &crate::SourceSpan,
) -> Result<Type, SourceError> {
    if strict {
        Err(SourceError::semantic(
            format!("operator `{operator}` does not accept {left} and {right}"),
            span.clone(),
        ))
    } else {
        Ok(Type::Unknown)
    }
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
        | Type::Resource
        | Type::Schema
        | Type::Struct(Some(_))
        | Type::Function(None)
        | Type::Task(None)
        | Type::Channel(None) => true,
        Type::Union(members) => members.iter().all(is_closed_coverage_type),
        Type::Unknown
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

fn is_irrefutable_pattern(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Wildcard | Pattern::Binding(_) => true,
        Pattern::At { pattern, .. } => is_irrefutable_pattern(pattern),
        Pattern::List { .. }
        | Pattern::Map { .. }
        | Pattern::Literal(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll => false,
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

type TypeFacts = Vec<(String, Type)>;

fn nil_condition_facts(expression: &Expr, environment: &Environment) -> (TypeFacts, TypeFacts) {
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

fn apply_type_facts(environment: &mut Environment, facts: TypeFacts) {
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
            imported_modules(arguments, environment).map(SemanticBinding::module)
        }
        _ => None,
    }
}

fn value_binding(
    expression: &Expr,
    value_type: &Type,
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
) -> Result<SemanticBinding, SourceError> {
    if let ExprKind::StructSchema(fields) = &expression.kind {
        return schema_binding(
            fields,
            environment,
            type_parameters,
            strict,
            &expression.span,
        );
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
                    static_map_member_binding(value, environment, type_parameters, strict)?,
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
    strict: bool,
) -> Result<SemanticBinding, SourceError> {
    if let Some(binding) = expression_binding(expression, environment) {
        return Ok(binding);
    }
    match &expression.kind {
        ExprKind::Value(value) => Ok(SemanticBinding::value(value_type(value))),
        ExprKind::Map(_) => value_binding(
            expression,
            &Type::Map(None),
            environment,
            type_parameters,
            strict,
        ),
        _ => Ok(SemanticBinding::value(Type::Unknown)),
    }
}

fn schema_binding(
    fields: &[super::ast::StructSchemaField],
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
    span: &SourceSpan,
) -> Result<SemanticBinding, SourceError> {
    let mut binding = SemanticBinding::value(Type::Schema);
    binding.schema_identity = Some(SchemaIdentity {
        id: format!("{}:{}:{}", span.path, span.line, span.column),
        name: "<schema>".into(),
    });
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
            check_expression(default, environment, type_parameters, strict)?.widen_unknown()
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
    environment.schema_by_identity(&identity.id)
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
) -> Option<HashMap<String, SemanticBinding>> {
    let mut result: HashMap<String, SemanticBinding> = HashMap::new();
    for argument in arguments {
        let CallArgument::Positional(Expr {
            kind: ExprKind::Value(Value::Str(name)),
            ..
        }) = argument
        else {
            return None;
        };
        let snapshot = environment.import(name.as_ref())?;
        for (name, incoming) in &snapshot.exports {
            let Some(existing) = result.get_mut(name) else {
                result.insert(name.clone(), incoming.clone());
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
    }
    Some(result)
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
        }
        Pattern::List { .. } | Pattern::Literal(_) | Pattern::Wildcard | Pattern::Pinned(_) => {}
    }
}

fn check_pattern(
    pattern: &Pattern,
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
) -> Result<(), SourceError> {
    match pattern {
        Pattern::At { pattern, .. } => check_pattern(pattern, environment, type_parameters, strict),
        Pattern::List { items, .. } => {
            for item in items {
                check_pattern(item, environment, type_parameters, strict)?;
            }
            Ok(())
        }
        Pattern::Map { entries, .. } => {
            for (key, value) in entries {
                if let MapPatternKey::Computed(key) = key {
                    check_expression(key, environment, type_parameters, strict)?;
                }
                check_pattern(value, environment, type_parameters, strict)?;
            }
            Ok(())
        }
        Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Binding(_)
        | Pattern::Pinned(_)
        | Pattern::MapAll => Ok(()),
    }
}

fn check_case_pattern(
    pattern: &CasePattern,
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
    span: &SourceSpan,
) -> Result<Option<Type>, SourceError> {
    let constraint = if let Some(constraint) = &pattern.constraint {
        let constraint = if strict {
            resolve_static_annotation(constraint, type_parameters, span, environment)?
        } else {
            resolve_annotation(constraint, type_parameters, span)?
        };
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
    check_pattern(&pattern.pattern, environment, type_parameters, strict)?;
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
        Pattern::Literal(_) | Pattern::Wildcard | Pattern::Pinned(_) | Pattern::MapAll => {}
    }
}

fn check_call(
    callee: &Expr,
    arguments: &[CallArgument],
    expression: &Expr,
    environment: &mut Environment,
    type_parameters: &[String],
    strict: bool,
    piped: Option<Type>,
) -> Result<Type, SourceError> {
    check_expression(callee, environment, type_parameters, strict)?;
    let mut actuals = piped.into_iter().collect::<Vec<_>>();
    actuals.extend(
        arguments
            .iter()
            .map(|argument| check_argument(argument, environment, type_parameters, strict))
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
            strict,
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
    strict: bool,
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
    if strict && actuals.len() != parameters.len() {
        return Err(SourceError::semantic(
            format!(
                "function value expects {} argument{}, got {}",
                parameters.len(),
                if parameters.len() == 1 { "" } else { "s" },
                actuals.len()
            ),
            span.clone(),
        ));
    }
    if strict {
        for (expected, actual) in parameters.iter().zip(actuals) {
            require(expected, &actual.clone().widen_unknown(), span)?;
        }
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
    strict: bool,
) -> Result<Type, SourceError> {
    let value = match argument {
        CallArgument::Positional(value)
        | CallArgument::Named { value, .. }
        | CallArgument::Spread(value) => value,
    };
    check_expression(value, environment, type_parameters, strict)
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
        let actual = (*actual).clone().widen_unknown();
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
        result: substitute(&signature.result, &substitutions).widen_unknown(),
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
    require(&substitute(expected, substitutions), actual, span)
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
        Value::Channel(_) => Type::Channel(None),
        Value::Closure(_)
        | Value::Native(_)
        | Value::DeclaredNative { .. }
        | Value::Builtin(_)
        | Value::Overloads(_) => Type::Function(None),
        Value::Task(_) => Type::Task(None),
        Value::NativeResource(_) => Type::Resource,
        Value::Uninitialized | Value::Binding { .. } => Type::Unknown,
    }
}

fn require(expected: &Type, actual: &Type, span: &crate::SourceSpan) -> Result<(), SourceError> {
    if actual.is_assignable_to(expected) {
        Ok(())
    } else {
        Err(SourceError::semantic(
            format!("expected {expected}, got {actual}"),
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
        ExprKind::Value(_)
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
        | Pattern::MapAll => Ok(()),
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
}
