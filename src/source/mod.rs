//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use crate::{Capture, Chunk, MatchPattern, Op, Program, SourceSpan, Value};

mod ast;
mod lexer;
mod parser;
use ast::{Binary, Expr, ExprKind, MatchCase, Pattern, Prefix, Token, TokenKind};
use lexer::Lexer;
use parser::Parser;

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

impl Lexer {
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
                    let kind = if self.peek() == Some('|') {
                        self.next();
                        TokenKind::OrOr
                    } else if self.peek() == Some('}') {
                        self.next();
                        TokenKind::RExactMap
                    } else {
                        return Err(SourceError::at("expected | after |", span));
                    };
                    Self::push(&mut result, kind, span);
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
                    let kind = if self.peek() == Some('|') {
                        self.next();
                        TokenKind::LExactMap
                    } else {
                        TokenKind::LBrace
                    };
                    Self::push(&mut result, kind, span);
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
                    let kind = if self.peek() == Some('.') {
                        self.next();
                        if self.peek() != Some('.') {
                            return Err(SourceError::at("expected . after ..", span));
                        }
                        self.next();
                        TokenKind::Ellipsis
                    } else {
                        TokenKind::Dot
                    };
                    Self::push(&mut result, kind, span);
                }
                '=' => {
                    self.next();
                    let kind = if self.peek() == Some('=') {
                        self.next();
                        TokenKind::EqEq
                    } else if self.peek() == Some('>') {
                        self.next();
                        TokenKind::Arrow
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
                        "throw" => TokenKind::Throw,
                        "defer" => TokenKind::Defer,
                        "onsuccess" => TokenKind::Onsuccess,
                        "onerror" => TokenKind::Onerror,
                        "recur" => TokenKind::Recur,
                        "match" => TokenKind::Match,
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

impl Parser {
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
            TokenKind::Recur => return self.recur(span),
            TokenKind::Match => return self.match_expression(span),
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
        let body = if self.matches(&TokenKind::Match) {
            let match_span = self.next().span;
            let subject = if parameters.len() == 1 {
                Expr {
                    kind: ExprKind::Name(parameters[0].clone()),
                    span: match_span.clone(),
                }
            } else {
                Expr {
                    kind: ExprKind::List(
                        parameters
                            .iter()
                            .map(|parameter| Expr {
                                kind: ExprKind::Name(parameter.clone()),
                                span: match_span.clone(),
                            })
                            .collect(),
                    ),
                    span: match_span.clone(),
                }
            };
            self.match_cases(subject, match_span)?
        } else {
            self.block()?
        };
        Ok(Expr {
            kind: ExprKind::Function {
                parameters,
                body: Box::new(body),
            },
            span,
        })
    }
    fn recur(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        let delimiter = self.consume(&TokenKind::LParen, "expected (")?;
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
        Ok(Expr {
            kind: ExprKind::Recur(arguments),
            span,
        })
    }
    fn match_expression(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        let subject = self.expression()?;
        self.match_cases(subject, span)
    }
    fn match_cases(&mut self, subject: Expr, span: SourceSpan) -> Result<Expr, SourceError> {
        let delimiter = self.consume(&TokenKind::LBrace, "expected { after match subject")?;
        self.enter_nesting(delimiter.span)?;
        let mut cases = Vec::new();
        self.separators();
        while !self.matches(&TokenKind::RBrace) {
            if self.matches(&TokenKind::End) {
                return Err(SourceError::at("expected }", self.peek().span.clone()));
            }
            let pattern = self.pattern()?;
            let guard = if self.matches(&TokenKind::If) {
                self.next();
                Some(self.expression()?)
            } else {
                None
            };
            let case_span = self
                .consume(&TokenKind::Arrow, "expected => after match pattern")?
                .span;
            let value = self.statement()?;
            cases.push(MatchCase {
                pattern,
                guard,
                value,
                span: case_span,
            });
            if !matches!(self.kind(), TokenKind::RBrace | TokenKind::Sep) {
                return Err(SourceError::at(
                    "expected match case separator",
                    self.peek().span.clone(),
                ));
            }
            self.separators();
        }
        self.next();
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::Match {
                subject: Box::new(subject),
                cases,
            },
            span,
        })
    }
    fn pattern(&mut self) -> Result<Pattern, SourceError> {
        let token = self.next();
        match token.kind {
            TokenKind::Int(value) => Ok(Pattern::Literal(Value::Int(value))),
            TokenKind::Str(value) => Ok(Pattern::Literal(Value::string(value))),
            TokenKind::True => Ok(Pattern::Literal(Value::Bool(true))),
            TokenKind::False => Ok(Pattern::Literal(Value::Bool(false))),
            TokenKind::Nil => Ok(Pattern::Literal(Value::Nil)),
            TokenKind::Name(name) if name == "_" => Ok(Pattern::Wildcard),
            TokenKind::Name(name) => Ok(Pattern::Binding(name)),
            TokenKind::LBracket => self.list_pattern(&token.span),
            TokenKind::LBrace => self.map_pattern(&token.span),
            TokenKind::LExactMap => self.exact_map_pattern(&token.span),
            _ => Err(SourceError::at("expected match pattern", token.span)),
        }
    }
    fn list_pattern(&mut self, span: &SourceSpan) -> Result<Pattern, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut items = Vec::new();
        let mut rest = None;
        if !self.matches(&TokenKind::RBracket) {
            loop {
                if self.matches(&TokenKind::Ellipsis) {
                    self.next();
                    let token = self.next();
                    let TokenKind::Name(name) = token.kind else {
                        return Err(SourceError::at("expected list spread binding", token.span));
                    };
                    rest = Some(name);
                    if !self.matches(&TokenKind::RBracket) {
                        return Err(SourceError::at(
                            "list spread pattern must be final",
                            self.peek().span.clone(),
                        ));
                    }
                    break;
                }
                items.push(self.pattern()?);
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
        Ok(Pattern::List { items, rest })
    }
    fn map_pattern(&mut self, span: &SourceSpan) -> Result<Pattern, SourceError> {
        self.map_pattern_with_mode(span, false)
    }
    fn exact_map_pattern(&mut self, span: &SourceSpan) -> Result<Pattern, SourceError> {
        self.map_pattern_with_mode(span, true)
    }
    fn map_pattern_with_mode(
        &mut self,
        span: &SourceSpan,
        exact: bool,
    ) -> Result<Pattern, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut entries = Vec::new();
        let mut rest = None;
        let closing = if exact {
            TokenKind::RExactMap
        } else {
            TokenKind::RBrace
        };
        if !self.matches(&closing) {
            loop {
                if self.matches(&TokenKind::Ellipsis) {
                    if exact {
                        return Err(SourceError::at(
                            "exact map patterns cannot capture a rest map",
                            self.peek().span.clone(),
                        ));
                    }
                    self.next();
                    let token = self.next();
                    let TokenKind::Name(name) = token.kind else {
                        return Err(SourceError::at("expected map spread binding", token.span));
                    };
                    rest = Some(name);
                    if !self.matches(&closing) {
                        return Err(SourceError::at(
                            "map spread pattern must be final",
                            self.peek().span.clone(),
                        ));
                    }
                    break;
                }
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected map pattern key", token.span));
                };
                let pattern = if self.matches(&TokenKind::Colon) {
                    self.next();
                    self.pattern()?
                } else {
                    Pattern::Binding(name.clone())
                };
                entries.push((name, pattern));
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.next();
                if self.matches(&closing) {
                    break;
                }
            }
        }
        self.consume(&closing, if exact { "expected |}" } else { "expected }" })?;
        self.leave_nesting();
        Ok(Pattern::Map {
            entries,
            rest,
            exact,
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
            if let ExprKind::Declare {
                mutable, pattern, ..
            } = &expression.kind
            {
                for name in pattern_bindings(pattern, &expression.span)? {
                    self.globals.insert(name, *mutable);
                }
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
                pattern,
                value,
            } => {
                self.expression(state, value)?;
                Self::bind_pattern(state, pattern, *mutable, &expression.span)?;
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
                self.tail_expression(state, value)?;
                state.emit(Op::Return, &expression.span);
            }
            ExprKind::Throw { value } => {
                self.expression(state, value)?;
                state.emit(Op::Throw, &expression.span);
            }
            ExprKind::Defer {
                value,
                mode,
                error_name,
            } => {
                let deferred = Expr {
                    kind: ExprKind::Function {
                        parameters: error_name.iter().cloned().collect(),
                        body: value.clone(),
                    },
                    span: expression.span.clone(),
                };
                self.expression(state, &deferred)?;
                state.emit(Op::Defer { mode: *mode }, &expression.span);
                state.emit(Op::Nil, &expression.span);
            }
            ExprKind::Recur(_) => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "recur is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                return Err(SourceError::semantic(
                    "recur is only valid in tail position",
                    expression.span.clone(),
                ));
            }
            ExprKind::Match { subject, cases } => {
                self.compile_match(state, subject, cases, false)?;
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
                state.emit(Op::EnterScope, &expression.span);
                for (index, value) in values.iter().enumerate() {
                    self.expression(state, value)?;
                    if index + 1 < values.len() {
                        state.emit(Op::Pop, &value.span);
                    }
                }
                if values.is_empty() {
                    state.emit(Op::Nil, &expression.span);
                }
                state.emit(Op::LeaveScope, &expression.span);
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
    fn compile_match(
        &mut self,
        state: &mut State,
        subject: &Expr,
        cases: &[MatchCase],
        tail: bool,
    ) -> Result<(), SourceError> {
        self.expression(state, subject)?;
        let mut ends = Vec::new();
        for case in cases {
            let names = pattern_bindings(&case.pattern, &case.span)?;
            state.emit(Op::Duplicate, &case.span);
            state.emit(
                Op::TryMatch {
                    pattern: lower_pattern(&case.pattern),
                    bindings: names.len(),
                },
                &case.span,
            );
            state.enter_scope();
            let slots = names
                .into_iter()
                .map(|name| state.declare(name, false))
                .collect::<Vec<_>>();
            let next = state.jump_if_false(&case.span);
            state.emit(Op::Pop, &case.span);
            for slot in slots.iter().rev() {
                state.emit(Op::SetLocal(*slot), &case.span);
            }
            state.emit(Op::Pop, &case.span);
            let guard_next = if let Some(guard) = &case.guard {
                self.expression(state, guard)?;
                Some(state.jump_if_false(&case.span))
            } else {
                None
            };
            if guard_next.is_some() {
                state.emit(Op::Pop, &case.span);
            }
            if tail {
                self.tail_expression(state, &case.value)?;
            } else {
                self.expression(state, &case.value)?;
            }
            state.leave_scope();
            ends.push(state.jump(&case.span));
            state.patch(next);
            state.emit(Op::Pop, &case.span);
            for _ in slots {
                state.emit(Op::Pop, &case.span);
            }
            if let Some(guard_next) = guard_next {
                let skip_guard_cleanup = state.jump(&case.span);
                state.patch(guard_next);
                state.emit(Op::Pop, &case.span);
                state.patch(skip_guard_cleanup);
            }
        }
        state.emit(Op::Pop, &subject.span);
        state.emit(Op::Nil, &subject.span);
        for end in ends {
            state.patch(end);
        }
        Ok(())
    }
    fn bind_pattern(
        state: &mut State,
        pattern: &Pattern,
        mutable: bool,
        span: &SourceSpan,
    ) -> Result<(), SourceError> {
        let names = pattern_bindings(pattern, span)?;
        state.emit(
            Op::TryMatch {
                pattern: lower_pattern(pattern),
                bindings: names.len(),
            },
            span,
        );
        let failed = state.jump_if_false(span);
        state.emit(Op::Pop, span);
        let bindings = names
            .into_iter()
            .map(|name| {
                let binding = if state.is_root() {
                    Binding::Global { mutable }
                } else {
                    Binding::Local {
                        slot: state.declare(name.clone(), mutable),
                        mutable,
                    }
                };
                (name, binding)
            })
            .collect::<Vec<_>>();
        for (name, binding) in bindings.iter().rev() {
            match binding {
                Binding::Global { .. } => state.emit(Op::DefineGlobal(name.clone()), span),
                Binding::Local { slot, .. } => state.emit(Op::SetLocal(*slot), span),
                Binding::Capture { .. } => unreachable!("new bindings cannot be captures"),
            }
        }
        let end = state.jump(span);
        state.patch(failed);
        state.emit(Op::Pop, span);
        for _ in &bindings {
            state.emit(Op::Pop, span);
        }
        state.emit(Op::MatchFailure, span);
        state.patch(end);
        Ok(())
    }
    fn tail_expression(&mut self, state: &mut State, expression: &Expr) -> Result<(), SourceError> {
        match &expression.kind {
            ExprKind::Recur(arguments) => {
                if !state.allows_return() {
                    return Err(SourceError::semantic(
                        "recur is only valid inside a function",
                        expression.span.clone(),
                    ));
                }
                if arguments.len() != state.arity() {
                    return Err(SourceError::semantic(
                        format!(
                            "recur expects {} arguments, got {}",
                            state.arity(),
                            arguments.len()
                        ),
                        expression.span.clone(),
                    ));
                }
                for argument in arguments {
                    self.expression(state, argument)?;
                }
                state.emit(Op::Recur(arguments.len()), &expression.span);
            }
            ExprKind::Block(values) => {
                state.enter_scope();
                state.emit(Op::EnterScope, &expression.span);
                for (index, value) in values.iter().enumerate() {
                    if index + 1 == values.len() {
                        self.tail_expression(state, value)?;
                    } else {
                        self.expression(state, value)?;
                        state.emit(Op::Pop, &value.span);
                    }
                }
                if values.is_empty() {
                    state.emit(Op::Nil, &expression.span);
                }
                state.emit(Op::LeaveScope, &expression.span);
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
                self.tail_expression(state, then_branch)?;
                let end = state.jump(&expression.span);
                state.patch(otherwise);
                state.emit(Op::Pop, &expression.span);
                if let Some(otherwise) = else_branch {
                    self.tail_expression(state, otherwise)?;
                } else {
                    state.emit(Op::Nil, &expression.span);
                }
                state.patch(end);
            }
            ExprKind::Match { subject, cases } => {
                self.compile_match(state, subject, cases, true)?;
            }
            _ => self.expression(state, expression)?,
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
        self.tail_expression(&mut state, body)?;
        state.emit(Op::Return, &body.span);
        let captures = state.captures.clone();
        Ok((state.finish("<fn>", parameters.len()), captures))
    }
}

fn lower_pattern(pattern: &Pattern) -> MatchPattern {
    match pattern {
        Pattern::Literal(value) => MatchPattern::Literal(value.clone()),
        Pattern::Wildcard => MatchPattern::Wildcard,
        Pattern::Binding(_) => MatchPattern::Binding,
        Pattern::List { items, rest } => MatchPattern::List {
            items: items.iter().map(lower_pattern).collect(),
            rest: rest.is_some(),
        },
        Pattern::Map {
            entries,
            rest,
            exact,
        } => MatchPattern::Map {
            entries: entries
                .iter()
                .map(|(key, pattern)| (key.clone(), lower_pattern(pattern)))
                .collect(),
            rest: rest.is_some(),
            exact: *exact,
        },
    }
}

fn pattern_bindings(pattern: &Pattern, span: &SourceSpan) -> Result<Vec<String>, SourceError> {
    fn collect(pattern: &Pattern, names: &mut Vec<String>) {
        match pattern {
            Pattern::Binding(name) => names.push(name.clone()),
            Pattern::List { items, rest } => {
                for item in items {
                    collect(item, names);
                }
                if let Some(name) = rest {
                    names.push(name.clone());
                }
            }
            Pattern::Map { entries, rest, .. } => {
                for (_, pattern) in entries {
                    collect(pattern, names);
                }
                if let Some(name) = rest {
                    names.push(name.clone());
                }
            }
            Pattern::Literal(_) | Pattern::Wildcard => {}
        }
    }

    let mut names = Vec::new();
    collect(pattern, &mut names);
    let mut seen = HashSet::new();
    if let Some(name) = names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(SourceError::semantic(
            format!("duplicate match binding `{name}`"),
            span.clone(),
        ));
    }
    Ok(names)
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
    fn arity(&self) -> usize {
        self.chunk.arity
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
