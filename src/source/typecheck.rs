use std::collections::HashMap;

use crate::Value;

use super::{
    SourceError,
    ast::{
        Binary, CallArgument, Expr, ExprKind, ListElement, MapPatternKey, Parameter, Pattern,
        SelectCaseKind, Tag, TypeAnnotation,
    },
    environment::{
        CallableParameter, CallableSignature, Environment, ImportSnapshots, ModuleSnapshot,
        SemanticAnalysis, SemanticBinding,
    },
    semantic::{Type, resolve_annotation},
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
                        .map(|annotation| resolve_annotation(annotation, type_parameters, span))
                        .transpose()?
                        .unwrap_or_else(Type::universal),
                    has_default: parameter.default.is_some(),
                    variadic: parameter.variadic,
                })
            })
            .collect::<Result<Vec<_>, SourceError>>()?,
        result: result
            .map(|annotation| resolve_annotation(annotation, type_parameters, span))
            .transpose()?
            .unwrap_or(Type::Unknown),
    })
}

fn callable_signature(
    expression: &Expr,
    span: &crate::SourceSpan,
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
        Pattern::Struct { fields, .. } => {
            for (_, value) in fields {
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
                callable_signature(value, &expression.span)?
                    .map(|signature| (name.clone(), signature))
            } else {
                None
            };
            if let Some((name, signature)) = &callable {
                environment.declare_callable(name.clone(), signature.clone(), &expression.span)?;
            }
            let actual = check_expression(value, environment, type_parameters, strict)?;
            let declared = annotation
                .as_ref()
                .map(|annotation| resolve_annotation(annotation, type_parameters, &expression.span))
                .transpose()?;
            if strict && let Some(expected) = &declared {
                require(expected, &actual, &expression.span)?;
            }
            if callable.is_none() {
                let mut binding = expression_binding(value, environment)
                    .unwrap_or_else(|| SemanticBinding::value(actual.clone().widen_unknown()));
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
            )?;
            environment.record_foreign(expression.span.clone(), callable.identity());
            environment.declare_callable(name.clone(), callable, &expression.span)?;
            for parameter in &signature.parameters {
                if let Some(default) = &parameter.default {
                    let actual =
                        check_expression(default, environment, &signature.type_parameters, strict)?;
                    if strict && let Some(annotation) = &parameter.annotation {
                        let expected = resolve_annotation(
                            annotation,
                            &signature.type_parameters,
                            &default.span,
                        )?;
                        require(&expected, &actual, &default.span)?;
                    }
                }
            }
            Ok(Type::Function(None))
        }
        ExprKind::Function {
            type_parameters: function_type_parameters,
            parameters,
            return_annotation,
            body,
        } => {
            environment.record_function(
                expression.span.clone(),
                function_type(
                    function_type_parameters,
                    parameters,
                    return_annotation.as_ref(),
                    &expression.span,
                )?
                .identity(),
            );
            let mut scoped = environment.clone();
            scoped.enter_scope();
            for parameter in parameters {
                let parameter_type = parameter
                    .annotation
                    .as_ref()
                    .map(|annotation| {
                        resolve_annotation(annotation, function_type_parameters, &body.span)
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
                let expected =
                    resolve_annotation(return_annotation, function_type_parameters, &body.span)?;
                require(&expected, &actual, &body.span)?;
            }
            Ok(Type::Function(None))
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
                        let expected =
                            resolve_annotation(annotation, type_parameters, &default.span)?;
                        require(&expected, &actual, &default.span)?;
                    }
                }
            }
            Ok(Type::Struct(None))
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
            Ok(result)
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            check_expression(condition, environment, type_parameters, strict)?;
            let left = check_expression(then_branch, environment, type_parameters, strict)?;
            let right = else_branch
                .as_ref()
                .map(|branch| check_expression(branch, environment, type_parameters, strict))
                .transpose()?
                .unwrap_or(Type::Nil);
            Ok(Type::union([left, right]))
        }
        ExprKind::Binary {
            left,
            operator,
            right,
        } => {
            let left = check_expression(left, environment, type_parameters, strict)?;
            if matches!(operator, Binary::Pipeline) {
                return match &right.kind {
                    ExprKind::Call { callee, arguments } => check_call(
                        callee,
                        arguments,
                        right,
                        environment,
                        type_parameters,
                        strict,
                        Some(left),
                    ),
                    ExprKind::Name(_) | ExprKind::TypeApply { .. } => check_call(
                        right,
                        &[],
                        right,
                        environment,
                        type_parameters,
                        strict,
                        Some(left),
                    ),
                    _ => check_expression(right, environment, type_parameters, strict),
                };
            }
            let right = check_expression(right, environment, type_parameters, strict)?;
            Ok(binary_result(*operator, left, right))
        }
        ExprKind::Prefix { value, .. } => {
            check_expression(value, environment, type_parameters, strict)
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
            if let Some(subject) = subject {
                check_expression(subject, environment, type_parameters, strict)?;
            }
            let mut results = Vec::new();
            for case in cases {
                for pattern in &case.patterns {
                    check_pattern(pattern, environment, type_parameters, strict)?;
                }
                if let Some(guard) = &case.guard {
                    check_expression(guard, environment, type_parameters, strict)?;
                }
                results.push(check_expression(
                    &case.value,
                    environment,
                    type_parameters,
                    strict,
                )?);
            }
            Ok(Type::union(results))
        }
        ExprKind::StructInit { schema, fields } => {
            check_expression(schema, environment, type_parameters, strict)?;
            for (_, value) in fields {
                check_expression(value, environment, type_parameters, strict)?;
            }
            Ok(Type::Struct(None))
        }
        ExprKind::StructCopy { value, fields } => {
            let result = check_expression(value, environment, type_parameters, strict)?;
            for (_, replacement) in fields {
                check_expression(replacement, environment, type_parameters, strict)?;
            }
            Ok(result)
        }
        ExprKind::Index { collection, index } => {
            let binding = expression_binding(expression, environment);
            check_expression(collection, environment, type_parameters, strict)?;
            check_expression(index, environment, type_parameters, strict)?;
            Ok(binding.map_or(Type::Unknown, |binding| binding.value_type))
        }
        ExprKind::Slice {
            collection,
            start,
            end,
            step,
        } => {
            let result = check_expression(collection, environment, type_parameters, strict)?;
            for bound in [start, end, step].into_iter().flatten() {
                check_expression(bound, environment, type_parameters, strict)?;
            }
            Ok(result)
        }
        ExprKind::Interpolate(_) => Ok(Type::Str),
        ExprKind::Documentation(_) => Ok(Type::Nil),
        ExprKind::NotImplemented => Ok(Type::Unknown),
    }
}

