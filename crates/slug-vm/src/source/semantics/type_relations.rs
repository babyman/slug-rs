//! Pure type relations shared by expression and pattern checking.

use super::super::semantic::Type;

pub(super) fn is_map_key_type(value: &Type) -> bool {
    match value {
        Type::Bool | Type::Num | Type::Str | Type::Bytes => true,
        Type::Union(members) => members.iter().all(is_map_key_type),
        _ => false,
    }
}

pub(super) fn is_dynamic_operation_type(value_type: &Type) -> bool {
    match value_type {
        Type::Unknown | Type::Any => true,
        Type::Union(members) => members.iter().any(is_dynamic_operation_type),
        _ => false,
    }
}

pub(super) fn type_intersection(left: &Type, right: &Type) -> Option<Type> {
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

pub(super) fn union_intersections(members: &[Type], other: &Type) -> Option<Type> {
    let intersections = members
        .iter()
        .filter_map(|member| type_intersection(member, other))
        .collect::<Vec<_>>();
    (!intersections.is_empty()).then(|| Type::union(intersections))
}

pub(super) fn type_subtract(left: &Type, right: &Type) -> Option<Type> {
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
