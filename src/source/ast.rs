use crate::{DeferMode, SourceSpan, Value};

#[derive(Clone, Debug)]
pub(super) struct Expr {
    pub(super) kind: ExprKind,
    pub(super) span: SourceSpan,
}
#[derive(Clone, Debug)]
pub(super) enum ExprKind {
    Value(Value),
    Name(String),
    Declare {
        mutable: bool,
        pattern: Pattern,
        value: Box<Expr>,
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
    Recur(Vec<Expr>),
    Match {
        subject: Box<Expr>,
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
        arguments: Vec<Expr>,
    },
    Function {
        parameters: Vec<String>,
        body: Box<Expr>,
    },
    Block(Vec<Expr>),
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    StructSchema(Vec<StructSchemaField>),
    StructInit {
        schema: Box<Expr>,
        fields: Vec<(String, Expr)>,
    },
    Index {
        collection: Box<Expr>,
        index: Box<Expr>,
    },
}
#[derive(Clone, Debug)]
pub(super) struct StructSchemaField {
    pub(super) name: String,
    pub(super) default: Option<Expr>,
}
#[derive(Clone, Debug)]
pub(super) struct MatchCase {
    pub(super) patterns: Vec<Pattern>,
    pub(super) guard: Option<Expr>,
    pub(super) value: Expr,
    pub(super) span: SourceSpan,
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
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Binary {
    Or,
    And,
    Add,
    Subtract,
    Multiply,
    Divide,
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
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TokenKind {
    Int(i64),
    Str(String),
    Name(String),
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
    Match,
    Struct,
    True,
    False,
    Nil,
    Plus,
    Minus,
    Star,
    Slash,
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
    End,
}
