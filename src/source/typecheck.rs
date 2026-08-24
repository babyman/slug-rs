use std::collections::HashMap;

use crate::Value;

use super::{
    SourceError,
    ast::{CallArgument, Expr, ExprKind, ListElement, Pattern, TypeAnnotation},
};

#[derive(Clone)]
struct FunctionType {
    type_parameters: Vec<String>,
    parameters: Vec<(String, Option<TypeAnnotation>, bool)>,
    result: Option<TypeAnnotation>,
}

pub(super) fn check(expressions: &[Expr]) -> Result<(), SourceError> {
    let mut bindings = HashMap::new();
    let mut functions = HashMap::new();
    for expression in expressions {
        if let ExprKind::Declare {
            pattern: Pattern::Binding(name),
            value,
            ..
        } = &expression.kind
            && let ExprKind::Function {
                type_parameters,
                parameters,
                return_annotation,
                ..
            } = &value.kind
        {
            functions.insert(
                name.clone(),
                FunctionType {
                    type_parameters: type_parameters.clone(),
                    parameters: parameters
                        .iter()
                        .map(|parameter| {
                            (
                                parameter.name.clone(),
                                parameter.annotation.clone(),
                                parameter.variadic,
                            )
                        })
                        .collect(),
                    result: return_annotation.clone(),
                },
            );
        }
        check_expression(expression, &mut bindings, &functions)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn check_expression(
    expression: &Expr,
    bindings: &mut HashMap<String, TypeAnnotation>,
    functions: &HashMap<String, FunctionType>,
) -> Result<Option<TypeAnnotation>, SourceError> {
    match &expression.kind {
        ExprKind::Declare {
            pattern,
            annotation,
            value,
            ..
        } => {
            let actual = check_expression(value, bindings, functions)?;
            if let (Some(expected), Some(actual)) = (annotation, actual.as_ref()) {
                require(expected, actual, &expression.span)?;
            }
            if let (Pattern::Binding(name), Some(annotation)) = (pattern, annotation) {
                bindings.insert(name.clone(), annotation.clone());
            }
            Ok(actual)
        }
        ExprKind::Function {
            type_parameters,
            parameters,
            return_annotation,
            body,
        } => {
            let mut scoped = bindings.clone();
            for parameter in parameters {
                if let Some(annotation) = &parameter.annotation {
                    scoped.insert(parameter.name.clone(), annotation.clone());
                }
                if let (Some(expected), Some(default)) = (&parameter.annotation, &parameter.default)
                    && let Some(actual) = check_expression(default, &mut scoped, functions)?
                {
                    require(expected, &actual, &default.span)?;
                }
            }
            let actual = check_expression(body, &mut scoped, functions)?;
            if let (Some(expected), Some(actual)) = (return_annotation, actual.as_ref()) {
                require(expected, actual, &body.span)?;
            }
            let _ = type_parameters;
            Ok(Some(TypeAnnotation::Name("fn".into())))
        }
        ExprKind::Call { callee, arguments } => {
            check_call(callee, arguments, expression, bindings, functions)
        }
        ExprKind::TypeApply { callee, .. } => check_expression(callee, bindings, functions),
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let (Some(annotation), Some(default)) = (&field.annotation, &field.default)
                    && let Some(actual) = check_expression(default, bindings, functions)?
                {
                    require(annotation, &actual, &default.span)?;
                }
            }
            Ok(Some(TypeAnnotation::Name("struct".into())))
        }
        ExprKind::Value(value) => Ok(Some(value_type(value))),
        ExprKind::List(values) => {
            for value in values {
                if let ListElement::Value(value) = value {
                    check_expression(value, bindings, functions)?;
                }
            }
            Ok(Some(TypeAnnotation::Name("list".into())))
        }
        ExprKind::Map(entries) => {
            for (key, value) in entries {
                check_expression(key, bindings, functions)?;
                check_expression(value, bindings, functions)?;
            }
            Ok(Some(TypeAnnotation::Name("map".into())))
        }
        ExprKind::Block(values) => {
            let mut scoped = bindings.clone();
            let mut result = Some(TypeAnnotation::Name("nil".into()));
            for value in values {
                result = check_expression(value, &mut scoped, functions)?;
            }
            Ok(result)
        }
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let left = check_expression(then_branch, bindings, functions)?;
            let right = else_branch
                .as_ref()
                .map(|branch| check_expression(branch, bindings, functions))
                .transpose()?;
            Ok(left.or(right.flatten()))
        }
        ExprKind::Binary { left, right, .. } => {
            check_expression(left, bindings, functions)?;
            check_expression(right, bindings, functions)
        }
        ExprKind::Prefix { value, .. } => check_expression(value, bindings, functions),
        ExprKind::Name(name) => Ok(bindings.get(name).cloned()),
        ExprKind::Return { value } | ExprKind::Throw { value } => {
            check_expression(value, bindings, functions)
        }
        _ => Ok(None),
    }
}

