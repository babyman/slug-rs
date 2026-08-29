use std::fmt;

use crate::SourceSpan;

use super::{SourceError, ast::TypeAnnotation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Type {
    Unknown,
    Any,
    Nil,
    Bool,
    Num,
    Str,
    Bytes,
    List(Option<Box<Type>>),
    Map(Option<(Box<Type>, Box<Type>)>),
    Function(Option<Vec<Type>>),
    Task(Option<Box<Type>>),
    Channel(Option<Box<Type>>),
    Struct(Option<String>),
    Tuple(Vec<Type>),
    Generic(usize),
    Union(Vec<Type>),
}

impl Type {
    pub(super) fn is_reifiable_match_constraint(&self) -> bool {
        match self {
            Self::Any
            | Self::Nil
            | Self::Bool
            | Self::Num
            | Self::Str
            | Self::Bytes
            | Self::Function(None)
            | Self::Task(None)
            | Self::Channel(None)
            | Self::Struct(_) => true,
            Self::List(element) => element
                .as_deref()
                .is_none_or(Self::is_reifiable_match_constraint),
            Self::Map(entries) => entries.as_ref().is_none_or(|(key, value)| {
                key.is_reifiable_match_constraint() && value.is_reifiable_match_constraint()
            }),
            Self::Union(members) => members.iter().all(Self::is_reifiable_match_constraint),
            Self::Unknown
            | Self::Function(Some(_))
            | Self::Task(Some(_))
            | Self::Channel(Some(_))
            | Self::Tuple(_)
            | Self::Generic(_) => false,
        }
    }
    pub(super) fn universal() -> Self {
        Self::Union(vec![Self::Any, Self::Nil])
    }

    pub(super) fn union(members: impl IntoIterator<Item = Self>) -> Self {
        let mut flattened = Vec::new();
        for member in members {
            match member {
                Self::Union(nested) => flattened.extend(nested),
                member => flattened.push(member),
            }
        }
        if flattened.iter().any(|member| member == &Self::Unknown) {
            return Self::Unknown;
        }
        if flattened.iter().any(|member| member == &Self::Any) {
            flattened.retain(|member| matches!(member, Self::Any | Self::Nil));
        }
        flattened.sort_by_key(sort_key);
        flattened.dedup();
        match flattened.len() {
            0 => Self::Unknown,
            1 => flattened.pop().expect("one normalized union member"),
            _ => Self::Union(flattened),
        }
    }

    pub(super) fn includes_nil(&self) -> bool {
        match self {
            Self::Unknown | Self::Nil => true,
            Self::Union(members) => members.iter().any(Self::includes_nil),
            _ => false,
        }
    }

    pub(super) fn widen_unknown(self) -> Self {
        match self {
            Self::Unknown => Self::universal(),
            Self::List(element) => {
                Self::List(element.map(|element| Box::new(element.widen_unknown())))
            }
            Self::Map(entries) => Self::Map(entries.map(|(key, value)| {
                (
                    Box::new(key.widen_unknown()),
                    Box::new(value.widen_unknown()),
                )
            })),
            Self::Function(signature) => Self::Function(
                signature.map(|types| types.into_iter().map(Self::widen_unknown).collect()),
            ),
            Self::Task(result) => Self::Task(result.map(|result| Box::new(result.widen_unknown()))),
            Self::Channel(value) => {
                Self::Channel(value.map(|value| Box::new(value.widen_unknown())))
            }
            Self::Tuple(elements) => {
                Self::Tuple(elements.into_iter().map(Self::widen_unknown).collect())
            }
            Self::Union(members) => Self::union(members.into_iter().map(Self::widen_unknown)),
            other => other,
        }
    }

    pub(super) fn is_assignable_to(&self, expected: &Self) -> bool {
        if self == expected || matches!(self, Self::Unknown) {
            return true;
        }
        if let Self::Union(members) = self {
            return members
                .iter()
                .all(|member| member.is_assignable_to(expected));
        }
        match expected {
            Self::Unknown => true,
            Self::Any => !self.includes_nil(),
            Self::Union(members) => members.iter().any(|member| self.is_assignable_to(member)),
            Self::List(expected) => match self {
                Self::List(actual) => {
                    optional_argument_assignable(actual.as_deref(), expected.as_deref())
                }
                _ => false,
            },
            Self::Map(expected) => match self {
                Self::Map(actual) => optional_pair_assignable(actual.as_ref(), expected.as_ref()),
                _ => false,
            },
            Self::Function(expected) => match self {
                Self::Function(actual) => {
                    optional_signature_assignable(actual.as_deref(), expected.as_deref())
                }
                _ => false,
            },
            Self::Task(expected) => match self {
                Self::Task(actual) => {
                    optional_argument_assignable(actual.as_deref(), expected.as_deref())
                }
                _ => false,
            },
            Self::Channel(expected) => match self {
                Self::Channel(actual) => {
                    optional_argument_assignable(actual.as_deref(), expected.as_deref())
                }
                _ => false,
            },
            Self::Struct(expected) => match self {
                Self::Struct(actual) => expected.is_none() || actual == expected,
                _ => false,
            },
            Self::Tuple(expected) => match self {
                Self::Tuple(actual) => {
                    actual.len() == expected.len()
                        && actual
                            .iter()
                            .zip(expected)
                            .all(|(actual, expected)| actual == expected)
                }
                _ => false,
            },
            _ => false,
        }
    }
}

fn optional_argument_assignable(actual: Option<&Type>, expected: Option<&Type>) -> bool {
    match (actual, expected) {
        (_, None) | (None, Some(_)) => true,
        (Some(actual), Some(expected)) => actual == expected,
    }
}

fn optional_pair_assignable(
    actual: Option<&(Box<Type>, Box<Type>)>,
    expected: Option<&(Box<Type>, Box<Type>)>,
) -> bool {
    match (actual, expected) {
        (_, None) | (None, Some(_)) => true,
        (Some(actual), Some(expected)) => actual == expected,
    }
}

fn optional_signature_assignable(actual: Option<&[Type]>, expected: Option<&[Type]>) -> bool {
    match (actual, expected) {
        (_, None) | (None, Some(_)) => true,
        (Some(actual), Some(expected)) => actual == expected,
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => formatter.write_str("unknown"),
            Self::Any => formatter.write_str("any"),
            Self::Nil => formatter.write_str("nil"),
            Self::Bool => formatter.write_str("bool"),
            Self::Num => formatter.write_str("num"),
            Self::Str => formatter.write_str("str"),
            Self::Bytes => formatter.write_str("bytes"),
            Self::List(argument) => display_application(formatter, "list", argument.as_deref()),
            Self::Map(arguments) => {
                if let Some((key, value)) = arguments {
                    write!(formatter, "map<{key}, {value}>")
                } else {
                    formatter.write_str("map")
                }
            }
            Self::Function(signature) => {
                if let Some(signature) = signature {
                    write!(
                        formatter,
                        "fn<{}>",
                        signature
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    formatter.write_str("fn")
                }
            }
            Self::Task(argument) => display_application(formatter, "task", argument.as_deref()),
            Self::Channel(argument) => display_application(formatter, "chan", argument.as_deref()),
            Self::Struct(name) => {
                if let Some(name) = name {
                    write!(formatter, "struct<{name}>")
                } else {
                    formatter.write_str("struct")
                }
            }
            Self::Tuple(elements) => write!(
                formatter,
                "[{}]",
                elements
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Generic(index) => write!(formatter, "T{index}"),
            Self::Union(members) => write!(
                formatter,
                "{}",
                members
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("|")
            ),
        }
    }
}

fn display_application(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    argument: Option<&Type>,
) -> fmt::Result {
    if let Some(argument) = argument {
        write!(formatter, "{name}<{argument}>")
    } else {
        formatter.write_str(name)
    }
}

fn sort_key(value: &Type) -> (bool, String) {
    (matches!(value, Type::Nil), value.to_string())
}

pub(super) fn resolve_annotation(
    annotation: &TypeAnnotation,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    match annotation {
        TypeAnnotation::Name(name) => resolve_name(name, type_parameters, span),
        TypeAnnotation::Apply { name, arguments } => {
            resolve_application(name, arguments, type_parameters, span)
        }
        TypeAnnotation::Tuple(elements) => elements
            .iter()
            .map(|element| resolve_annotation(element, type_parameters, span))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::Tuple),
        TypeAnnotation::Union(members) => members
            .iter()
            .map(|member| resolve_annotation(member, type_parameters, span))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::union),
    }
}

