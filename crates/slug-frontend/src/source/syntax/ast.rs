use slug_vm::{DeferMode, SourceSpan, Value};

use super::SourcePosition;

#[derive(Debug)]
pub(super) struct Expr {
    pub(super) kind: ExprKind,
    pub(super) span: SourceSpan,
    /// Exclusive end of the expression's source.
    pub(super) end: SourcePosition,
    /// Levels from this expression down to its deepest leaf, counting
    /// pattern levels and every nested expression. The parser bounds it, so
    /// recursion over a tree, including dropping and cloning it, is bounded.
    pub(super) depth: u32,
}

// Cloning recurses through the whole tree, so it grows the stack like the
// compiler's passes do.
impl Clone for Expr {
    fn clone(&self) -> Self {
        super::stack::grow(|| Self {
            kind: self.kind.clone(),
            span: self.span.clone(),
            end: self.end,
            depth: self.depth,
        })
    }
}

impl Expr {
    pub(super) fn new(kind: ExprKind, span: SourceSpan, end: SourcePosition) -> Self {
        let mut expression = Self {
            kind,
            span,
            end,
            depth: 1,
        };
        let mut deepest = 0;
        expression.for_each_child(|child| deepest = deepest.max(child.depth));
        let mut tags = |tags: &[Tag]| {
            for tag in tags {
                for argument in &tag.arguments {
                    deepest = deepest.max(argument.depth);
                }
            }
        };
        match &expression.kind {
            ExprKind::Declare { pattern, .. } => deepest = deepest.max(pattern.depth()),
            ExprKind::Match { cases, .. } => {
                for case in cases {
                    for pattern in &case.patterns {
                        deepest = deepest.max(pattern.pattern.depth());
                    }
                }
            }
            ExprKind::Function { parameters, .. } => {
                for parameter in parameters {
                    tags(&parameter.tags);
                }
            }
            ExprKind::Foreign { signature, .. } => {
                for parameter in &signature.parameters {
                    tags(&parameter.tags);
                }
            }
            _ => {}
        }
        expression.depth = deepest.saturating_add(1);
        expression
    }

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
    /// Where the case's first pattern begins.
    pub(super) start: SourcePosition,
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
impl Pattern {
    /// Levels from this pattern down to its deepest leaf, including the
    /// depth of any computed map key.
    pub(super) fn depth(&self) -> u32 {
        let nested = match self {
            Pattern::At { pattern, .. } => pattern.depth(),
            Pattern::List { items, .. } => items.iter().map(Pattern::depth).max().unwrap_or(0),
            Pattern::Map { entries, .. } => entries
                .iter()
                .map(|(key, pattern)| {
                    let key = match key {
                        MapPatternKey::Computed(key) => key.depth,
                        MapPatternKey::String(_) => 0,
                    };
                    key.max(pattern.depth())
                })
                .max()
                .unwrap_or(0),
            Pattern::Literal(_)
            | Pattern::Wildcard
            | Pattern::Binding(_)
            | Pattern::Pinned(_)
            | Pattern::MapAll
            | Pattern::EnumCase { .. } => 0,
        };
        nested.saturating_add(1)
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Binary {
    Or,
    And,
    /// `??`: the left operand unless it is `nil`, else the right one.
    Coalesce,
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
    QuestionQuestion,
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
    /// A statement separator from a newline.
    Sep,
    /// A statement separator written as `;`. Enum and struct member lists
    /// accept newlines but not semicolons.
    Semicolon,
    BlankSep,
    End,
}

impl Pattern {
    /// Whether testing this pattern evaluates nothing: no pinned binding
    /// loads and no computed map keys.
    pub(super) fn is_static(&self) -> bool {
        match self {
            Self::Literal(_)
            | Self::Wildcard
            | Self::Binding(_)
            | Self::MapAll
            | Self::EnumCase { .. } => true,
            Self::Pinned(_) => false,
            Self::At { pattern, .. } => pattern.is_static(),
            Self::List { items, .. } => items.iter().all(Self::is_static),
            Self::Map { entries, .. } => entries
                .iter()
                .all(|(key, value)| matches!(key, MapPatternKey::String(_)) && value.is_static()),
        }
    }
}

impl TokenKind {
    /// The source text of a closing delimiter, which is the only token a fix
    /// may insert.
    pub(super) const fn closing_text(&self) -> Option<&'static str> {
        match self {
            Self::RParen => Some(")"),
            Self::RBracket => Some("]"),
            Self::RBrace => Some("}"),
            Self::RExactMap => Some("|}"),
            _ => None,
        }
    }

    /// The word, when this token is a reserved word, which cannot be a name.
    pub(super) fn reserved_word(&self) -> Option<&'static str> {
        Some(match self {
            Self::Export => "export",
            Self::Foreign => "foreign",
            Self::Resource => "resource",
            Self::Enum => "enum",
            Self::Val => "val",
            Self::Var => "var",
            Self::Fn => "fn",
            Self::If => "if",
            Self::Else => "else",
            Self::Return => "return",
            Self::Throw => "throw",
            Self::Defer => "defer",
            Self::Onsuccess => "onsuccess",
            Self::Onerror => "onerror",
            Self::Recur => "recur",
            Self::Nursery => "nursery",
            Self::Spawn => "spawn",
            Self::Select => "select",
            Self::Match => "match",
            Self::Struct => "struct",
            Self::Copy => "copy",
            Self::True => "true",
            Self::False => "false",
            Self::Nil => "nil",
            _ => return None,
        })
    }

