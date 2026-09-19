use std::sync::Arc;

use crate::{
    Value,
    source::environment::{CallableIdentity, ForeignResourceSignature},
};

/// A source position attached to an instruction for language diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceSpan {
    pub path: Arc<str>,
    pub line: u32,
    pub column: u32,
}

impl SourceSpan {
    #[must_use]
    pub fn new(path: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            path: Arc::from(path.into()),
            line,
            column,
        }
    }
}

/// Private source-path table index used by bytecode span metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(pub(crate) u32);

impl SourceId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Private source-span table index used by bytecode instructions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpanId(pub(crate) u32);

impl SpanId {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

macro_rules! metadata_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name(pub(crate) u32);

        impl $name {
            #[must_use]
            pub const fn new(index: u32) -> Self {
                Self(index)
            }

            pub(crate) fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

metadata_id!(GlobalNameId);
metadata_id!(CaptureListId);
metadata_id!(SchemaFieldsId);
metadata_id!(StructFieldsId);
metadata_id!(MatchPatternId);
metadata_id!(InterpolationId);
metadata_id!(ListSpreadId);
metadata_id!(CallArgumentsId);
metadata_id!(SelectedCallId);
metadata_id!(SelectCasesId);

/// A literal embedded in a bytecode chunk.
#[derive(Clone, Debug)]
pub enum Constant {
    Value(Value),
    Function(usize),
}

/// The enclosing slot from which a closure captures a value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Capture {
    Local(usize),
    Capture(usize),
}

/// The subset of source patterns lowered by the current compiler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchRest {
    None,
    Discard,
    Binding,
}

#[derive(Clone, Debug)]
pub enum MatchMapKey {
    String(String),
    Operand(usize),
}

#[derive(Clone, Debug)]
pub struct SchemaField {
    pub name: String,
    pub has_default: bool,
}

/// Private callable metadata used by source-call binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterSignature {
    pub name: String,
    pub has_default: bool,
    pub variadic: bool,
}

/// Retained evaluated tag metadata for a top-level source declaration.
#[derive(Clone, Debug)]
pub struct ModuleTag {
    pub name: String,
    pub arguments: Vec<Value>,
}

/// Retained source metadata for a top-level declaration.
#[derive(Clone, Debug)]
pub struct ModuleDeclaration {
    pub bindings: Vec<String>,
    pub mutable: bool,
    pub exported: bool,
    /// Whether this declaration must be supplied by the module-qualified host registry.
    pub foreign: bool,
    /// The inclusive declared call-arity range for a foreign binding. `None`
    /// denotes a variadic declaration.
    pub foreign_arity: Option<(usize, Option<usize>)>,
    /// The source-level nominal resource type declared by this metadata entry.
    pub resource_type: Option<String>,
    /// Private canonical callable identity for a resolved foreign binding.
    pub(crate) foreign_callable_identity: Option<CallableIdentity>,
    /// Resource positions that require validation when invoking this foreign binding.
    pub(crate) foreign_resource_signature: Option<ForeignResourceSignature>,
    pub documentation: Option<String>,
    pub tags: Vec<ModuleTag>,
}

/// The subset of source patterns lowered by the current compiler.
#[derive(Clone, Debug)]
pub enum MatchPattern {
    Literal(Value),
    /// A qualified enum case evaluated before attempting the pattern.
    Enum(usize),
    Wildcard,
    Binding,
    Pinned(usize),
    At(Box<MatchPattern>),
    Alternatives(Vec<MatchPattern>),
    List {
        items: Vec<MatchPattern>,
        rest: MatchRest,
    },
    Map {
        entries: Vec<(MatchMapKey, MatchPattern)>,
        rest: MatchRest,
        exact: bool,
    },
    Constrained {
        pattern: Box<MatchPattern>,
        constraint: MatchType,
    },
}

/// Private runtime-checkable type form used by source match patterns.
#[derive(Clone, Debug)]
pub enum MatchType {
    Any,
    Nil,
    Bool,
    Num,
    Str,
    Bytes,
    Resource { module: String, name: String },
    Enum { module: String, name: String },
    List(Option<Box<MatchType>>),
    Map(Option<(Box<MatchType>, Box<MatchType>)>),
    Function,
    Task,
    Channel,
    Schema,
    Struct(Option<usize>),
    Union(Vec<MatchType>),
}

/// One source-order case consumed by the private select instruction.
#[derive(Clone, Debug)]
pub enum SelectCase {
    Receive { has_handler: bool },
    Send { has_handler: bool },
    After { has_handler: bool },
    Await { has_handler: bool },
    Default { has_handler: bool },
}
