//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashMap, fmt};

use crate::{Capture, Chunk, Op, Program, SourceSpan, Value};

#[derive(Clone, Debug)]
pub struct SourceError {
    pub kind: SourceErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceErrorKind {
    Parse,
    Semantic,
}

impl SourceError {
    fn at(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Parse,
            message: message.into(),
            span: Some(span),
        }
    }

    fn semantic(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            kind: SourceErrorKind::Semantic,
            message: message.into(),
            span: Some(span),
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at {}:{}:{}", span.path, span.line, span.column)?;
        }
        Ok(())
    }
}
impl std::error::Error for SourceError {}

/// Compiles the currently supported core Slug source subset into a VM program.
///
/// # Errors
/// Returns a source error with a source location for invalid syntax or checked
/// source semantics.
pub fn compile(path: &str, source: &str) -> Result<Program, SourceError> {
    let tokens = Lexer::new(path, source).tokens()?;
    Compiler::new(path, Parser::new(tokens).parse()?).compile()
}

#[derive(Clone, Debug)]
struct Expr {
    kind: ExprKind,
    span: SourceSpan,
}
#[derive(Clone, Debug)]
enum ExprKind {
    Value(Value),
    Name(String),
    Declare {
        mutable: bool,
        name: String,
        value: Box<Expr>,
    },
    Assign {
        name: String,
        value: Box<Expr>,
    },
    Return {
        value: Box<Expr>,
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
    Index {
        collection: Box<Expr>,
        index: Box<Expr>,
    },
}
#[derive(Clone, Copy, Debug)]
enum Binary {
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
enum Prefix {
    Negate,
    Not,
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: TokenKind,
    span: SourceSpan,
}
#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Int(i64),
    Str(String),
    Name(String),
    Val,
    Var,
    Fn,
    If,
    Else,
    Return,
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
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Sep,
    End,
}

struct Lexer {
    path: String,
    input: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
}
impl Lexer {
    fn new(path: &str, input: &str) -> Self {
        Self {
            path: path.into(),
            input: input.chars().collect(),
            index: 0,
            line: 1,
            column: 1,
        }
    }
    fn span(&self) -> SourceSpan {
        SourceSpan::new(self.path.clone(), self.line, self.column)
    }
    fn peek(&self) -> Option<char> {
        self.input.get(self.index).copied()
    }
    fn next(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.index += 1;
        if value == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(value)
    }
    fn newline_continues(&self, tokens: &[Token]) -> bool {
        matches!(
            tokens.last().map(|token| &token.kind),
            Some(
                TokenKind::Eq
                    | TokenKind::EqEq
                    | TokenKind::BangEq
                    | TokenKind::Less
                    | TokenKind::LessEq
                    | TokenKind::Greater
                    | TokenKind::GreaterEq
                    | TokenKind::AndAnd
                    | TokenKind::OrOr
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Colon
                    | TokenKind::Dot
            )
        ) || self.next_starts_infix()
    }
    fn next_starts_infix(&self) -> bool {
        let mut index = self.index;
        while matches!(self.input.get(index), Some(' ' | '\t' | '\r')) {
            index += 1;
        }
        matches!(
            self.input.get(index),
            Some('+' | '-' | '*' | '/' | '<' | '>' | '=' | '!' | '&' | '|' | '.')
        )
    }
    fn push(tokens: &mut Vec<Token>, kind: TokenKind, span: SourceSpan) {
        tokens.push(Token { kind, span });
    }
    #[allow(clippy::too_many_lines)]
    fn tokens(mut self) -> Result<Vec<Token>, SourceError> {
        let mut result = Vec::new();
        let mut delimiters = 0usize;
        while let Some(character) = self.peek() {
            let span = self.span();
            match character {
                ' ' | '\t' | '\r' => {
                    self.next();
                }
                ';' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Sep, span);
                }
                '\n' => {
                    self.next();
                    if delimiters == 0 && !self.newline_continues(&result) {
                        Self::push(&mut result, TokenKind::Sep, span);
                    }
                }
                '#' => {
                    self.next();
                    while self.peek().is_some_and(|value| value != '\n') {
                        self.next();
                    }
                }
                '+' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Plus, span);
                }
                '-' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Minus, span);
                }
                '*' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Star, span);
                }
                '/' => {
                    self.next();
                    match self.peek() {
                        Some('/') => {
                            self.next();
                            while self.peek().is_some_and(|value| value != '\n') {
                                self.next();
                            }
                        }
                        Some('*') => {
                            self.next();
                            let mut closed = false;
                            while let Some(value) = self.next() {
                                if value == '*' && self.peek() == Some('/') {
                                    self.next();
                                    closed = true;
                                    break;
                                }
                            }
                            if !closed {
                                return Err(SourceError::at("unterminated block comment", span));
                            }
                        }
                        _ => Self::push(&mut result, TokenKind::Slash, span),
                    }
                }
                '&' => {
                    self.next();
                    if self.peek() != Some('&') {
                        return Err(SourceError::at("expected & after &", span));
                    }
                    self.next();
                    Self::push(&mut result, TokenKind::AndAnd, span);
                }
                '|' => {
                    self.next();
                    if self.peek() != Some('|') {
                        return Err(SourceError::at("expected | after |", span));
                    }
                    self.next();
                    Self::push(&mut result, TokenKind::OrOr, span);
                }
                '(' => {
                    self.next();
                    delimiters += 1;
                    Self::push(&mut result, TokenKind::LParen, span);
                }
                ')' => {
                    self.next();
                    delimiters = delimiters.saturating_sub(1);
                    Self::push(&mut result, TokenKind::RParen, span);
                }
                '{' => {
                    self.next();
                    Self::push(&mut result, TokenKind::LBrace, span);
                }
                '}' => {
                    self.next();
                    Self::push(&mut result, TokenKind::RBrace, span);
                }
                '[' => {
                    self.next();
                    delimiters += 1;
                    Self::push(&mut result, TokenKind::LBracket, span);
                }
                ']' => {
                    self.next();
                    delimiters = delimiters.saturating_sub(1);
                    Self::push(&mut result, TokenKind::RBracket, span);
                }
                ',' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Comma, span);
                }
                ':' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Colon, span);
                }
                '.' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Dot, span);
                }
                '=' => {
                    self.next();
                    let kind = if self.peek() == Some('=') {
                        self.next();
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    };
                    Self::push(&mut result, kind, span);
                }
                '!' => {
                    self.next();
                    let kind = if self.peek() == Some('=') {
                        self.next();
                        TokenKind::BangEq
                    } else {
                        TokenKind::Bang
                    };
                    Self::push(&mut result, kind, span);
                }
                '<' => {
                    self.next();
                    let kind = if self.peek() == Some('=') {
                        self.next();
                        TokenKind::LessEq
                    } else {
                        TokenKind::Less
                    };
                    Self::push(&mut result, kind, span);
                }
                '>' => {
                    self.next();
                    let kind = if self.peek() == Some('=') {
                        self.next();
                        TokenKind::GreaterEq
                    } else {
                        TokenKind::Greater
                    };
                    Self::push(&mut result, kind, span);
                }
                '0'..='9' => {
                    let mut text = String::new();
                    while self
                        .peek()
                        .is_some_and(|value| value.is_ascii_digit() || value == '_')
                    {
                        text.push(self.next().expect("peeked character exists"));
                    }
                    let value = text
                        .replace('_', "")
                        .parse()
                        .map_err(|_| SourceError::at("invalid number", span.clone()))?;
                    Self::push(&mut result, TokenKind::Int(value), span);
                }
                '"' => {
                    self.next();
                    let mut text = String::new();
                    loop {
                        match self.next() {
                            Some('"') => break,
                            Some('\\') => match self.next() {
                                Some('n') => text.push('\n'),
                                Some('r') => text.push('\r'),
                                Some('t') => text.push('\t'),
                                Some('"') => text.push('"'),
                                Some('\\') => text.push('\\'),
                                _ => return Err(SourceError::at("invalid string escape", span)),
                            },
                            Some(value) => text.push(value),
                            None => return Err(SourceError::at("unterminated string", span)),
                        }
                    }
                    Self::push(&mut result, TokenKind::Str(text), span);
                }
                value if value == '_' || value.is_alphabetic() => {
                    let mut text = String::new();
                    while self
                        .peek()
                        .is_some_and(|item| item == '_' || item.is_alphanumeric())
                    {
                        text.push(self.next().expect("peeked character exists"));
                    }
                    let kind = match text.as_str() {
                        "val" => TokenKind::Val,
                        "var" => TokenKind::Var,
                        "fn" => TokenKind::Fn,
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "return" => TokenKind::Return,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        "nil" => TokenKind::Nil,
                        _ => TokenKind::Name(text),
                    };
                    Self::push(&mut result, kind, span);
                }
                _ => {
                    return Err(SourceError::at(
                        format!("unexpected character `{character}`"),
                        span,
                    ));
                }
            }
        }
        Self::push(&mut result, TokenKind::End, self.span());
        Ok(result)
    }
}