    /// A short description of this token for diagnostics, such as `` `}` ``
    /// or ``name `x` ``.
    pub(super) fn describe(&self) -> String {
        match self {
            Self::Int(_) | Self::Float(_) => "number".into(),
            Self::Bytes(_) => "byte literal".into(),
            Self::Interpolated(_) => "string".into(),
            Self::Documentation(_) => "documentation block".into(),
            Self::Name(name) => format!("name `{name}`"),
            Self::Export => "`export`".into(),
            Self::Foreign => "`foreign`".into(),
            Self::Resource => "`resource`".into(),
            Self::Enum => "`enum`".into(),
            Self::Val => "`val`".into(),
            Self::Var => "`var`".into(),
            Self::Fn => "`fn`".into(),
            Self::If => "`if`".into(),
            Self::Else => "`else`".into(),
            Self::Return => "`return`".into(),
            Self::Throw => "`throw`".into(),
            Self::Defer => "`defer`".into(),
            Self::Onsuccess => "`onsuccess`".into(),
            Self::Onerror => "`onerror`".into(),
            Self::Recur => "`recur`".into(),
            Self::Nursery => "`nursery`".into(),
            Self::Spawn => "`spawn`".into(),
            Self::Select => "`select`".into(),
            Self::Match => "`match`".into(),
            Self::Struct => "`struct`".into(),
            Self::Copy => "`copy`".into(),
            Self::True => "`true`".into(),
            Self::False => "`false`".into(),
            Self::Nil => "`nil`".into(),
            Self::Plus => "`+`".into(),
            Self::ColonPlus => "`:+`".into(),
            Self::PlusColon => "`+:`".into(),
            Self::Minus => "`-`".into(),
            Self::Star => "`*`".into(),
            Self::Slash => "`/`".into(),
            Self::Pipeline => "`/>`".into(),
            Self::Percent => "`%`".into(),
            Self::Eq => "`=`".into(),
            Self::EqEq => "`==`".into(),
            Self::Bang => "`!`".into(),
            Self::BangEq => "`!=`".into(),
            Self::Less => "`<`".into(),
            Self::LessEq => "`<=`".into(),
            Self::Greater => "`>`".into(),
            Self::GreaterEq => "`>=`".into(),
            Self::AndAnd => "`&&`".into(),
            Self::OrOr => "`||`".into(),
            Self::QuestionQuestion => "`??`".into(),
            Self::Ampersand => "`&`".into(),
            Self::Pipe => "`|`".into(),
            Self::ShiftLeft => "`<<`".into(),
            Self::ShiftRight => "`>>`".into(),
            Self::Tilde => "`~`".into(),
            Self::LParen => "`(`".into(),
            Self::RParen => "`)`".into(),
            Self::LBrace => "`{`".into(),
            Self::RBrace => "`}`".into(),
            Self::LExactMap => "`{|`".into(),
            Self::RExactMap => "`|}`".into(),
            Self::LBracket => "`[`".into(),
            Self::RBracket => "`]`".into(),
            Self::Comma => "`,`".into(),
            Self::Colon => "`:`".into(),
            Self::At => "`@`".into(),
            Self::Caret => "`^`".into(),
            Self::Dot => "`.`".into(),
            Self::Ellipsis => "`...`".into(),
            Self::Arrow => "`=>`".into(),
            Self::NotImplemented => "`???`".into(),
            Self::Sep => "statement separator".into(),
            Self::Semicolon => "`;`".into(),
            Self::BlankSep => "blank line".into(),
            Self::End => "end of input".into(),
        }
    }
}