fn resolve_name(
    name: &str,
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    if let Some(index) = type_parameters
        .iter()
        .position(|parameter| parameter == name)
    {
        return Ok(Type::Generic(index));
    }
    let resolved = match name {
        "any" => Type::Any,
        "nil" => Type::Nil,
        "bool" => Type::Bool,
        "num" => Type::Num,
        "str" => Type::Str,
        "bytes" => Type::Bytes,
        "list" => Type::List(None),
        "map" => Type::Map(None),
        "fn" => Type::Function(None),
        "task" => Type::Task(None),
        "chan" => Type::Channel(None),
        "struct" => Type::Struct(None),
        _ => {
            return Err(SourceError::semantic(
                format!("unknown type `{name}`"),
                span.clone(),
            ));
        }
    };
    Ok(resolved)
}

fn resolve_application(
    name: &str,
    arguments: &[TypeAnnotation],
    type_parameters: &[String],
    span: &SourceSpan,
) -> Result<Type, SourceError> {
    if name == "struct" {
        if arguments.len() != 1 {
            return Err(wrong_type_arity(name, "1", span));
        }
        let TypeAnnotation::Name(name) = &arguments[0] else {
            return Err(SourceError::semantic(
                "struct type argument must be a schema name",
                span.clone(),
            ));
        };
        return Ok(Type::Struct(Some(name.clone())));
    }
    let resolved = arguments
        .iter()
        .map(|argument| resolve_annotation(argument, type_parameters, span))
        .collect::<Result<Vec<_>, _>>()?;
    match (name, resolved.as_slice()) {
        ("list", [element]) => Ok(Type::List(Some(Box::new(element.clone())))),
        ("map", [key, value]) => Ok(Type::Map(Some((
            Box::new(key.clone()),
            Box::new(value.clone()),
        )))),
        ("fn", [_, ..]) => Ok(Type::Function(Some(resolved))),
        ("task", [result]) => Ok(Type::Task(Some(Box::new(result.clone())))),
        ("chan", [value]) => Ok(Type::Channel(Some(Box::new(value.clone())))),
        ("list" | "task" | "chan", _) => Err(wrong_type_arity(name, "1", span)),
        ("map", _) => Err(wrong_type_arity(name, "2", span)),
        ("fn", _) => Err(wrong_type_arity(name, "at least 1", span)),
        _ => Err(SourceError::semantic(
            format!("unknown type constructor `{name}`"),
            span.clone(),
        )),
    }
}

