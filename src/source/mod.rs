//! The Rust source front end. Its AST and bytecode lowering are deliberately
//! private while the language surface is still growing.

use std::{collections::HashSet, fmt};

use crate::{MatchPattern, Program, SourceSpan, Value};

mod ast;
mod compiler;
mod lexer;
mod parser;
mod state;
use ast::{Binary, Expr, ExprKind, MatchCase, Pattern, Prefix, Token, TokenKind};
use compiler::Compiler;
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