const MAX_PARSE_NESTING: usize = 512;

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    nesting: usize,
}
impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            nesting: 0,
        }
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn kind(&self) -> &TokenKind {
        &self.peek().kind
    }
    fn next(&mut self) -> Token {
        let token = self.peek().clone();
        self.index += 1;
        token
    }
    fn matches(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.kind()) == std::mem::discriminant(kind)
    }
    fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<Token, SourceError> {
        if self.matches(kind) {
            Ok(self.next())
        } else {
            Err(SourceError::at(message, self.peek().span.clone()))
        }
    }
    fn separators(&mut self) {
        while self.matches(&TokenKind::Sep) {
            self.next();
        }
    }
    fn enter_nesting(&mut self, span: SourceSpan) -> Result<(), SourceError> {
        if self.nesting == MAX_PARSE_NESTING {
            return Err(SourceError::at("source nesting limit exceeded", span));
        }
        self.nesting += 1;
        Ok(())
    }
    fn leave_nesting(&mut self) {
        self.nesting -= 1;
    }
    fn parse(&mut self) -> Result<Vec<Expr>, SourceError> {
        let mut expressions = Vec::new();
        self.separators();
        while !self.matches(&TokenKind::End) {
            expressions.push(self.statement()?);
            if !matches!(self.kind(), TokenKind::End | TokenKind::Sep) {
                return Err(SourceError::at(
                    "expected statement separator",
                    self.peek().span.clone(),
                ));
            }
            self.separators();
        }
        Ok(expressions)
    }
    fn statement(&mut self) -> Result<Expr, SourceError> {
        if matches!(self.kind(), TokenKind::Return) {
            let span = self.next().span;
            let value = self.expression()?;
            return Ok(Expr {
                span,
                kind: ExprKind::Return {
                    value: Box::new(value),
                },
            });
        }
        if matches!(self.kind(), TokenKind::Val | TokenKind::Var) {
            let mutable = matches!(self.next().kind, TokenKind::Var);
            let token = self.next();
            let TokenKind::Name(name) = token.kind else {
                return Err(SourceError::at("expected binding name", token.span));
            };
            self.consume(&TokenKind::Eq, "expected =")?;
            let value = self.expression()?;
            return Ok(Expr {
                span: value.span.clone(),
                kind: ExprKind::Declare {
                    mutable,
                    name,
                    value: Box::new(value),
                },
            });
        }
        self.expression()
    }
    fn expression(&mut self) -> Result<Expr, SourceError> {
        if let (
            TokenKind::Name(name),
            Some(Token {
                kind: TokenKind::Eq,
                ..
            }),
        ) = (self.kind().clone(), self.tokens.get(self.index + 1))
        {
            let span = self.next().span;
            self.next();
            self.enter_nesting(span.clone())?;
            let value = self.expression()?;
            self.leave_nesting();
            return Ok(Expr {
                span,
                kind: ExprKind::Assign {
                    name,
                    value: Box::new(value),
                },
            });
        }
        self.binary(0)
    }
    fn binary(&mut self, minimum: u8) -> Result<Expr, SourceError> {
        let mut left = self.prefix()?;
        loop {
            let (operator, precedence) = match self.kind() {
                TokenKind::OrOr => (Binary::Or, 1),
                TokenKind::AndAnd => (Binary::And, 2),
                TokenKind::EqEq => (Binary::Equal, 3),
                TokenKind::BangEq => (Binary::NotEqual, 3),
                TokenKind::Less => (Binary::Less, 4),
                TokenKind::LessEq => (Binary::LessEqual, 4),
                TokenKind::Greater => (Binary::Greater, 4),
                TokenKind::GreaterEq => (Binary::GreaterEqual, 4),
                TokenKind::Plus => (Binary::Add, 5),
                TokenKind::Minus => (Binary::Subtract, 5),
                TokenKind::Star => (Binary::Multiply, 6),
                TokenKind::Slash => (Binary::Divide, 6),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            let span = self.next().span;
            let right = self.binary(precedence + 1)?;
            left = Expr {
                span,
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }
    fn prefix(&mut self) -> Result<Expr, SourceError> {
        let mut operators = Vec::new();
        while self.matches(&TokenKind::Minus) || self.matches(&TokenKind::Bang) {
            let token = self.next();
            let operator = if matches!(token.kind, TokenKind::Minus) {
                Prefix::Negate
            } else {
                Prefix::Not
            };
            operators.push((operator, token.span));
        }
        let mut value = self.primary()?;
        loop {
            if self.matches(&TokenKind::LParen) {
                let delimiter = self.next();
                self.enter_nesting(delimiter.span)?;
                let mut arguments = Vec::new();
                if !self.matches(&TokenKind::RParen) {
                    loop {
                        arguments.push(self.expression()?);
                        if !self.matches(&TokenKind::Comma) {
                            break;
                        }
                        self.next();
                        if self.matches(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, "expected )")?;
                self.leave_nesting();
                let span = value.span.clone();
                value = Expr {
                    span,
                    kind: ExprKind::Call {
                        callee: Box::new(value),
                        arguments,
                    },
                };
            } else if self.matches(&TokenKind::LBracket) {
                let span = self.next().span;
                self.enter_nesting(span.clone())?;
                let index = self.expression()?;
                self.consume(&TokenKind::RBracket, "expected ]")?;
                self.leave_nesting();
                value = Expr {
                    span,
                    kind: ExprKind::Index {
                        collection: Box::new(value),
                        index: Box::new(index),
                    },
                };
            } else if self.matches(&TokenKind::Dot) {
                let span = self.next().span;
                let name = self.next();
                let TokenKind::Name(name) = name.kind else {
                    return Err(SourceError::at("expected property name", name.span));
                };
                let index = Expr {
                    span: span.clone(),
                    kind: ExprKind::Value(Value::string(name)),
                };
                value = Expr {
                    span,
                    kind: ExprKind::Index {
                        collection: Box::new(value),
                        index: Box::new(index),
                    },
                };
            } else {
                break;
            }
        }
        if operators.is_empty() {
            Ok(value)
        } else {
            let span = operators[0].1.clone();
            Ok(Expr {
                span,
                kind: ExprKind::Prefix {
                    operators,
                    value: Box::new(value),
                },
            })
        }
    }
    fn primary(&mut self) -> Result<Expr, SourceError> {
        let token = self.next();
        let span = token.span.clone();
        let kind = match token.kind {
            TokenKind::Int(value) => ExprKind::Value(Value::Int(value)),
            TokenKind::Str(value) => ExprKind::Value(Value::string(value)),
            TokenKind::True => ExprKind::Value(Value::Bool(true)),
            TokenKind::False => ExprKind::Value(Value::Bool(false)),
            TokenKind::Nil => ExprKind::Value(Value::Nil),
            TokenKind::Name(value) => ExprKind::Name(value),
            TokenKind::LParen => {
                self.enter_nesting(span.clone())?;
                let value = self.expression()?;
                self.consume(&TokenKind::RParen, "expected )")?;
                self.leave_nesting();
                return Ok(value);
            }
            TokenKind::LBracket => return self.list(span),
            TokenKind::LBrace => return self.map_or_block(span),
            TokenKind::Fn => return self.function(span),
            TokenKind::If => return self.if_expression(span),
            _ => return Err(SourceError::at("expected expression", span)),
        };
        Ok(Expr { kind, span })
    }
    fn list(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut values = Vec::new();
        if !self.matches(&TokenKind::RBracket) {
            loop {
                values.push(self.expression()?);
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.next();
                if self.matches(&TokenKind::RBracket) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RBracket, "expected ]")?;
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::List(values),
            span,
        })
    }
    fn map_or_block(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        if self.matches(&TokenKind::RBrace) {
            self.next();
            return Ok(Expr {
                kind: ExprKind::Map(Vec::new()),
                span,
            });
        }
        let map = (matches!(self.kind(), TokenKind::Name(_))
            && matches!(
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                Some(TokenKind::Colon)
            ))
            || self.starts_computed_map_key();
        if map {
            self.map(span)
        } else {
            self.block_after_open(span)
        }
    }
    fn starts_computed_map_key(&self) -> bool {
        if !self.matches(&TokenKind::LBracket) {
            return false;
        }
        let mut depth = 0usize;
        for (offset, token) in self.tokens[self.index..].iter().enumerate() {
            match token.kind {
                TokenKind::LBracket => depth += 1,
                TokenKind::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.tokens
                                .get(self.index + offset + 1)
                                .map(|token| &token.kind),
                            Some(TokenKind::Colon)
                        );
                    }
                }
                _ => {}
            }
        }
        false
    }
    fn map(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut entries = Vec::new();
        loop {
            let key = if self.matches(&TokenKind::LBracket) {
                self.next();
                let key = self.expression()?;
                self.consume(&TokenKind::RBracket, "expected ]")?;
                key
            } else {
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected map key", token.span));
                };
                Expr {
                    span: token.span,
                    kind: ExprKind::Value(Value::string(name)),
                }
            };
            self.consume(&TokenKind::Colon, "expected :")?;
            let value = self.expression()?;
            entries.push((key, value));
            if !self.matches(&TokenKind::Comma) {
                break;
            }
            self.next();
            if self.matches(&TokenKind::RBrace) {
                break;
            }
        }
        self.consume(&TokenKind::RBrace, "expected }")?;
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::Map(entries),
            span,
        })
    }
    fn block_after_open(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut values = Vec::new();
        self.separators();
        while !self.matches(&TokenKind::RBrace) {
            if self.matches(&TokenKind::End) {
                return Err(SourceError::at("expected }", self.peek().span.clone()));
            }
            values.push(self.statement()?);
            if !matches!(self.kind(), TokenKind::RBrace | TokenKind::Sep) {
                return Err(SourceError::at(
                    "expected statement separator",
                    self.peek().span.clone(),
                ));
            }
            self.separators();
        }
        self.next();
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::Block(values),
            span,
        })
    }
    fn block(&mut self) -> Result<Expr, SourceError> {
        let span = self.consume(&TokenKind::LBrace, "expected {")?.span;
        self.block_after_open(span)
    }
    fn function(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.consume(&TokenKind::LParen, "expected (")?;
        let mut parameters = Vec::new();
        if !self.matches(&TokenKind::RParen) {
            loop {
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected parameter name", token.span));
                };
                parameters.push(name);
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.next();
                if self.matches(&TokenKind::RParen) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, "expected )")?;
        let body = self.block()?;
        Ok(Expr {
            kind: ExprKind::Function {
                parameters,
                body: Box::new(body),
            },
            span,
        })
    }
    fn if_expression(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.consume(&TokenKind::LParen, "expected (")?;
        let condition = self.expression()?;
        self.consume(&TokenKind::RParen, "expected )")?;
        let then_branch = self.block()?;
        let else_branch = if self.matches(&TokenKind::Else) {
            self.next();
            Some(Box::new(if self.matches(&TokenKind::If) {
                let token = self.next();
                self.if_expression(token.span)?
            } else {
                self.block()?
            }))
        } else {
            None
        };
        Ok(Expr {
            kind: ExprKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch,
            },
            span,
        })
    }
}

