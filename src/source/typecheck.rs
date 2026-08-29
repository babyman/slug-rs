use std::collections::HashMap;

use crate::Value;

use super::{
    SourceError,
    ast::{
        Binary, CallArgument, Expr, ExprKind, ListElement, MapPatternKey, Parameter, Pattern,
        SelectCaseKind, Tag, TypeAnnotation,
    },
    semantic::{Type, resolve_annotation},
};

#[derive(Clone)]
struct FunctionParameter {
    name: String,
    value_type: Type,
    variadic: bool,
}

#[derive(Clone)]
struct FunctionType {
    type_parameters: Vec<String>,
    parameters: Vec<FunctionParameter>,
    result: Type,
}

pub(super) fn validate(expressions: &[Expr]) -> Result<(), SourceError> {
    for expression in expressions {
        validate_expression(expression, &[])?;
    }
    Ok(())
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
                function_type(
                    type_parameters,
                    parameters,
                    return_annotation.as_ref(),
                    &expression.span,
                )?,
            );
        }
        if let ExprKind::Foreign {
            name, signature, ..
        } = &expression.kind
        {
            functions.insert(
                name.clone(),
                function_type(
                    &signature.type_parameters,
                    &signature.parameters,
                    signature.return_annotation.as_ref(),
                    &expression.span,
                )?,
            );
        }
        check_expression(expression, &mut bindings, &functions, &[])?;
    }
    Ok(())
}

