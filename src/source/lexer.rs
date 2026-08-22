/// Stateful scanner for the source front end.
use crate::SourceSpan;

use super::{
    SourceError,
    ast::{Token, TokenKind},
};

pub(super) struct Lexer {
    path: String,
    input: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
}

impl Lexer {
    pub(super) fn new(path: &str, input: &str) -> Self {
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
                    | TokenKind::At
                    | TokenKind::Caret
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
            Some('+' | '-' | '*' | '/' | '<' | '>' | '=' | '!' | '&' | '|' | '@' | '^' | '.',)
        )
    }
    fn push(tokens: &mut Vec<Token>, kind: TokenKind, span: SourceSpan) {
        tokens.push(Token { kind, span });
    }
    #[allow(clippy::too_many_lines)]
    pub(super) fn tokens(mut self) -> Result<Vec<Token>, SourceError> {
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
                '@' => {
                    self.next();
                    Self::push(&mut result, TokenKind::At, span);
                }
                '^' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Caret, span);
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