#[derive(Clone, Debug)]
enum Binding {
    Global { mutable: bool },
    Local { slot: usize, mutable: bool },
    Capture { slot: usize, mutable: bool },
}
struct Compiler {
    path: String,
    expressions: Vec<Expr>,
    chunks: Vec<Chunk>,
    globals: HashMap<String, bool>,
}
impl Compiler {
    fn new(path: &str, expressions: Vec<Expr>) -> Self {
        Self {
            path: path.into(),
            expressions,
            chunks: Vec::new(),
            globals: HashMap::new(),
        }
    }
    fn compile(mut self) -> Result<Program, SourceError> {
        for expression in &self.expressions {
            if let ExprKind::Declare { mutable, name, .. } = &expression.kind {
                self.globals.insert(name.clone(), *mutable);
            }
        }
        let expressions = self.expressions.clone();
        let mut state = State::root();
        for (index, expression) in expressions.iter().enumerate() {
            self.expression(&mut state, expression)?;
            if index + 1 < expressions.len() {
                state.emit(Op::Pop, &expression.span);
            }
        }
        if expressions.is_empty() {
            state.emit(Op::Nil, &SourceSpan::new(self.path.clone(), 1, 1));
        }
        state.emit(Op::Return, &SourceSpan::new(self.path.clone(), 1, 1));
        self.chunks.push(state.finish("main", 0));
        let mut program = Program::new();
        for chunk in self.chunks {
            program.add_chunk(chunk);
        }
        Ok(program)
    }
    #[allow(clippy::too_many_lines)]
    fn expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        match &expression.kind {
            ExprKind::Value(value) => {
                let constant = state.chunk.constant(value.clone());
                state.emit(Op::Constant(constant), &expression.span);
            }
            ExprKind::Name(name) => match state.lookup(name).or_else(|| {
                self.globals
                    .get(name)
                    .map(|mutable| Binding::Global { mutable: *mutable })
            }) {
                Some(Binding::Global { .. }) | None => {
                    state.emit(Op::GetGlobal(name.clone()), &expression.span);
                }
                Some(Binding::Local { slot, .. }) => {
                    state.emit(Op::GetLocal(slot), &expression.span);
                }
                Some(Binding::Capture { slot, .. }) => {
                    state.emit(Op::GetCapture(slot), &expression.span);
                }
            },
            ExprKind::Declare {
                mutable,
                name,
                value,
            } => {
                self.expression(state, value)?;
                if state.is_root() {
                    self.globals.insert(name.clone(), *mutable);
                    state.emit(Op::DefineGlobal(name.clone()), &expression.span);
                } else {
                    let slot = state.declare(name.clone(), *mutable);
                    state.emit(Op::SetLocal(slot), &expression.span);
                }
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Assign { name, value } => {
                let binding = state
                    .lookup(name)
                    .or_else(|| {
                        self.globals
                            .get(name)
                            .map(|mutable| Binding::Global { mutable: *mutable })
                    })
                    .ok_or_else(|| {
                        SourceError::semantic(
                            format!("unknown name `{name}`"),
                            expression.span.clone(),
                        )
                    })?;
                let mutable = match binding {
                    Binding::Global { mutable }
                    | Binding::Local { mutable, .. }
                    | Binding::Capture { mutable, .. } => mutable,
                };
                if !mutable {
                    return Err(SourceError::semantic(
                        format!("cannot assign to immutable binding `{name}`"),
                        expression.span.clone(),
                    ));
                }
                self.expression(state, value)?;
                match binding {
                    Binding::Global { .. } => {
                        state.emit(Op::SetGlobal(name.clone()), &expression.span);
                    }
                    Binding::Local { slot, .. } => {
                        state.emit(Op::SetLocal(slot), &expression.span);
                    }
                    Binding::Capture { slot, .. } => {
                        state.emit(Op::SetCapture(slot), &expression.span);
                    }
                }
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Return { value } => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "return is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                self.expression(state, value)?;
                state.emit(Op::Return, &expression.span);
            }
            ExprKind::Binary {
                left,
                operator,
                right,
            } => {
                self.expression(state, left)?;
                match operator {
                    Binary::And => {
                        let end = state.jump_if_false(&expression.span);
                        state.emit(Op::Pop, &expression.span);
                        self.expression(state, right)?;
                        state.patch(end);
                    }
                    Binary::Or => {
                        let right_hand_side = state.jump_if_false(&expression.span);
                        let end = state.jump(&expression.span);
                        state.patch(right_hand_side);
                        state.emit(Op::Pop, &expression.span);
                        self.expression(state, right)?;
                        state.patch(end);
                    }
                    Binary::NotEqual => {
                        self.expression(state, right)?;
                        state.emit(Op::Equal, &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::GreaterEqual => {
                        self.expression(state, right)?;
                        state.emit(Op::Less, &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::LessEqual => {
                        self.expression(state, right)?;
                        state.emit(Op::Greater, &expression.span);
                        state.emit(Op::Not, &expression.span);
                    }
                    Binary::Add => {
                        self.expression(state, right)?;
                        state.emit(Op::Add, &expression.span);
                    }
                    Binary::Subtract => {
                        self.expression(state, right)?;
                        state.emit(Op::Subtract, &expression.span);
                    }
                    Binary::Multiply => {
                        self.expression(state, right)?;
                        state.emit(Op::Multiply, &expression.span);
                    }
                    Binary::Divide => {
                        self.expression(state, right)?;
                        state.emit(Op::Divide, &expression.span);
                    }
                    Binary::Equal => {
                        self.expression(state, right)?;
                        state.emit(Op::Equal, &expression.span);
                    }
                    Binary::Greater => {
                        self.expression(state, right)?;
                        state.emit(Op::Greater, &expression.span);
                    }
                    Binary::Less => {
                        self.expression(state, right)?;
                        state.emit(Op::Less, &expression.span);
                    }
                }
            }
            ExprKind::Prefix { operators, value } => {
                self.expression(state, value)?;
                for (operator, span) in operators.iter().rev() {
                    state.emit(
                        match operator {
                            Prefix::Negate => Op::Negate,
                            Prefix::Not => Op::Not,
                        },
                        span,
                    );
                }
            }
            ExprKind::Call { callee, arguments } => {
                self.expression(state, callee)?;
                for argument in arguments {
                    self.expression(state, argument)?;
                }
                state.emit(Op::Call(arguments.len()), &expression.span);
            }
            ExprKind::List(values) => {
                for value in values {
                    self.expression(state, value)?;
                }
                state.emit(Op::List(values.len()), &expression.span);
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.expression(state, key)?;
                    self.expression(state, value)?;
                }
                state.emit(Op::Map(entries.len()), &expression.span);
            }
            ExprKind::Index { collection, index } => {
                self.expression(state, collection)?;
                self.expression(state, index)?;
                state.emit(Op::GetIndex, &expression.span);
            }
            ExprKind::Block(values) => {
                state.enter_scope();
                for (index, value) in values.iter().enumerate() {
                    self.expression(state, value)?;
                    if index + 1 < values.len() {
                        state.emit(Op::Pop, &value.span);
                    }
                }
                if values.is_empty() {
                    state.emit(Op::Nil, &expression.span);
                }
                state.leave_scope();
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(state, condition)?;
                let otherwise = state.jump_if_false(&expression.span);
                state.emit(Op::Pop, &expression.span);
                self.expression(state, then_branch)?;
                let end = state.jump(&expression.span);
                state.patch(otherwise);
                state.emit(Op::Pop, &expression.span);
                if let Some(otherwise) = else_branch {
                    self.expression(state, otherwise)?;
                } else {
                    state.emit(Op::Nil, &expression.span);
                }
                state.patch(end);
            }
            ExprKind::Function { parameters, body } => {
                let (mut chunk, captures) = self.function(parameters, body, state.visible())?;
                let index = self.chunks.len();
                chunk.name = format!("<fn #{index}>");
                self.chunks.push(chunk);
                state.emit(
                    Op::MakeClosure {
                        chunk: index,
                        captures,
                    },
                    &expression.span,
                );
            }
        }
        Ok(())
    }
    fn function(
        &mut self,
        parameters: &[String],
        body: &Expr,
        visible: HashMap<String, Binding>,
    ) -> Result<(Chunk, Vec<Capture>), SourceError> {
        let mut state = State::function(parameters, visible);
        self.expression(&mut state, body)?;
        state.emit(Op::Return, &body.span);
        let captures = state.captures.clone();
        Ok((state.finish("<fn>", parameters.len()), captures))
    }
}

struct State {
    chunk: Chunk,
    scopes: Vec<HashMap<String, Binding>>,
    outer: HashMap<String, Binding>,
    captures: Vec<Capture>,
    root: bool,
    next_local: usize,
}
impl State {
    fn root() -> Self {
        Self {
            chunk: Chunk::new("main", 0),
            scopes: vec![HashMap::new()],
            outer: HashMap::new(),
            captures: Vec::new(),
            root: true,
            next_local: 0,
        }
    }
    fn function(parameters: &[String], visible: HashMap<String, Binding>) -> Self {
        let mut outer = HashMap::new();
        let mut captures = Vec::new();
        for (name, binding) in visible {
            match binding {
                Binding::Global { mutable } => {
                    outer.insert(name, Binding::Global { mutable });
                }
                Binding::Local { slot, mutable } => {
                    let capture = captures.len();
                    captures.push(Capture::Local(slot));
                    outer.insert(
                        name,
                        Binding::Capture {
                            slot: capture,
                            mutable,
                        },
                    );
                }
                Binding::Capture { slot, mutable } => {
                    let capture = captures.len();
                    captures.push(Capture::Capture(slot));
                    outer.insert(
                        name,
                        Binding::Capture {
                            slot: capture,
                            mutable,
                        },
                    );
                }
            }
        }
        let mut parameters_scope = HashMap::new();
        for (slot, name) in parameters.iter().enumerate() {
            parameters_scope.insert(
                name.clone(),
                Binding::Local {
                    slot,
                    mutable: false,
                },
            );
        }
        Self {
            chunk: Chunk::new("<fn>", parameters.len()),
            scopes: vec![parameters_scope],
            outer,
            captures,
            root: false,
            next_local: parameters.len(),
        }
    }
    fn finish(mut self, name: &str, arity: usize) -> Chunk {
        self.chunk.name = name.into();
        self.chunk.arity = arity;
        self.chunk.locals = self.next_local;
        self.chunk
    }
    fn emit(&mut self, op: Op, span: &SourceSpan) {
        self.chunk.emit_at(op, span.clone());
    }
    fn jump_if_false(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::JumpIfFalse(usize::MAX), span);
        index
    }
    fn jump(&mut self, span: &SourceSpan) -> usize {
        let index = self.chunk.code.len();
        self.emit(Op::Jump(usize::MAX), span);
        index
    }
    fn patch(&mut self, instruction: usize) {
        let target = self.chunk.code.len();
        match &mut self.chunk.code[instruction].op {
            Op::Jump(value) | Op::JumpIfFalse(value) => *value = target,
            _ => unreachable!("only jump instructions are patched"),
        }
    }
    fn is_root(&self) -> bool {
        self.root && self.scopes.len() == 1
    }
    fn allows_return(&self) -> bool {
        !self.root
    }
    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn leave_scope(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: String, mutable: bool) -> usize {
        let slot = self.next_local;
        self.next_local += 1;
        self.scopes
            .last_mut()
            .expect("a compiler state always has a scope")
            .insert(name, Binding::Local { slot, mutable });
        slot
    }
    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .or_else(|| self.outer.get(name).cloned())
    }
    fn visible(&self) -> HashMap<String, Binding> {
        let mut result = self.outer.clone();
        for scope in &self.scopes {
            result.extend(scope.clone());
        }
        result
    }
}