fn function_type(
    type_parameters: &[String],
    parameters: &[Parameter],
    result: Option<&TypeAnnotation>,
    span: &crate::SourceSpan,
) -> Result<FunctionType, SourceError> {
    Ok(FunctionType {
        type_parameters: type_parameters.to_vec(),
        parameters: parameters
            .iter()
            .map(|parameter| {
                Ok(FunctionParameter {
                    name: parameter.name.clone(),
                    value_type: parameter
                        .annotation
                        .as_ref()
                        .map(|annotation| resolve_annotation(annotation, type_parameters, span))
                        .transpose()?
                        .unwrap_or_else(Type::universal),
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

#[allow(clippy::too_many_lines)]
fn check_expression(
    expression: &Expr,
    bindings: &mut HashMap<String, Type>,
    functions: &HashMap<String, FunctionType>,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    match &expression.kind {
        ExprKind::Declare {
            pattern,
            annotation,
            value,
            ..
        } => {
            let actual = check_expression(value, bindings, functions, type_parameters)?;
            let declared = annotation
                .as_ref()
                .map(|annotation| resolve_annotation(annotation, type_parameters, &expression.span))
                .transpose()?;
            if let Some(expected) = &declared {
                require(expected, &actual, &expression.span)?;
            }
            if let Pattern::Binding(name) = pattern {
                bindings.insert(
                    name.clone(),
                    declared.unwrap_or_else(|| actual.clone().widen_unknown()),
                );
            }
            Ok(actual)
        }
        ExprKind::Foreign { signature, .. } => {
            for parameter in &signature.parameters {
                if let Some(default) = &parameter.default {
                    let actual =
                        check_expression(default, bindings, functions, &signature.type_parameters)?;
                    if let Some(annotation) = &parameter.annotation {
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
            let mut scoped = bindings.clone();
            for parameter in parameters {
                let parameter_type = parameter
                    .annotation
                    .as_ref()
                    .map(|annotation| {
                        resolve_annotation(annotation, function_type_parameters, &body.span)
                    })
                    .transpose()?
                    .unwrap_or_else(Type::universal);
                scoped.insert(parameter.name.clone(), parameter_type.clone());
                if let Some(default) = &parameter.default {
                    let actual = check_expression(
                        default,
                        &mut scoped,
                        functions,
                        function_type_parameters,
                    )?;
                    require(&parameter_type, &actual, &default.span)?;
                }
            }
            let actual = check_expression(body, &mut scoped, functions, function_type_parameters)?;
            if let Some(return_annotation) = return_annotation {
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
            bindings,
            functions,
            type_parameters,
        ),
        ExprKind::TypeApply { callee, .. } => {
            check_expression(callee, bindings, functions, type_parameters)
        }
        ExprKind::StructSchema(fields) => {
            for field in fields {
                if let Some(default) = &field.default {
                    let actual = check_expression(default, bindings, functions, type_parameters)?;
                    if let Some(annotation) = &field.annotation {
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
                        bindings,
                        functions,
                        type_parameters,
                    )?),
                    ListElement::Spread(value) => {
                        let spread = check_expression(value, bindings, functions, type_parameters)?;
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
                keys.push(check_expression(key, bindings, functions, type_parameters)?);
                values.push(check_expression(
                    value,
                    bindings,
                    functions,
                    type_parameters,
                )?);
            }
            Ok(Type::Map((!entries.is_empty()).then(|| {
                (Box::new(Type::union(keys)), Box::new(Type::union(values)))
            })))
        }
        ExprKind::Block(values) => {
            let mut scoped = bindings.clone();
            let mut result = Type::Nil;
            for value in values {
                result = check_expression(value, &mut scoped, functions, type_parameters)?;
            }
            Ok(result)
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            check_expression(condition, bindings, functions, type_parameters)?;
            let left = check_expression(then_branch, bindings, functions, type_parameters)?;
            let right = else_branch
                .as_ref()
                .map(|branch| check_expression(branch, bindings, functions, type_parameters))
                .transpose()?
                .unwrap_or(Type::Nil);
            Ok(Type::union([left, right]))
        }
        ExprKind::Binary {
            left,
            operator,
            right,
        } => {
            let left = check_expression(left, bindings, functions, type_parameters)?;
            let right = check_expression(right, bindings, functions, type_parameters)?;
            Ok(binary_result(*operator, left, right))
        }
        ExprKind::Prefix { value, .. } => {
            check_expression(value, bindings, functions, type_parameters)
        }
        ExprKind::Name(name) => Ok(bindings.get(name).cloned().unwrap_or(Type::Unknown)),
        ExprKind::Assign { name, value } => {
            let actual = check_expression(value, bindings, functions, type_parameters)?;
            if let Some(expected) = bindings.get(name) {
                require(expected, &actual, &value.span)?;
            }
            Ok(actual)
        }
        ExprKind::Return { value } => check_expression(value, bindings, functions, type_parameters),
        ExprKind::Throw { value } => {
            check_expression(value, bindings, functions, type_parameters)?;
            Ok(Type::Unknown)
        }
        ExprKind::Defer { value, .. } => {
            check_expression(value, bindings, functions, type_parameters)?;
            Ok(Type::Nil)
        }
        ExprKind::Spawn(value) => {
            let result = check_expression(value, bindings, functions, type_parameters)?;
            Ok(Type::Task(Some(Box::new(result.widen_unknown()))))
        }
        ExprKind::Nursery { limit, body } => {
            if let Some(limit) = limit {
                check_expression(limit, bindings, functions, type_parameters)?;
            }
            check_expression(body, bindings, functions, type_parameters)
        }
        ExprKind::Recur(arguments) => {
            for argument in arguments {
                check_argument(argument, bindings, functions, type_parameters)?;
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
                        check_expression(value, bindings, functions, type_parameters)?;
                    }
                    SelectCaseKind::Send { channel, value } => {
                        check_expression(channel, bindings, functions, type_parameters)?;
                        check_expression(value, bindings, functions, type_parameters)?;
                    }
                    SelectCaseKind::Default => {}
                }
                results.push(
                    case.handler
                        .as_ref()
                        .map(|handler| {
                            check_expression(handler, bindings, functions, type_parameters)
                        })
                        .transpose()?
                        .unwrap_or(Type::Unknown),
                );
            }
            Ok(Type::union(results))
        }
        ExprKind::Match { subject, cases } => {
            if let Some(subject) = subject {
                check_expression(subject, bindings, functions, type_parameters)?;
            }
            let mut results = Vec::new();
            for case in cases {
                for pattern in &case.patterns {
                    check_pattern(pattern, bindings, functions, type_parameters)?;
                }
                if let Some(guard) = &case.guard {
                    check_expression(guard, bindings, functions, type_parameters)?;
                }
                results.push(check_expression(
                    &case.value,
                    bindings,
                    functions,
                    type_parameters,
                )?);
            }
            Ok(Type::union(results))
        }
        ExprKind::StructInit { schema, fields } => {
            check_expression(schema, bindings, functions, type_parameters)?;
            for (_, value) in fields {
                check_expression(value, bindings, functions, type_parameters)?;
            }
            Ok(Type::Struct(None))
        }
        ExprKind::StructCopy { value, fields } => {
            let result = check_expression(value, bindings, functions, type_parameters)?;
            for (_, replacement) in fields {
                check_expression(replacement, bindings, functions, type_parameters)?;
            }
            Ok(result)
        }
        ExprKind::Index { collection, index } => {
            check_expression(collection, bindings, functions, type_parameters)?;
            check_expression(index, bindings, functions, type_parameters)?;
            Ok(Type::Unknown)
        }
        ExprKind::Slice {
            collection,
            start,
            end,
            step,
        } => {
            let result = check_expression(collection, bindings, functions, type_parameters)?;
            for bound in [start, end, step].into_iter().flatten() {
                check_expression(bound, bindings, functions, type_parameters)?;
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

fn check_pattern(
    pattern: &Pattern,
    bindings: &mut HashMap<String, Type>,
    functions: &HashMap<String, FunctionType>,
    type_parameters: &[String],
) -> Result<(), SourceError> {
    match pattern {
        Pattern::At { pattern, .. } => check_pattern(pattern, bindings, functions, type_parameters),
        Pattern::List { items, .. } => {
            for item in items {
                check_pattern(item, bindings, functions, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Map { entries, .. } => {
            for (key, value) in entries {
                if let MapPatternKey::Computed(key) = key {
                    check_expression(key, bindings, functions, type_parameters)?;
                }
                check_pattern(value, bindings, functions, type_parameters)?;
            }
            Ok(())
        }
        Pattern::Struct { fields, .. } => {
            for (_, field) in fields {
                check_pattern(field, bindings, functions, type_parameters)?;
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
    bindings: &mut HashMap<String, Type>,
    functions: &HashMap<String, FunctionType>,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    check_expression(callee, bindings, functions, type_parameters)?;
    let actuals = arguments
        .iter()
        .map(|argument| check_argument(argument, bindings, functions, type_parameters))
        .collect::<Result<Vec<_>, _>>()?;
    let (name, explicit) = match &callee.kind {
        ExprKind::Name(name) => (name, None),
        ExprKind::TypeApply { callee, arguments } => {
            let ExprKind::Name(name) = &callee.kind else {
                return Ok(Type::Unknown);
            };
            (name, Some(arguments))
        }
        _ => return Ok(Type::Unknown),
    };
    let Some(function) = functions.get(name) else {
        return Ok(Type::Unknown);
    };
    let mut substitutions = HashMap::new();
    if let Some(explicit) = explicit {
        if explicit.len() != function.type_parameters.len() {
            return Err(SourceError::semantic(
                "wrong number of type arguments",
                expression.span.clone(),
            ));
        }
        for (index, value) in explicit.iter().enumerate() {
            let value = resolve_annotation(value, type_parameters, &expression.span)?;
            if value.includes_nil() {
                return Err(SourceError::semantic(
                    "generic type argument cannot include nil",
                    expression.span.clone(),
                ));
            }
            substitutions.insert(index, value);
        }
    }
    let variadic = function
        .parameters
        .last()
        .is_some_and(|parameter| parameter.variadic);
    let mut positional = 0usize;
    for (argument, actual) in arguments.iter().zip(&actuals) {
        let parameter = match argument {
            CallArgument::Positional(_) => {
                let parameter = function
                    .parameters
                    .get(positional)
                    .or_else(|| variadic.then(|| function.parameters.last()).flatten());
                positional += 1;
                parameter
            }
            CallArgument::Named { name, .. } => function
                .parameters
                .iter()
                .find(|parameter| parameter.name == *name),
            CallArgument::Spread(_) => None,
        };
        if let Some(parameter) = parameter {
            infer(
                &parameter.value_type,
                actual,
                &mut substitutions,
                &expression.span,
            )?;
        }
    }
    Ok(substitute(&function.result, &substitutions).widen_unknown())
}

fn check_argument(
    argument: &CallArgument,
    bindings: &mut HashMap<String, Type>,
    functions: &HashMap<String, FunctionType>,
    type_parameters: &[String],
) -> Result<Type, SourceError> {
    let value = match argument {
        CallArgument::Positional(value)
        | CallArgument::Named { value, .. }
        | CallArgument::Spread(value) => value,
    };
    check_expression(value, bindings, functions, type_parameters)
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
        Value::Closure(_) | Value::Native(_) | Value::Builtin(_) | Value::Overloads(_) => {
            Type::Function(None)
        }
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
