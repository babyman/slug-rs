use std::rc::Rc;

use crate::{MatchMapKey, MatchPattern, MatchRest, StructValue, Value};

use super::RuntimeErrorKind;

#[allow(clippy::cast_precision_loss)]
pub(super) fn numbers(left: Value, right: Value) -> Result<(f64, f64), String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok((a as f64, b as f64)),
        (Value::Int(a), Value::Float(b)) => Ok((a as f64, b)),
        (Value::Float(a), Value::Int(b)) => Ok((a, b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok((a, b)),
        (a, b) => Err(format!(
            "expected numbers, got {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}
pub(super) fn add(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a
            .checked_add(b)
            .map(Value::Int)
            .ok_or((RuntimeErrorKind::Type, "integer overflow".into())),
        (Value::Str(a), Value::Str(b)) => Ok(Value::string(format!("{a}{b}"))),
        (a, b) => {
            let (a, b) = numbers(a, b).map_err(|message| (RuntimeErrorKind::Type, message))?;
            Ok(Value::Float(a + b))
        }
    }
}
pub(super) fn subtract(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    integer_or_float(left, right, i64::checked_sub, |a, b| a - b)
}
pub(super) fn multiply(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    integer_or_float(left, right, i64::checked_mul, |a, b| a * b)
}
pub(super) fn divide(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        if *right == 0 {
            return Err((RuntimeErrorKind::DivideByZero, "division by zero".into()));
        }
        if let Some(value) = left.checked_div(*right) {
            if left % right == 0 {
                return Ok(Value::Int(value));
            }
        } else {
            return Err((RuntimeErrorKind::Type, "integer overflow".into()));
        }
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    if b == 0.0 {
        Err((RuntimeErrorKind::DivideByZero, "division by zero".into()))
    } else {
        Ok(Value::Float(a / b))
    }
}
pub(super) fn modulo(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        return left.checked_rem(*right).map(Value::Int).ok_or_else(|| {
            if *right == 0 {
                (RuntimeErrorKind::DivideByZero, "division by zero".into())
            } else {
                (RuntimeErrorKind::Type, "integer overflow".into())
            }
        });
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    if b == 0.0 {
        Err((RuntimeErrorKind::DivideByZero, "division by zero".into()))
    } else {
        Ok(Value::Float(a % b))
    }
}

pub(super) fn bitwise(
    left: Value,
    right: Value,
    operation: fn(i64, i64) -> i64,
) -> Result<Value, (RuntimeErrorKind, String)> {
    let (Value::Int(left), Value::Int(right)) = (left, right) else {
        return Err((
            RuntimeErrorKind::Type,
            "bitwise operators require integers".into(),
        ));
    };
    Ok(Value::Int(operation(left, right)))
}

pub(super) fn shift(
    left: Value,
    right: Value,
    operation: fn(i64, u32) -> Option<i64>,
) -> Result<Value, (RuntimeErrorKind, String)> {
    let (Value::Int(left), Value::Int(right)) = (left, right) else {
        return Err((
            RuntimeErrorKind::Type,
            "shift operators require integers".into(),
        ));
    };
    let count = u32::try_from(right)
        .ok()
        .filter(|count| *count < i64::BITS)
        .ok_or((RuntimeErrorKind::Type, "shift count is out of range".into()))?;
    operation(left, count).map(Value::Int).ok_or((
        RuntimeErrorKind::Type,
        "shift result is out of range".into(),
    ))
}

pub(super) fn bit_not(value: &Value) -> Result<Value, String> {
    let Value::Int(value) = value else {
        return Err("bitwise operators require integers".into());
    };
    Ok(Value::Int(!value))
}

pub(super) fn list_append(list: Value, value: Value) -> Result<Value, String> {
    let Value::List(list) = list else {
        return Err("left operand of :+ must be a list".into());
    };
    let mut values = (*list).clone();
    values.push(value);
    Ok(Value::List(Rc::new(values)))
}

pub(super) fn list_prepend(value: Value, list: Value) -> Result<Value, String> {
    let Value::List(list) = list else {
        return Err("right operand of +: must be a list".into());
    };
    let mut values = Vec::with_capacity(list.len() + 1);
    values.push(value);
    values.extend(list.iter().cloned());
    Ok(Value::List(Rc::new(values)))
}

pub(super) fn matches_pattern(
    pattern: &MatchPattern,
    value: &Value,
    operands: &[Value],
    bindings: &mut Vec<Value>,
) -> Result<bool, (RuntimeErrorKind, String)> {
    match pattern {
        MatchPattern::Literal(expected) => Ok(value == expected),
        MatchPattern::Wildcard => Ok(true),
        MatchPattern::Binding => {
            bindings.push(value.clone());
            Ok(true)
        }
        MatchPattern::Pinned(index) => operands
            .get(*index)
            .map(|expected| value == expected)
            .ok_or_else(|| {
                (
                    RuntimeErrorKind::InvalidBytecode,
                    format!("match pattern operand {index} does not exist"),
                )
            }),
        MatchPattern::At(pattern) => {
            let binding_start = bindings.len();
            bindings.push(value.clone());
            if matches_pattern(pattern, value, operands, bindings)? {
                Ok(true)
            } else {
                bindings.truncate(binding_start);
                Ok(false)
            }
        }
        MatchPattern::Alternatives(patterns) => {
            let binding_start = bindings.len();
            for pattern in patterns {
                if matches_pattern(pattern, value, operands, bindings)? {
                    return Ok(true);
                }
                bindings.truncate(binding_start);
            }
            Ok(false)
        }
        MatchPattern::List { items, rest } => {
            let Value::List(values) = value else {
                return Ok(false);
            };
            if values.len() < items.len()
                || (*rest == MatchRest::None && values.len() != items.len())
            {
                return Ok(false);
            }
            let binding_start = bindings.len();
            for (item, value) in items.iter().zip(values.iter()) {
                if !matches_pattern(item, value, operands, bindings)? {
                    bindings.truncate(binding_start);
                    return Ok(false);
                }
            }
            if *rest == MatchRest::Binding {
                bindings.push(Value::List(Rc::new(values[items.len()..].to_vec())));
            }
            Ok(true)
        }
        MatchPattern::Map {
            entries: patterns,
            rest,
            exact,
        } => {
            let keys = resolve_map_pattern_keys(patterns, operands)?;
            let Value::Map(entries) = value else {
                return Ok(false);
            };
            if *exact && entries.len() != patterns.len() {
                return Ok(false);
            }
            let binding_start = bindings.len();
            for ((_, pattern), key) in patterns.iter().zip(&keys) {
                let Some((_, value)) = entries.iter().rev().find(|(entry, _)| entry == key) else {
                    bindings.truncate(binding_start);
                    return Ok(false);
                };
                if !matches_pattern(pattern, value, operands, bindings)? {
                    bindings.truncate(binding_start);
                    return Ok(false);
                }
            }
            if *rest == MatchRest::Binding {
                let rest_entries = entries
                    .iter()
                    .filter(|(key, _)| !keys.iter().any(|pattern_key| key == pattern_key))
                    .cloned()
                    .collect();
                bindings.push(Value::Map(Rc::new(rest_entries)));
            }
            Ok(true)
        }
    }
}

fn resolve_map_pattern_keys(
    patterns: &[(MatchMapKey, MatchPattern)],
    operands: &[Value],
) -> Result<Vec<Value>, (RuntimeErrorKind, String)> {
    patterns
        .iter()
        .map(|(key, _)| {
            let key = match key {
                MatchMapKey::String(key) => Value::string(key.clone()),
                MatchMapKey::Operand(index) => operands.get(*index).cloned().ok_or_else(|| {
                    (
                        RuntimeErrorKind::InvalidBytecode,
                        format!("match pattern operand {index} does not exist"),
                    )
                })?,
            };
            if !is_map_key(&key) {
                return Err((
                    RuntimeErrorKind::Type,
                    format!("{} cannot be used as a map key", key.type_name()),
                ));
            }
            Ok(key)
        })
        .collect()
}

pub(super) fn is_map_key(value: &Value) -> bool {
    matches!(
        value,
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Bytes(_)
    )
}

pub(super) fn index_value(collection: Value, index: &Value) -> Result<Value, String> {
    match collection {
        Value::List(values) => {
            let Value::Int(index) = index else {
                return Err("list index must be an integer".into());
            };
            let length = i64::try_from(values.len()).map_err(|_| "list is too large".to_owned())?;
            let index = if *index < 0 { length + *index } else { *index };
            usize::try_from(index)
                .ok()
                .and_then(|index| values.get(index))
                .cloned()
                .ok_or_else(|| "list index is out of bounds".into())
        }
        Value::Map(entries) => Ok(entries
            .iter()
            .rev()
            .find(|(key, _)| key == index)
            .map_or(Value::Nil, |(_, value)| value.clone())),
        Value::Struct(value) => {
            let Value::Str(name) = index else {
                return Err("struct index must be a string".into());
            };
            value
                .schema
                .fields
                .iter()
                .position(|field| field.name.as_ref() == name.as_ref())
                .and_then(|index| value.values.get(index))
                .cloned()
                .ok_or_else(|| format!("struct has no field '{name}'"))
        }
        value => Err(format!("cannot index {}", value.type_name())),
    }
}

pub(super) fn slice_value(
    collection: Value,
    start: Option<&Value>,
    end: Option<&Value>,
    step: Option<&Value>,
) -> Result<Value, String> {
    let Value::List(values) = collection else {
        return Err(format!("cannot slice {}", collection.type_name()));
    };
    let length = i64::try_from(values.len()).map_err(|_| "list is too large".to_owned())?;
    let start = slice_bound(start, 0, length, "start")?;
    let end = slice_bound(end, length, length, "end")?;
    let step = match step {
        None => 1,
        Some(Value::Int(step)) if *step > 0 => *step,
        Some(Value::Int(_)) => return Err("list slice step must be positive".into()),
        Some(_) => return Err("list slice step must be an integer".into()),
    };
    let mut result = Vec::new();
    let mut index = start;
    while index < end {
        result.push(values[usize::try_from(index).expect("slice bounds are non-negative")].clone());
        index = index
            .checked_add(step)
            .ok_or_else(|| "list slice step is too large".to_owned())?;
    }
    Ok(Value::List(Rc::new(result)))
}

fn slice_bound(
    value: Option<&Value>,
    default: i64,
    length: i64,
    name: &str,
) -> Result<i64, String> {
    let value = match value {
        None => default,
        Some(Value::Int(value)) if *value < 0 => length.saturating_add(*value),
        Some(Value::Int(value)) => *value,
        Some(_) => return Err(format!("list slice {name} must be an integer")),
    };
    Ok(value.clamp(0, length))
}

pub(super) fn construct_struct(
    schema: Value,
    names: &[String],
    provided: &[Value],
) -> Result<Value, String> {
    let Value::StructSchema(schema) = schema else {
        return Err(format!(
            "cannot construct struct from {}",
            schema.type_name()
        ));
    };
    for (index, name) in names.iter().enumerate() {
        if names[..index].contains(name) {
            return Err(format!("duplicate struct field '{name}'"));
        }
        if !schema
            .fields
            .iter()
            .any(|field| field.name.as_ref() == name)
        {
            return Err(format!("struct schema has no field '{name}'"));
        }
    }
    let values = schema
        .fields
        .iter()
        .map(|field| {
            names
                .iter()
                .position(|name| name == field.name.as_ref())
                .and_then(|index| provided.get(index).cloned())
                .or_else(|| field.default.clone())
                .ok_or_else(|| format!("missing required struct field '{}'", field.name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Struct(Rc::new(StructValue { schema, values })))
}

pub(super) fn copy_struct(
    value: Value,
    names: &[String],
    replacements: &[Value],
) -> Result<Value, String> {
    let Value::Struct(value) = value else {
        return Err("cannot copy non-struct value".into());
    };
    for (index, name) in names.iter().enumerate() {
        if names[..index].contains(name) {
            return Err(format!("duplicate struct field '{name}'"));
        }
        if !value
            .schema
            .fields
            .iter()
            .any(|field| field.name.as_ref() == name)
        {
            return Err(format!("struct has no field '{name}'"));
        }
    }
    let mut values = value.values.clone();
    for (name, replacement) in names.iter().zip(replacements) {
        let index = value
            .schema
            .fields
            .iter()
            .position(|field| field.name.as_ref() == name)
            .expect("field names were validated");
        values[index] = replacement.clone();
    }
    Ok(Value::Struct(Rc::new(StructValue {
        schema: value.schema.clone(),
        values,
    })))
}

fn integer_or_float(
    left: Value,
    right: Value,
    integer_operation: fn(i64, i64) -> Option<i64>,
    operation: fn(f64, f64) -> f64,
) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        return integer_operation(*left, *right)
            .map(Value::Int)
            .ok_or((RuntimeErrorKind::Type, "integer overflow".into()));
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    Ok(Value::Float(operation(a, b)))
}
pub(super) fn negate(value: Value) -> Result<Value, String> {
    match value {
        Value::Int(value) => value
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow".into()),
        Value::Float(value) => Ok(Value::Float(-value)),
        value => Err(format!("expected number, got {}", value.type_name())),
    }
}
