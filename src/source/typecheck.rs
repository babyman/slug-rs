use std::collections::HashMap;

use crate::Value;

use super::{
    SourceError,
    ast::{Expr, ExprKind, ListElement, TypeAnnotation},
};

pub(super) fn check(expressions: &[Expr]) -> Result<(), SourceError> {
    let mut bindings = HashMap::new();
    for expression in expressions {
        check_expression(expression, &mut bindings)?;
    }
    Ok(())
}

fn check_expression(
    expression: &Expr,
    bindings: &mut HashMap<String, TypeAnnotation>,
) -> Result<Option<TypeAnnotation>, SourceError> {
    match &expression.kind {
        ExprKind::Declare {
            pattern,
            annotation,
            value,
            ..
        } => {
            let actual = check_expression(value, bindings)?;
            if let (Some(expected), Some(actual)) = (annotation, actual.as_ref()) {
                require(expected, actual, &expression.span)?;
            }
            if let (super::ast::Pattern::Binding(name), Some(annotation)) = (pattern, annotation) {
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
            let mut function_bindings = bindings.clone();
            for parameter in parameters {
                if let Some(annotation) = &parameter.annotation {
                    function_bindings.insert(parameter.name.clone(), annotation.clone());
                }
                if let Some(default) = &parameter.default {
                    let actual = check_expression(default, &mut function_bindings)?;
                    if let (Some(expected), Some(actual)) = (&parameter.annotation, actual.as_ref())
                    {
                        require(expected, actual, &default.span)?;
                    }
                }
            }
            for parameter in type_parameters {
                function_bindings
                    .insert(parameter.clone(), TypeAnnotation::Name(parameter.clone()));
            }
            let actual = check_expression(body, &mut function_bindings)?;
            if let (Some(expected), Some(actual)) = (return_annotation, actual.as_ref()) {
                require(expected, actual, &body.span)?;
            }
            Ok(Some(TypeAnnotation::Name("fn".into())))
        }
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let (Some(annotation), Some(default)) = (&field.annotation, &field.default)
                    && let Some(actual) = check_expression(default, bindings)?
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
                    check_expression(value, bindings)?;
                }
            }
            Ok(Some(TypeAnnotation::Name("list".into())))
        }
        ExprKind::Map(entries) => {
            for (key, value) in entries {
                check_expression(key, bindings)?;
                check_expression(value, bindings)?;
            }
            Ok(Some(TypeAnnotation::Name("map".into())))
        }
        ExprKind::Block(values) => {
            let mut scoped = bindings.clone();
            let mut result = Some(TypeAnnotation::Name("nil".into()));
            for value in values {
                result = check_expression(value, &mut scoped)?;
            }
            Ok(result)
        }
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let left = check_expression(then_branch, bindings)?;
            let right = else_branch
                .as_ref()
                .map(|branch| check_expression(branch, bindings))
                .transpose()?;
            Ok(left.or(right.flatten()))
        }
        ExprKind::Name(name) => Ok(bindings.get(name).cloned()),
        ExprKind::Return { value } | ExprKind::Throw { value } => check_expression(value, bindings),
        _ => Ok(None),
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
            Value::Closure(_) | Value::Native { .. } => "fn",
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