fn wrong_type_arity(name: &str, expected: &str, span: &SourceSpan) -> SourceError {
    SourceError::semantic(
        format!("type `{name}` expects {expected} argument(s)"),
        span.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::Type;

    #[test]
    fn any_excludes_nil_and_any_nil_is_universal() {
        let universal = Type::universal();

        assert!(Type::Str.is_assignable_to(&Type::Any));
        assert!(!Type::Nil.is_assignable_to(&Type::Any));
        assert!(Type::Str.is_assignable_to(&universal));
        assert!(Type::Nil.is_assignable_to(&universal));
    }

    #[test]
    fn unions_are_canonical_and_absorb_redundant_members() {
        assert_eq!(Type::union([Type::Any, Type::Str]), Type::Any);
        assert_eq!(
            Type::union([Type::Str, Type::Nil, Type::Any]),
            Type::universal()
        );
        assert_eq!(
            Type::union([Type::Str, Type::Nil, Type::Str]),
            Type::Union(vec![Type::Str, Type::Nil])
        );
        assert_eq!(Type::union([Type::Unknown, Type::Any]), Type::Unknown);
    }

    #[test]
    fn structured_types_are_reflexive() {
        let list = Type::List(Some(Box::new(Type::Str)));
        let tuple = Type::Tuple(vec![Type::Str, Type::Num]);

        assert!(list.is_assignable_to(&list));
        assert!(tuple.is_assignable_to(&tuple));
        assert!(Type::universal().is_assignable_to(&Type::universal()));
    }
}