fn binary_result(operator: Binary, left: Type, right: Type) -> Type {
    match operator {
        Binary::Or
        | Binary::And
        | Binary::Equal
        | Binary::NotEqual
        | Binary::Greater
        | Binary::GreaterEqual
        | Binary::Less
        | Binary::LessEqual => Type::Bool,
        Binary::BitOr
        | Binary::BitXor
        | Binary::BitAnd
        | Binary::ShiftLeft
        | Binary::ShiftRight
        | Binary::Subtract
        | Binary::Divide
        | Binary::Modulo => Type::Num,
        Binary::Append | Binary::Prepend => Type::List(None),
        Binary::Add | Binary::Multiply => {
            if matches!(left, Type::Unknown) {
                right
            } else {
                left
            }
        }
        Binary::Pipeline => right,
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
            for (key, pattern) in entries {
                let MapPatternKey::String(key) = key else {
                    continue;
                };
                if let Some(member) = binding.members.get(key) {
                    bind_semantic_pattern(pattern, member, environment);
                }
            }
        }
        Pattern::MapAll => {
            for (name, member) in &binding.members {
                environment.declare(name.clone(), member.clone());
            }
        }
        Pattern::List { .. }
        | Pattern::Struct { .. }
        | Pattern::Literal(_)
        | Pattern::Wildcard
        | Pattern::Pinned(_) => {}
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
        Pattern::Struct { fields, .. } => {
            for (_, field) in fields {
                check_pattern(field, environment, type_parameters, strict)?;
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
        return Ok(Type::Unknown);
    }
    let callables = binding.callables.clone();
    let mut applicable = Vec::new();
    for signature in &callables {
        if let Some(candidate) = instantiate_candidate(
            signature,
            &shapes,
            &actuals,
            explicit,
            type_parameters,
            &expression.span,
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
    result: Type,
    identity: super::environment::CallableIdentity,
}

#[derive(Clone, Copy)]
enum ArgumentShape<'a> {
    Positional,
    Named(&'a str),
    Spread,
}

fn instantiate_candidate(
    signature: &CallableSignature,
    arguments: &[ArgumentShape<'_>],
    actuals: &[Type],
    explicit: Option<&Vec<TypeAnnotation>>,
    type_parameters: &[String],
    span: &crate::SourceSpan,
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
            let value = resolve_annotation(value, type_parameters, span)?;
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
    for (parameter, actual) in &bound {
        let actual = (*actual).clone().widen_unknown();
        if let Err(error) = infer(&parameter.value_type, &actual, &mut substitutions, span) {
            if report_mismatch {
                return Err(error);
            }
            return Ok(None);
        }
    }
    let bound_types = bound
        .iter()
        .map(|(parameter, _)| substitute(&parameter.value_type, &substitutions).widen_unknown())
        .collect();
    Ok(Some(InstantiatedCandidate {
        bound_types,
        generic_arity: signature.generic_arity,
        result: substitute(&signature.result, &substitutions).widen_unknown(),
        identity: signature.identity(),
    }))
}

fn bind_arguments<'a>(
    parameters: &'a [CallableParameter],
    arguments: &[ArgumentShape<'_>],
    actuals: &'a [Type],
) -> Option<Vec<(&'a CallableParameter, &'a Type)>> {
    if arguments
        .iter()
        .any(|argument| matches!(argument, ArgumentShape::Spread))
    {
        return Some(Vec::new());
    }
    let variadic = parameters
        .last()
        .is_some_and(|parameter| parameter.variadic);
    let fixed = parameters.len() - usize::from(variadic);
    let mut assigned = vec![false; parameters.len()];
    let mut bound = Vec::new();
    let mut positional = 0usize;
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
    Some(bound)
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
    left.bound_types
        .iter()
        .zip(&right.bound_types)
        .any(|(left, right)| !right.is_assignable_to(left))
        || left.generic_arity < right.generic_arity
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

fn value_type(value: &Value) -> Type {
    match value {
        Value::Nil => Type::Nil,
        Value::Bool(_) => Type::Bool,
        Value::Int(_) | Value::Float(_) => Type::Num,
        Value::Str(_) => Type::Str,
        Value::Bytes(_) => Type::Bytes,
        Value::List(_) => Type::List(None),
        Value::Map(_) => Type::Map(None),
        Value::StructSchema(_) | Value::Struct(_) => Type::Struct(None),
        Value::Channel(_) => Type::Channel(None),
        Value::Closure(_)
        | Value::Native(_)
        | Value::DeclaredNative { .. }
        | Value::Builtin(_)
        | Value::Overloads(_) => Type::Function(None),
        Value::Task(_) => Type::Task(None),
        Value::NativeResource(_) => Type::Any,
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
                    validate_pattern(pattern, type_parameters)?;
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
        Pattern::Struct { fields, .. } => {
            for (_, field) in fields {
                validate_pattern(field, type_parameters)?;
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
