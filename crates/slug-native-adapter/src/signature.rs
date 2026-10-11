//! Reads the canonical signature text a shim table carries. The adapter takes
//! its cell layout from this text, so it needs no manifest model at run time.

use alloc::{string::String, vec::Vec};

use crate::abi::SHIM_ABI_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Direction {
    In,
    Out,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CellType {
    Bool,
    Signed {
        min: i64,
        max: i64,
        name: &'static str,
    },
    Unsigned {
        max: u64,
        name: &'static str,
    },
    F32,
    F64,
    Str,
    Bytes,
    Cstr,
    Handle(String),
}

pub(crate) struct Parsed {
    pub module: String,
    pub name: String,
    pub parameters: Vec<(Direction, CellType)>,
    pub result: Option<CellType>,
}

impl Parsed {
    pub(crate) fn argument_cells(&self) -> usize {
        self.parameters
            .iter()
            .filter(|(direction, _)| *direction != Direction::Out)
            .count()
    }

    pub(crate) fn result_cells(&self) -> usize {
        usize::from(self.result.is_some())
            + self
                .parameters
                .iter()
                .filter(|(direction, _)| *direction == Direction::Out)
                .count()
    }
}

/// Parses `slug-shim/<abi>;<module>;<function>;<symbol>;(<p>,...)-><result>[;macro]`.
pub(crate) fn parse(text: &str) -> Result<Parsed, String> {
    let mut fields = text.split(';');
    let prefix = fields.next().unwrap_or_default();
    if prefix != alloc::format!("slug-shim/{SHIM_ABI_VERSION}") {
        return Err(alloc::format!("unsupported signature prefix `{prefix}`"));
    }
    let (Some(module), Some(name), Some(_symbol), Some(shape)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err("signature has too few fields".into());
    };
    match (fields.next(), fields.next()) {
        (None | Some("macro"), None) => {}
        _ => return Err("signature has unexpected trailing fields".into()),
    }
    let (parameters, result) = shape
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(")->"))
        .ok_or("signature has no `(...)->` shape")?;
    let parameters = if parameters.is_empty() {
        Vec::new()
    } else {
        parameters
            .split(',')
            .map(parameter)
            .collect::<Result<Vec<_>, _>>()?
    };
    let result = match result {
        "void" => None,
        text => Some(cell_type(text)?),
    };
    if matches!(result, Some(CellType::Str | CellType::Bytes)) {
        return Err("`str` and `bytes` are arguments only".into());
    }
    Ok(Parsed {
        module: module.into(),
        name: name.into(),
        parameters,
        result,
    })
}

fn parameter(text: &str) -> Result<(Direction, CellType), String> {
    let (direction, text) = if let Some(rest) = text.strip_prefix("out ") {
        (Direction::Out, rest)
    } else if let Some(rest) = text.strip_prefix("close ") {
        (Direction::Close, rest)
    } else {
        (Direction::In, text)
    };
    let cell_type = cell_type(text)?;
    match (direction, &cell_type) {
        (Direction::Close, CellType::Handle(_)) | (Direction::In, _) => Ok((direction, cell_type)),
        (Direction::Out, CellType::Str | CellType::Bytes) | (Direction::Close, _) => {
            Err(alloc::format!("invalid parameter `{text}`"))
        }
        (Direction::Out, _) => Ok((direction, cell_type)),
    }
}

fn cell_type(text: &str) -> Result<CellType, String> {
    Ok(match text {
        "bool" => CellType::Bool,
        "i8" => signed::<i8>("i8"),
        "i16" => signed::<i16>("i16"),
        "i32" => signed::<i32>("i32"),
        "i64" => signed::<i64>("i64"),
        "u8" => unsigned::<u8>("u8"),
        "u16" => unsigned::<u16>("u16"),
        "u32" => unsigned::<u32>("u32"),
        "u64" => unsigned::<u64>("u64"),
        "f32" => CellType::F32,
        "f64" => CellType::F64,
        "str" => CellType::Str,
        "bytes" => CellType::Bytes,
        "cstr" => CellType::Cstr,
        _ => {
            if let Some(free) = text.strip_prefix("cstr owned(") {
                if free.strip_suffix(')').is_some_and(is_identifier) {
                    return Ok(CellType::Cstr);
                }
            } else if let Some((resource, c_type)) = text
                .strip_prefix("handle<")
                .and_then(|rest| rest.strip_suffix('>'))
                .and_then(|rest| rest.split_once(':'))
                && is_identifier(resource)
                && is_identifier(c_type)
            {
                return Ok(CellType::Handle(resource.into()));
            }
            return Err(alloc::format!("unknown type `{text}`"));
        }
    })
}

fn signed<T: Into<i64> + Bounded>(name: &'static str) -> CellType {
    CellType::Signed {
        min: T::MIN_VALUE.into(),
        max: T::MAX_VALUE.into(),
        name,
    }
}

fn unsigned<T: Into<u64> + Bounded>(name: &'static str) -> CellType {
    CellType::Unsigned {
        max: T::MAX_VALUE.into(),
        name,
    }
}

trait Bounded: Sized {
    const MIN_VALUE: Self;
    const MAX_VALUE: Self;
}

macro_rules! bounded {
    ($($type:ty),*) => {
        $(impl Bounded for $type {
            const MIN_VALUE: Self = <$type>::MIN;
            const MAX_VALUE: Self = <$type>::MAX;
        })*
    };
}

bounded!(i8, i16, i32, i64, u8, u16, u32, u64);

fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
