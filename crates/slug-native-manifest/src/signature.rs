//! Binding signatures: `(param, ...) -> result`.

use std::fmt::{self, Write as _};

/// A C-facing type supported by typed bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeType {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    /// Pointer and length. An argument is borrowed for the call; an `out`
    /// result is borrowed or, with `owned`, freed by the named function.
    Str {
        owned: Option<String>,
    },
    /// Pointer and length, with the same rules as `Str`.
    Bytes {
        owned: Option<String>,
    },
    /// NUL-terminated text. In result position, `owned` names the function
    /// that frees it; without one the library keeps ownership.
    Cstr {
        owned: Option<String>,
    },
    /// A pointer to the resource's C type, held by a Slug resource.
    /// `resource` is `R` for the binding's own module, or `module.R` for a
    /// resource another module of the same clutch declares.
    Handle {
        resource: String,
        c_type: String,
    },
}

impl NativeType {
    fn scalar(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "str" => Self::Str { owned: None },
            "bytes" => Self::Bytes { owned: None },
            _ => return None,
        })
    }

    fn write_canonical(&self, output: &mut String) {
        let name = match self {
            Self::Bool => "bool",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Str { owned } | Self::Bytes { owned } | Self::Cstr { owned } => {
                output.push_str(match self {
                    Self::Str { .. } => "str",
                    Self::Bytes { .. } => "bytes",
                    _ => "cstr",
                });
                if let Some(free) = owned {
                    let _ = write!(output, " owned({free})");
                }
                return;
            }
            Self::Handle { resource, c_type } => {
                let _ = write!(output, "handle<{resource}:{c_type}>");
                return;
            }
        };
        output.push_str(name);
    }
}

/// How a parameter crosses the call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// An argument cell.
    In,
    /// A result slot the caller supplies and reads back.
    Out,
    /// The handle argument the call consumes; marks an explicit close.
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameter {
    pub direction: Direction,
    pub native_type: NativeType,
    /// Documentation only; never part of the canonical signature.
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub parameters: Vec<Parameter>,
    /// `None` for `void`.
    pub result: Option<NativeType>,
}

impl Signature {
    /// Number of argument cells: every parameter that is not `out`.
    #[must_use]
    pub fn argument_cells(&self) -> usize {
        self.parameters
            .iter()
            .filter(|parameter| parameter.direction != Direction::Out)
            .count()
    }

    /// Number of result cells: the C result, if any, then each `out` value.
    #[must_use]
    pub fn result_cells(&self) -> usize {
        usize::from(self.result.is_some())
            + self
                .parameters
                .iter()
                .filter(|parameter| parameter.direction == Direction::Out)
                .count()
    }

    pub(crate) fn write_canonical(&self, output: &mut String) {
        output.push('(');
        for (index, parameter) in self.parameters.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            match parameter.direction {
                Direction::In => {}
                Direction::Out => output.push_str("out "),
                Direction::Close => output.push_str("close "),
            }
            parameter.native_type.write_canonical(output);
        }
        output.push_str(")->");
        match &self.result {
            Some(result) => result.write_canonical(output),
            None => output.push_str("void"),
        }
    }
}

/// A signature as written, before handle resources are resolved.
pub(crate) struct ParsedSignature {
    pub parameters: Vec<ParsedParameter>,
    pub result: Option<ParsedType>,
}

pub(crate) struct ParsedParameter {
    pub direction: Direction,
    pub native_type: ParsedType,
    pub name: Option<String>,
}

