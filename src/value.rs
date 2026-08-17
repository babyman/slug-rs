use std::{fmt, fmt::Write as _, rc::Rc};

/// Host functions installed deliberately through the VM API.
pub type NativeFunction = fn(&[Value]) -> Result<Value, String>;

#[derive(Clone, Debug)]
pub struct Closure {
    pub(crate) chunk: usize,
    pub(crate) captures: Vec<Value>,
}

/// The dynamic values used by the initial Slug VM core.
///
/// Collections are reference-counted so closures and later concurrency support
/// can share them without requiring a copying garbage collector.
#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
    Symbol(Rc<str>),
    List(Rc<Vec<Value>>),
    Map(Rc<Vec<(Value, Value)>>),
    Closure(Rc<Closure>),
    Native {
        name: Rc<str>,
        function: NativeFunction,
    },
}

impl Value {
    #[must_use]
    pub fn string(value: impl Into<Rc<str>>) -> Self {
        Self::Str(value.into())
    }

    #[must_use]
    pub fn symbol(value: impl Into<Rc<str>>) -> Self {
        Self::Symbol(value.into())
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    #[must_use]
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::Float(_) => "num",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::Symbol(_) => "sym",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::Closure(_) | Self::Native { .. } => "fn",
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) | (Self::Float(b), Self::Int(a)) => {
                int_as_float(*a) == *b
            }
            (Self::Str(a), Self::Str(b)) | (Self::Symbol(a), Self::Symbol(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::Closure(a), Self::Closure(b)) => Rc::ptr_eq(a, b),
            (Self::Native { function: a, .. }, Self::Native { function: b, .. }) => {
                std::ptr::fn_addr_eq(*a, *b)
            }
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bytes(value) => write!(f, "0x\"{}\"", hex(value)),
            Self::Symbol(value) => write!(f, ":{value}"),
            Self::List(values) => f.debug_list().entries(values.iter()).finish(),
            Self::Map(entries) => f
                .debug_map()
                .entries(entries.iter().map(|(key, value)| (key, value)))
                .finish(),
            Self::Closure(_) => write!(f, "<fn>"),
            Self::Native { name, .. } => write!(f, "<native {name}>"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(value) => write!(f, "{value}"),
            value => write!(f, "{value:?}"),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing into String cannot fail");
    }
    output
}

#[allow(clippy::cast_precision_loss)]
fn int_as_float(value: i64) -> f64 {
    value as f64
}
