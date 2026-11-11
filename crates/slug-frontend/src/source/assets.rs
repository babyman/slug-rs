//! `asset("name")`: a file a clutch ships, embedded when a module compiles.
//!
//! Before analysis, every `asset` call in a module is checked and each name it
//! uses is read once through the host. The checker then types each call
//! `bytes`, and lowering emits the bytes read as a constant.

use std::collections::HashMap;

use slug_vm::Value;

use super::{
    SourceError, SourceErrorCode,
    ast::{Binary, CallArgument, Expr, ExprKind},
};

/// The bytes of each asset a module names, by name.
pub(super) type Assets = HashMap<String, Value>;

/// Whether `callee` names the `asset` form. Like `import`, it is recognised
/// by name.
pub(super) fn is_asset(callee: &Expr) -> bool {
    matches!(&callee.kind, ExprKind::Name(name) if name == "asset")
}

/// The asset name of a call's arguments: its one positional string literal.
pub(super) fn literal_name(arguments: &[CallArgument]) -> Option<&str> {
    match arguments {
        [
            CallArgument::Positional(Expr {
                kind: ExprKind::Value(Value::Str(name)),
                ..
            }),
        ] => Some(name),
        _ => None,
    }
}

/// Whether `name` is a valid asset name: `/`-separated segments, none empty,
/// `.`, or `..`, without `\` or NUL.
fn valid_name(name: &str) -> bool {
    !name.contains(['\\', '\0'])
        && name
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

/// Checks every `asset` call in `expressions` and reads each name they use
/// with `read`, once.
pub(super) fn load(
    expressions: &[Expr],
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Assets, SourceError> {
    let mut assets = Assets::new();
    for expression in expressions {
        visit(expression, &mut assets, &mut read)?;
    }
    Ok(assets)
}

fn visit(
    expression: &Expr,
    assets: &mut Assets,
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<(), SourceError> {
    super::stack::grow(|| {
        match &expression.kind {
            ExprKind::Call { callee, arguments } if is_asset(callee) => {
                let Some(name) = literal_name(arguments) else {
                    return Err(invalid(
                        "`asset` takes one string literal naming a file in its clutch's assets folder",
                        expression,
                    ));
                };
                if !valid_name(name) {
                    return Err(invalid(
                        format!(
                            "invalid asset name `{name}`: write a path in the assets folder with `/`, without empty, `.`, or `..` parts"
                        ),
                        expression,
                    ));
                }
                if !assets.contains_key(name) {
                    let bytes = read(name).map_err(|message| invalid(message, expression))?;
                    assets.insert(name.into(), Value::Bytes(bytes.into()));
                }
                return Ok(());
            }
            ExprKind::Binary {
                operator: Binary::Pipeline,
                right,
                ..
            } if matches!(&right.kind, ExprKind::Call { callee, .. } if is_asset(callee)) => {
                return Err(invalid(
                    "`asset` takes its name as a string literal argument, not through `/>`",
                    expression,
                ));
            }
            _ => {}
        }
        let mut result = Ok(());
        expression.for_each_child(|child| {
            if result.is_ok() {
                result = visit(child, assets, read);
            }
        });
        result
    })
}

fn invalid(message: impl Into<String>, expression: &Expr) -> SourceError {
    SourceError::semantic(
        SourceErrorCode::InvalidAsset,
        message,
        expression.span.clone(),
    )
    .with_extent(expression)
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn asset_names_are_relative_slash_paths() {
        for name in ["htmx.js", "images/foo.png", "a/b/c.txt", ".hidden"] {
            assert!(valid_name(name), "{name}");
        }
        for name in [
            "", "/htmx.js", "a//b", "a/", "./a", "a/./b", "../a", "a/..", "a\\b", "a\0b",
        ] {
            assert!(!valid_name(name), "{name:?}");
        }
    }
}