pub(crate) enum ParsedType {
    Plain(NativeType),
    Cstr { owned: Option<String> },
    Handle { resource: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token<'a> {
    Identifier(&'a str),
    Symbol(&'static str),
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identifier(text) | Self::Symbol(text) => write!(formatter, "`{text}`"),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token<'_>>, String> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(next) = rest.chars().next() {
        if next.is_ascii_whitespace() {
            rest = &rest[1..];
        } else if let Some(stripped) = rest.strip_prefix("->") {
            tokens.push(Token::Symbol("->"));
            rest = stripped;
        } else if let Some(symbol) = ["(", ")", ",", "<", ">", "."]
            .into_iter()
            .find(|symbol| rest.starts_with(symbol))
        {
            tokens.push(Token::Symbol(symbol));
            rest = &rest[1..];
        } else if next == '_' || next.is_ascii_alphabetic() {
            let end = rest
                .find(|character: char| character != '_' && !character.is_ascii_alphanumeric())
                .unwrap_or(rest.len());
            tokens.push(Token::Identifier(&rest[..end]));
            rest = &rest[end..];
        } else {
            return Err(format!("unexpected character `{next}`"));
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    position: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.position)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token<'a>> {
        self.tokens.get(self.position + offset)
    }

    fn next(&mut self) -> Option<Token<'a>> {
        let token = self.tokens.get(self.position).cloned();
        self.position += usize::from(token.is_some());
        token
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<(), String> {
        match self.next() {
            Some(Token::Symbol(found)) if found == symbol => Ok(()),
            Some(found) => Err(format!("expected `{symbol}`, found {found}")),
            None => Err(format!("expected `{symbol}` at end of signature")),
        }
    }

    fn identifier(&mut self, what: &str) -> Result<&'a str, String> {
        match self.next() {
            Some(Token::Identifier(name)) => Ok(name),
            Some(found) => Err(format!("expected {what}, found {found}")),
            None => Err(format!("expected {what} at end of signature")),
        }
    }

    fn signature(&mut self) -> Result<ParsedSignature, String> {
        self.expect_symbol("(")?;
        let mut parameters = Vec::new();
        if self.peek() != Some(&Token::Symbol(")")) {
            loop {
                parameters.push(self.parameter()?);
                if self.peek() == Some(&Token::Symbol(",")) {
                    self.next();
                } else {
                    break;
                }
            }
        }
        self.expect_symbol(")")?;
        self.expect_symbol("->")?;
        let result = if self.peek() == Some(&Token::Identifier("void")) {
            self.next();
            None
        } else {
            Some(self.native_type()?)
        };
        if let Some(extra) = self.next() {
            return Err(format!("unexpected {extra} after result type"));
        }
        Ok(ParsedSignature { parameters, result })
    }

    fn parameter(&mut self) -> Result<ParsedParameter, String> {
        let direction = match (self.peek(), self.peek_at(1)) {
            (Some(Token::Identifier("out")), Some(Token::Identifier(_))) => Direction::Out,
            (Some(Token::Identifier("close")), Some(Token::Identifier(_))) => Direction::Close,
            _ => Direction::In,
        };
        if direction != Direction::In {
            self.next();
        }
        let native_type = self.native_type()?;
        let name = match self.peek() {
            Some(Token::Identifier(name)) => {
                let name = (*name).to_owned();
                self.next();
                Some(name)
            }
            _ => None,
        };
        Ok(ParsedParameter {
            direction,
            native_type,
            name,
        })
    }

    fn native_type(&mut self) -> Result<ParsedType, String> {
        let name = self.identifier("a type")?;
        let parsed = match name {
            "cstr" => ParsedType::Cstr { owned: None },
            "handle" => {
                self.expect_symbol("<")?;
                let mut resource = self.identifier("a resource name")?.to_owned();
                while self.peek() == Some(&Token::Symbol(".")) {
                    self.next();
                    resource.push('.');
                    resource.push_str(self.identifier("a resource name")?);
                }
                self.expect_symbol(">")?;
                ParsedType::Handle { resource }
            }
            "void" => return Err("`void` is only valid as a result".into()),
            _ => ParsedType::Plain(
                NativeType::scalar(name).ok_or_else(|| format!("unknown type `{name}`"))?,
            ),
        };
        if self.peek() == Some(&Token::Identifier("owned"))
            && self.peek_at(1) == Some(&Token::Symbol("("))
        {
            self.next();
            self.next();
            let free = self.identifier("a free function")?.to_owned();
            self.expect_symbol(")")?;
            return match parsed {
                ParsedType::Cstr { .. } => Ok(ParsedType::Cstr { owned: Some(free) }),
                ParsedType::Plain(NativeType::Str { .. }) => {
                    Ok(ParsedType::Plain(NativeType::Str { owned: Some(free) }))
                }
                ParsedType::Plain(NativeType::Bytes { .. }) => {
                    Ok(ParsedType::Plain(NativeType::Bytes { owned: Some(free) }))
                }
                _ => Err(format!(
                    "only `cstr`, `str`, and `bytes` can be `owned`, not `{name}`"
                )),
            };
        }
        Ok(parsed)
    }
}

pub(crate) fn parse(text: &str) -> Result<ParsedSignature, String> {
    let tokens = tokenize(text)?;
    Parser {
        tokens,
        position: 0,
    }
    .signature()
}
