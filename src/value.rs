use std::{cell::RefCell, collections::HashMap, fmt, fmt::Write as _, rc::Rc};

/// Host functions installed deliberately through the VM API.
pub type NativeFunction = fn(&[Value]) -> Result<Value, String>;

/// Shared storage for a lexical binding captured by one or more closures.
pub(crate) type BindingCell = Rc<RefCell<Value>>;

pub(crate) fn binding_cell(value: Value) -> BindingCell {
    Rc::new(RefCell::new(value))
}

pub(crate) fn module_binding(name: impl Into<Rc<str>>) -> Value {
    let name = name.into();
    Value::Binding {
        name,
        cell: binding_cell(Value::Uninitialized),
    }
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub(crate) chunk: usize,
    pub(crate) captures: Vec<BindingCell>,
    pub(crate) program: Option<Rc<crate::Program>>,
    pub(crate) globals: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub(crate) name: Rc<str>,
    pub(crate) default: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct StructSchema {
    pub(crate) fields: Vec<StructField>,
}

#[derive(Clone, Debug)]
pub struct StructValue {
    pub(crate) schema: Rc<StructSchema>,
    pub(crate) values: Vec<Value>,
}

/// The dynamic values used by the initial Slug VM core.
///
/// Collections are reference-counted so closures and later concurrency support
/// can share them without requiring a copying garbage collector.
#[derive(Clone)]
pub enum Value {
    /// Internal marker for a predeclared module binding that has not run yet.
    Uninitialized,
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
    List(Rc<Vec<Value>>),
    Map(Rc<Vec<(Value, Value)>>),
    StructSchema(Rc<StructSchema>),
    Struct(Rc<StructValue>),
    Closure(Rc<Closure>),
    Native {
        name: Rc<str>,
        function: NativeFunction,
    },
    /// A live module binding exposed through an import map.
    Binding {
        name: Rc<str>,
        cell: BindingCell,
    },
}

impl Value {
    #[must_use]
    pub fn string(value: impl Into<Rc<str>>) -> Self {
        Self::Str(value.into())
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    #[must_use]
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Uninitialized | Self::Binding { .. } => "binding",
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::Float(_) => "num",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::StructSchema(_) => "struct schema",
            Self::Struct(_) => "struct",
            Self::Closure(_) | Self::Native { .. } => "fn",
        }
    }

    pub(crate) fn resolve(&self) -> Result<Value, String> {
        let Self::Binding { name, cell } = self else {
            return Ok(self.clone());
        };
        let value = cell.borrow().clone();
        if matches!(value, Self::Uninitialized) {
            Err(format!("binding `{name}` is not initialized"))
        } else {
            Ok(value)
        }
    }

    pub(crate) fn replace_binding(&self, value: Value) -> bool {
        let Self::Binding { cell, .. } = self else {
            return false;
        };
        *cell.borrow_mut() = value;
        true
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Binding { cell, .. }, value) => cell.borrow().eq(value),
            (value, Self::Binding { cell, .. }) => value.eq(&cell.borrow()),
            (Self::Uninitialized, Self::Uninitialized) | (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) | (Self::Float(b), Self::Int(a)) => {
                int_as_float(*a) == *b
            }
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::StructSchema(a), Self::StructSchema(b)) => Rc::ptr_eq(a, b),
            (Self::Struct(a), Self::Struct(b)) => {
                Rc::ptr_eq(&a.schema, &b.schema) && a.values == b.values
            }
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
            Self::Uninitialized => write!(f, "<uninitialized>"),
            Self::Nil => write!(f, "nil"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bytes(value) => write!(f, "0x\"{}\"", hex(value)),
            Self::List(values) => f.debug_list().entries(values.iter()).finish(),
            Self::Map(entries) => f
                .debug_map()
                .entries(entries.iter().map(|(key, value)| (key, value)))
                .finish(),
            Self::StructSchema(_) => write!(f, "<struct schema>"),
            Self::Struct(value) => {
                write!(f, "struct ")?;
                f.debug_map()
                    .entries(
                        value
                            .schema
                            .fields
                            .iter()
                            .zip(&value.values)
                            .map(|(field, value)| (&field.name, value)),
                    )
                    .finish()
            }
            Self::Closure(_) => write!(f, "<fn>"),
            Self::Native { name, .. } => write!(f, "<native {name}>"),
            Self::Binding { name, .. } => write!(f, "<binding {name}>"),
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
