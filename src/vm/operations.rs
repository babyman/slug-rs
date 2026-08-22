use std::rc::Rc;

use crate::{MatchPattern, Value};

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

pub(super) fn matches_pattern(
    pattern: &MatchPattern,
    value: &Value,
    bindings: &mut Vec<Value>,
) -> bool {
    match pattern {
        MatchPattern::Literal(expected) => value == expected,
        MatchPattern::Wildcard => true,
        MatchPattern::Binding => {
            bindings.push(value.clone());
            true
        }
        MatchPattern::List { items, rest } => {
            let Value::List(values) = value else {
                return false;
            };
            if values.len() < items.len() || (!rest && values.len() != items.len()) {
                return false;
            }
            let binding_start = bindings.len();
            for (item, value) in items.iter().zip(values.iter()) {
                if !matches_pattern(item, value, bindings) {
                    bindings.truncate(binding_start);
                    return false;
                }
            }
            if *rest {
                bindings.push(Value::List(Rc::new(values[items.len()..].to_vec())));
            }
            true
        }
        MatchPattern::Map {
            entries: patterns,
            rest,
            exact,
        } => {
            let Value::Map(entries) = value else {
                return false;
            };
            if *exact && entries.len() != patterns.len() {
                return false;
            }
            let binding_start = bindings.len();
            for (key, pattern) in patterns {
                let key = Value::string(key.clone());
                let Some((_, value)) = entries.iter().rev().find(|(entry, _)| entry == &key) else {
                    bindings.truncate(binding_start);
                    return false;
                };
                if !matches_pattern(pattern, value, bindings) {
                    bindings.truncate(binding_start);
                    return false;
                }
            }
            if *rest {
                let rest_entries = entries
                    .iter()
                    .filter(|(key, _)| {
                        !patterns
                            .iter()
                            .any(|(pattern_key, _)| key == &Value::string(pattern_key.clone()))
                    })
                    .cloned()
                    .collect();
                bindings.push(Value::Map(Rc::new(rest_entries)));
            }
            true
        }
    }
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
        value => Err(format!("cannot index {}", value.type_name())),
    }
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