fn check_call(
    callee: &Expr,
    arguments: &[CallArgument],
    expression: &Expr,
    bindings: &mut HashMap<String, TypeAnnotation>,
    functions: &HashMap<String, FunctionType>,
) -> Result<Option<TypeAnnotation>, SourceError> {
    let (name, explicit) = match &callee.kind {
        ExprKind::Name(name) => (name, None),
        ExprKind::TypeApply { callee, arguments } => {
            let ExprKind::Name(name) = &callee.kind else {
                return Ok(None);
            };
            (name, Some(arguments))
        }
        _ => return Ok(None),
    };
    let Some(function) = functions.get(name) else {
        return Ok(None);
    };
    let mut substitutions = HashMap::new();
    if let Some(explicit) = explicit {
        if explicit.len() != function.type_parameters.len() {
            return Err(SourceError::semantic(
                "wrong number of type arguments",
                expression.span.clone(),
            ));
        }
        for (parameter, value) in function.type_parameters.iter().zip(explicit) {
            substitutions.insert(parameter.clone(), value.clone());
        }
    }
    let mut positional = 0usize;
    for argument in arguments {
        let (parameter, value) = match argument {
            CallArgument::Positional(value) => {
                let parameter = function.parameters.get(positional);
                positional += 1;
                (parameter, value)
            }
            CallArgument::Named { name, value } => (
                function
                    .parameters
                    .iter()
                    .find(|(parameter, _, _)| parameter == name),
                value,
            ),
            CallArgument::Spread(value) => {
                check_expression(value, bindings, functions)?;
                continue;
            }
        };
        let Some((_, Some(expected), _)) = parameter else {
            continue;
        };
        let Some(actual) = check_expression(value, bindings, functions)? else {
            continue;
        };
        infer(
            expected,
            &actual,
            &function.type_parameters,
            &mut substitutions,
            &value.span,
        )?;
    }
    Ok(function
        .result
        .as_ref()
        .map(|result| substitute(result, &substitutions)))
}

fn infer(
    expected: &TypeAnnotation,
    actual: &TypeAnnotation,
    parameters: &[String],
    substitutions: &mut HashMap<String, TypeAnnotation>,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    if let TypeAnnotation::Name(name) = expected
        && parameters.contains(name)
    {
        if let Some(previous) = substitutions.get(name) {
            return require(previous, actual, span);
        }
        substitutions.insert(name.clone(), actual.clone());
        return Ok(());
    }
    require(&substitute(expected, substitutions), actual, span)
}

fn substitute(
    annotation: &TypeAnnotation,
    substitutions: &HashMap<String, TypeAnnotation>,
) -> TypeAnnotation {
    match annotation {
        TypeAnnotation::Name(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| annotation.clone()),
        TypeAnnotation::Apply { name, arguments } => TypeAnnotation::Apply {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute(argument, substitutions))
                .collect(),
        },
        TypeAnnotation::Tuple(values) => TypeAnnotation::Tuple(
            values
                .iter()
                .map(|value| substitute(value, substitutions))
                .collect(),
        ),
        TypeAnnotation::Union(values) => TypeAnnotation::Union(
            values
                .iter()
                .map(|value| substitute(value, substitutions))
                .collect(),
        ),
    }
}

fn value_type(value: &Value) -> TypeAnnotation {
    TypeAnnotation::Name(
        match value {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) | Value::Float(_) => "num",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::StructSchema(_) | Value::Struct(_) => "struct",
            Value::Channel(_) => "chan",
            Value::Closure(_) | Value::Native(_) | Value::Builtin(_) | Value::Overloads(_) => "fn",
            Value::NativeResource(_) => "native resource",
            Value::Task(_) => "task",
            Value::Uninitialized | Value::Binding { .. } => "binding",
        }
        .into(),
    )
}

fn require(
    expected: &TypeAnnotation,
    actual: &TypeAnnotation,
    span: &crate::SourceSpan,
) -> Result<(), SourceError> {
    if conforms(actual, expected) {
        Ok(())
    } else {
        Err(SourceError::semantic(
            format!("expected {}, got {}", display(expected), display(actual)),
            span.clone(),
        ))
    }
}

fn conforms(actual: &TypeAnnotation, expected: &TypeAnnotation) -> bool {
    match expected {
        TypeAnnotation::Union(members) => members.iter().any(|member| conforms(actual, member)),
        TypeAnnotation::Name(expected) => match expected.as_str() {
            "nil" | "bool" | "num" | "str" | "bytes" | "list" | "map" | "fn" | "struct" => {
                matches!(actual, TypeAnnotation::Name(actual) if actual == expected)
            }
            _ => true,
        },
        TypeAnnotation::Apply { name, .. } => {
            matches!(actual, TypeAnnotation::Name(actual) if actual == name)
        }
        TypeAnnotation::Tuple(_) => false,
    }
}

fn display(annotation: &TypeAnnotation) -> String {
    match annotation {
        TypeAnnotation::Name(name) | TypeAnnotation::Apply { name, .. } => name.clone(),
        TypeAnnotation::Tuple(_) => "tuple".into(),
        TypeAnnotation::Union(members) => members.iter().map(display).collect::<Vec<_>>().join("|"),
    }
}
