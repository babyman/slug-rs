use slug_vm::{DeferMode, SourceSpan, Value};

use super::SourcePosition;

#[derive(Clone, Debug)]
pub(super) struct Expr {
    pub(super) kind: ExprKind,
    pub(super) span: SourceSpan,
    /// Exclusive end of the expression's source.
    pub(super) end: SourcePosition,
}

impl Expr {
    /// The leftmost source position covered by this expression, including
    /// nested expressions that begin before its own focus span, such as the
    /// left operand of a binary operator.
    pub(super) fn extent_start(&self) -> SourcePosition {
        let mut start = SourcePosition {
            line: self.span.line,
            column: self.span.column,
        };
        self.for_each_child(|child| {
            let child = child.extent_start();
            if (child.line, child.column) < (start.line, start.column) {
                start = child;
            }
        });
        start
    }

    /// Calls `visit` with each expression directly nested in this one.
    #[allow(clippy::too_many_lines)]
    pub(super) fn for_each_child<'a>(&'a self, mut visit: impl FnMut(&'a Expr)) {
        match &self.kind {
            ExprKind::Call { callee, arguments } => {
                visit(callee);
                for argument in arguments {
                    let value = match argument {
                        CallArgument::Positional(value)
                        | CallArgument::Named { value, .. }
                        | CallArgument::Spread(value) => value,
                    };
                    visit(value);
                }
            }
            ExprKind::Declare { value, tags, .. } => {
                for tag in tags {
                    for argument in &tag.arguments {
                        visit(argument);
                    }
                }
                visit(value);
            }
            ExprKind::Foreign {
                signature, tags, ..
            } => {
                for tag in tags {
                    for argument in &tag.arguments {
                        visit(argument);
                    }
                }
                for parameter in &signature.parameters {
                    if let Some(default) = &parameter.default {
                        visit(default);
                    }
                }
            }
            ExprKind::Function {
                parameters, body, ..
            } => {
                for parameter in parameters {
                    if let Some(default) = &parameter.default {
                        visit(default);
                    }
                }
                visit(body);
            }
            ExprKind::Assign { value, .. }
            | ExprKind::Return { value }
            | ExprKind::Throw { value }
            | ExprKind::Defer { value, .. }
            | ExprKind::Spawn(value)
            | ExprKind::Prefix { value, .. }
            | ExprKind::TypeApply { callee: value, .. } => visit(value),
            ExprKind::Recur(arguments) => {
                for argument in arguments {
                    let value = match argument {
                        CallArgument::Positional(value)
                        | CallArgument::Named { value, .. }
                        | CallArgument::Spread(value) => value,
                    };
                    visit(value);
                }
            }
            ExprKind::Nursery { limit, body } => {
                if let Some(limit) = limit {
                    visit(limit);
                }
                visit(body);
            }
            ExprKind::Select(cases) => {
                for case in cases {
                    match &case.kind {
                        SelectCaseKind::Receive(value)
                        | SelectCaseKind::After(value)
                        | SelectCaseKind::Await(value) => visit(value),
                        SelectCaseKind::Send { channel, value } => {
                            visit(channel);
                            visit(value);
                        }
                        SelectCaseKind::Default => {}
                    }
                    if let Some(handler) = &case.handler {
                        visit(handler);
                    }
                }
            }
            ExprKind::Match { subject, cases } => {
                if let Some(subject) = subject {
                    visit(subject);
                }
                for case in cases {
                    if let Some(guard) = &case.guard {
                        visit(guard);
                    }
                    visit(&case.value);
                }
            }
            ExprKind::Binary { left, right, .. } => {
                visit(left);
                visit(right);
            }
            ExprKind::Block(values) => {
                for value in values {
                    visit(value);
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit(condition);
                visit(then_branch);
                if let Some(else_branch) = else_branch {
                    visit(else_branch);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    let value = match value {
                        ListElement::Value(value) | ListElement::Spread(value) => value,
                    };
                    visit(value);
                }
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    visit(key);
                    visit(value);
                }
            }
            ExprKind::StructSchema(fields) => {
                for field in fields {
                    if let Some(default) = &field.default {
                        visit(default);
                    }
                }
            }
            ExprKind::StructInit { schema, fields } => {
                visit(schema);
                for (_, value) in fields {
                    visit(value);
                }
            }
            ExprKind::StructCopy { value, fields } => {
                visit(value);
                for (_, replacement) in fields {
                    visit(replacement);
                }
            }
            ExprKind::Index { collection, index } => {
                visit(collection);
                visit(index);
            }
            ExprKind::Slice {
                collection,
                start,
                end,
                step,
            } => {
                visit(collection);
                for bound in [start, end, step].into_iter().flatten() {
                    visit(bound);
                }
            }
            ExprKind::Resource { .. }
            | ExprKind::Enum { .. }
            | ExprKind::TypeAlias { .. }
            | ExprKind::Value(_)
            | ExprKind::Interpolate(_)
            | ExprKind::Documentation(_)
            | ExprKind::NotImplemented
            | ExprKind::Name(_) => {}
        }
    }
}
#[derive(Clone, Debug)]
pub(super) enum ExprKind {
    Value(Value),
    Interpolate(Vec<StringPart>),
    Documentation(String),
    NotImplemented,
    Name(String),
    Declare {
        mutable: bool,
        exported: bool,
        pattern: Pattern,
        documentation: Option<String>,
        tags: Vec<Tag>,
        annotation: Option<TypeAnnotation>,
        value: Box<Expr>,
    },
    Foreign {
        exported: bool,
        name: String,
        documentation: Option<String>,
        tags: Vec<Tag>,
        signature: Box<ForeignSignature>,
    },
    Resource {
        exported: bool,
        name: String,
        documentation: Option<String>,
        tags: Vec<Tag>,
    },
    Enum {
        exported: bool,
        name: String,
        cases: Vec<String>,
        documentation: Option<String>,
        tags: Vec<Tag>,
    },
    TypeAlias {
        exported: bool,
        name: String,
        annotation: TypeAnnotation,
    },
    Assign {
        name: String,
        value: Box<Expr>,
    },
    Return {
        value: Box<Expr>,
    },
    Throw {
        value: Box<Expr>,
    },
    Defer {
        value: Box<Expr>,
        mode: DeferMode,
        error_name: Option<String>,
    },
    Recur(Vec<CallArgument>),
    Nursery {
        limit: Option<Box<Expr>>,
        body: Box<Expr>,
    },
    Spawn(Box<Expr>),
    Select(Vec<SelectCase>),
    Match {
        subject: Option<Box<Expr>>,
        cases: Vec<MatchCase>,
    },
    Binary {
        left: Box<Expr>,
        operator: Binary,
        right: Box<Expr>,
    },
    Prefix {
        operators: Vec<(Prefix, SourceSpan)>,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<CallArgument>,
    },
    TypeApply {
        callee: Box<Expr>,
        arguments: Vec<TypeAnnotation>,
    },
    Function {
        type_parameters: Vec<String>,
        parameters: Vec<Parameter>,
        return_annotation: Option<TypeAnnotation>,
        body: Box<Expr>,
    },
    Block(Vec<Expr>),
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    List(Vec<ListElement>),
    Map(Vec<(Expr, Expr)>),
    StructSchema(Vec<StructSchemaField>),
    StructInit {
        schema: Box<Expr>,
        fields: Vec<(String, Expr)>,
    },
    StructCopy {
        value: Box<Expr>,
        fields: Vec<(String, Expr)>,
    },
    Index {
        collection: Box<Expr>,
        index: Box<Expr>,
    },
    Slice {
        collection: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
}
#[derive(Clone, Debug)]
pub(super) struct SelectCase {
    pub(super) kind: SelectCaseKind,
    pub(super) handler: Option<Expr>,
}
#[derive(Clone, Debug)]
pub(super) enum SelectCaseKind {
    Receive(Expr),
    Send { channel: Expr, value: Box<Expr> },
    After(Expr),
    Await(Expr),
    Default,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum StringPart {
    Text(String),
    Name(String),
}
#[derive(Clone, Debug)]
pub(super) struct Parameter {
    pub(super) name: String,
    pub(super) discard: bool,
    pub(super) tags: Vec<Tag>,
    pub(super) annotation: Option<TypeAnnotation>,
    pub(super) default: Option<Expr>,
    pub(super) variadic: bool,
}
#[derive(Clone, Debug)]
pub(super) struct ForeignSignature {
    pub(super) type_parameters: Vec<String>,
    pub(super) parameters: Vec<Parameter>,
    pub(super) return_annotation: Option<TypeAnnotation>,
}
#[derive(Clone, Debug)]
pub(super) struct Tag {
    pub(super) name: String,
    pub(super) arguments: Vec<Expr>,
}
#[derive(Clone, Debug)]
pub(super) enum CallArgument {
    Positional(Expr),
    Named { name: String, value: Expr },
    Spread(Expr),
}
#[derive(Clone, Debug)]
pub(super) enum ListElement {
    Value(Expr),
    Spread(Expr),
}
#[derive(Clone, Debug)]
pub(super) struct StructSchemaField {
    pub(super) name: String,
    pub(super) annotation: Option<TypeAnnotation>,
    pub(super) default: Option<Expr>,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TypeAnnotation {
    Name(String),
    Apply {
        name: String,
        arguments: Vec<TypeAnnotation>,
    },
    Tuple(Vec<TypeAnnotation>),
    Union(Vec<TypeAnnotation>),
}
#[derive(Clone, Debug)]
pub(super) struct MatchCase {
    pub(super) patterns: Vec<CasePattern>,
    pub(super) guard: Option<Expr>,
    pub(super) value: Expr,
    pub(super) span: SourceSpan,
    /// Exclusive end of the case body.
    pub(super) end: SourcePosition,
}
#[derive(Clone, Debug)]
pub(super) struct CasePattern {
    pub(super) pattern: Pattern,
    pub(super) constraint: Option<TypeAnnotation>,
}
#[derive(Clone, Debug)]
pub(super) enum RestPattern {
    Discard,
    Binding(String),
}
#[derive(Clone, Debug)]
pub(super) enum MapPatternKey {
    String(String),
    Computed(Expr),
}
#[derive(Clone, Debug)]
pub(super) enum Pattern {
    Literal(Value),
    Wildcard,
    Binding(String),
    Pinned(String),
    At {
        name: String,
        pattern: Box<Pattern>,
    },
    List {
        items: Vec<Pattern>,
        rest: Option<RestPattern>,
    },
    Map {
        entries: Vec<(MapPatternKey, Pattern)>,
        rest: Option<RestPattern>,
        exact: bool,
    },
    /// Binds every string key of a map into a top-level declaration scope.
    MapAll,
    EnumCase {
        path: String,
        case: String,
    },
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Binary {
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    Pipeline,
    Append,
    Prepend,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Prefix {
    Negate,
    Not,
    BitNot,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) span: SourceSpan,
    /// Exclusive end of the token's source.
    pub(super) end: SourcePosition,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TokenKind {
    Int(i64),
    Float(f64),
    Bytes(Vec<u8>),
    Interpolated(Vec<StringPart>),
    Documentation(String),
    NotImplemented,
    Name(String),
    Export,
    Foreign,
    Resource,
    Enum,
    Val,
    Var,
    Fn,
    If,
    Else,
    Return,
    Throw,
    Defer,
    Onsuccess,
    Onerror,
    Recur,
    Nursery,
    Limit,
    Spawn,
    Select,
    Match,
    Struct,
    Copy,
    True,
    False,
    Nil,
    Plus,
    ColonPlus,
    PlusColon,
    Minus,
    Star,
    Slash,
    Pipeline,
    Percent,
    Eq,
    EqEq,
    Bang,
    BangEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    AndAnd,
    OrOr,
    Ampersand,
    Pipe,
    ShiftLeft,
    ShiftRight,
    Tilde,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LExactMap,
    RExactMap,
    LBracket,
    RBracket,
    Comma,
    Colon,
    At,
    Caret,
    Dot,
    Ellipsis,
    Arrow,
    Sep,
    BlankSep,
    End,
}
