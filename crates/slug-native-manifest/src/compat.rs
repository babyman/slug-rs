//! Slug/C type compatibility between `foreign` declarations and typed
//! bindings. Hosts run it where they compile a clutch's module source: the
//! desktop loader at activation and static builds before linking.

use std::collections::BTreeSet;

use slug_loader::ForeignDeclaration;

use crate::{Binding, Direction, NativeManifest, NativeType};

/// Checks a module's `foreign` declarations against its typed bindings, by
/// the compatibility table in the typed native manifest bindings record.
/// Returns the bindings whose text result is annotated `str` without `nil`,
/// for which a NULL result must fail.
///
/// # Errors
///
/// Returns a message naming the first uncovered binding, unbound declaration,
/// or incompatible annotation.
pub fn check_foreign_declarations(
    manifest: &NativeManifest,
    module: &str,
    declarations: &[ForeignDeclaration],
) -> Result<Vec<String>, String> {
    let bindings = manifest
        .functions
        .iter()
        .filter(|binding| binding.module == module)
        .collect::<Vec<_>>();
    if let Some(binding) = bindings
        .iter()
        .find(|binding| !declarations.iter().any(|item| item.name == binding.name))
    {
        return Err(format!(
            "native binding `{module}.{}` has no foreign declaration",
            binding.name
        ));
    }
    let mut non_null_text = Vec::new();
    for declaration in declarations {
        let binding = bindings
            .iter()
            .find(|binding| binding.name == declaration.name)
            .ok_or_else(|| {
                format!(
                    "foreign `{module}.{}` has no native binding in the clutch manifest",
                    declaration.name
                )
            })?;
        check_declaration(module, binding, declaration)?;
        if matches!(binding.signature.result, Some(NativeType::Cstr { .. }))
            && declaration.result.as_deref() == Some("str")
        {
            non_null_text.push(declaration.name.clone());
        }
    }
    Ok(non_null_text)
}

fn check_declaration(
    module: &str,
    binding: &Binding,
    declaration: &ForeignDeclaration,
) -> Result<(), String> {
    let name = format!("{module}.{}", declaration.name);
    let arguments = binding
        .signature
        .parameters
        .iter()
        .filter(|parameter| parameter.direction != Direction::Out)
        .collect::<Vec<_>>();
    if declaration.parameters.len() != arguments.len() {
        return Err(format!(
            "foreign `{name}` declares {} parameters, but its native binding takes {}",
            declaration.parameters.len(),
            arguments.len()
        ));
    }
    for (parameter, native) in declaration.parameters.iter().zip(arguments) {
        if parameter.variadic || parameter.has_default {
            return Err(format!(
                "foreign `{name}` parameter `{}` cannot be variadic or have a default",
                parameter.name
            ));
        }
        let expected = argument_annotation(&native.native_type);
        compare(
            &format!("foreign `{name}` parameter `{}`", parameter.name),
            parameter.annotation.as_deref(),
            &[expected.as_str()],
        )?;
    }
    let has_out = binding
        .signature
        .parameters
        .iter()
        .any(|parameter| parameter.direction == Direction::Out);
    let accepted = if has_out {
        vec!["list".to_owned()]
    } else {
        match &binding.signature.result {
            None => vec!["nil".to_owned()],
            Some(NativeType::Cstr { .. }) => vec!["str".to_owned(), "nil|str".to_owned()],
            Some(NativeType::Handle { resource, .. }) => vec![union(&[resource, "nil"])],
            Some(other) => vec![argument_annotation(other)],
        }
    };
    compare(
        &format!("foreign `{name}` result"),
        declaration.result.as_deref(),
        &accepted.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn argument_annotation(native_type: &NativeType) -> String {
    match native_type {
        NativeType::Bool => "bool".into(),
        NativeType::Str { .. } | NativeType::Cstr { .. } => "str".into(),
        NativeType::Bytes { .. } => "bytes".into(),
        NativeType::Handle { resource, .. } => resource.clone(),
        _ => "num".into(),
    }
}

/// Spells a union the way the frontend renders it: members sorted.
fn union(members: &[&str]) -> String {
    members
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|")
}

fn compare(what: &str, annotation: Option<&str>, accepted: &[&str]) -> Result<(), String> {
    let Some(annotation) = annotation else {
        return Err(format!(
            "{what} must be annotated `{}` for its native binding",
            accepted[0]
        ));
    };
    if accepted
        .iter()
        .any(|expected| annotation_matches(annotation, expected))
    {
        return Ok(());
    }
    Err(format!(
        "{what} is annotated `{annotation}`, but its native binding needs `{}`",
        accepted.join("` or `")
    ))
}

/// Compares union annotations member by member, in any order. A qualified
/// resource member, `module.R`, matches any import-qualified spelling of `R`,
/// such as `alias.R`; the VM checks the exact resource identity at run time.
fn annotation_matches(annotation: &str, expected: &str) -> bool {
    let mut members = annotation.split('|').collect::<Vec<_>>();
    let wanted = expected.split('|').collect::<Vec<_>>();
    if members.len() != wanted.len() {
        return false;
    }
    wanted.iter().all(|wanted| {
        let position = members
            .iter()
            .position(|member| match wanted.rsplit_once('.') {
                Some((_, resource)) => member
                    .rsplit_once('.')
                    .is_some_and(|(_, name)| name == resource),
                None => member == wanted,
            });
        position.map(|index| members.swap_remove(index)).is_some()
    })
}
