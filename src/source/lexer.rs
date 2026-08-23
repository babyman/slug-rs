/// Stateful scanner for the source front end.
use crate::SourceSpan;

use super::{
    SourceError,
    ast::{StringPart, Token, TokenKind},
};

pub(super) struct Lexer {
    path: String,
    input: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
}

impl Lexer {
    fn current_line_is_blank(&self) -> bool {
        self.input[..self.index]
            .iter()
            .rev()
            .take_while(|value| **value != '\n')
            .all(|value| matches!(value, ' ' | '\t' | '\r'))
    }

    fn documentation(&mut self, span: SourceSpan) -> Result<String, SourceError> {
        let mut content = String::new();
        loop {
            let value = self
                .next()
                .ok_or_else(|| SourceError::at("unterminated documentation block", span.clone()))?;
            if value == '*' && self.peek() == Some('/') {
                self.next();
                break;
            }
            content.push(value);
        }
        if content
            .lines()
            .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('*'))
        {
            return Err(SourceError::at(
                "every non-empty documentation line must begin with *",
                span,
            ));
        }
        Ok(content)
    }

    fn string(
        &mut self,
        delimiter: char,
        raw: bool,
        span: SourceSpan,
    ) -> Result<Vec<StringPart>, SourceError> {
        self.next();
        let triple =
            self.peek() == Some(delimiter) && self.input.get(self.index + 1) == Some(&delimiter);
        if triple {
            self.next();
            self.next();
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            if self.peek() == Some(delimiter)
                && (!triple
                    || (self.input.get(self.index + 1) == Some(&delimiter)
                        && self.input.get(self.index + 2) == Some(&delimiter)))
            {
                self.next();
                if triple {
                    self.next();
                    self.next();
                }
                break;
            }
            let value = self
                .next()
                .ok_or_else(|| SourceError::at("unterminated string", span.clone()))?;
            if !raw
                && value == '$'
                && self
                    .peek()
                    .is_some_and(|value| value == '_' || value.is_alphabetic())
            {
                if !text.is_empty() {
                    parts.push(StringPart::Text(std::mem::take(&mut text)));
                }
                let mut name = String::new();
                while self
                    .peek()
                    .is_some_and(|value| value == '_' || value.is_alphanumeric())
                {
                    name.push(self.next().expect("peeked character exists"));
                }
                parts.push(StringPart::Name(name));
            } else if !raw && value == '\\' {
                match self.next() {
                    Some('n') => text.push('\n'),
                    Some('r') => text.push('\r'),
                    Some('t') => text.push('\t'),
                    Some('"') => text.push('"'),
                    Some('\\') => text.push('\\'),
                    Some('$') => text.push('$'),
                    Some(first @ '0'..='7') => {
                        let mut digits = String::from(first);
                        for _ in 0..2 {
                            if let Some(value @ '0'..='7') = self.peek() {
                                digits.push(value);
                                self.next();
                            } else {
                                break;
                            }
                        }
                        let value = u32::from_str_radix(&digits, 8)
                            .expect("one to three octal digits always parse");
                        text.push(char::from_u32(value).expect("three octal digits fit in char"));
                    }
                    Some(value) => {
                        text.push('\\');
                        text.push(value);
                    }
                    None => return Err(SourceError::at("unterminated string", span)),
                }
            } else {
                text.push(value);
            }
        }
        if triple {
            if text.starts_with('\n') {
                text.remove(0);
            }
            if text.ends_with('\n') {
                text.pop();
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(StringPart::Text(text));
        }
        Ok(parts)
    }

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
                    | TokenKind::Ampersand
                    | TokenKind::Pipe
                    | TokenKind::ShiftLeft
                    | TokenKind::ShiftRight
                    | TokenKind::Plus
                    | TokenKind::PlusColon
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Percent
                    | TokenKind::Colon
                    | TokenKind::ColonPlus
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
        !matches!(
            (self.input.get(index), self.input.get(index + 1)),
            (Some('/'), Some('/' | '*'))
        ) && matches!(
            self.input.get(index),
            Some('+' | '-' | '*' | '/' | '%' | ':' | '<' | '>' | '=' | '!' | '&' | '|' | '^' | '.',)
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
                    let blank = self.current_line_is_blank();
                    self.next();
                    if delimiters == 0 && !self.newline_continues(&result) {
                        Self::push(
                            &mut result,
                            if blank {
                                TokenKind::BlankSep
                            } else {
                                TokenKind::Sep
                            },
                            span,
                        );
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
                    let kind = if self.peek() == Some(':') {
                        self.next();
                        TokenKind::PlusColon
                    } else {
                        TokenKind::Plus
                    };
                    Self::push(&mut result, kind, span);
                }
                '-' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Minus, span);
                }
                '*' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Star, span);
                }
                '%' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Percent, span);
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
                            if self.peek() == Some('*') {
                                self.next();
                                let documentation = self.documentation(span.clone())?;
                                Self::push(
                                    &mut result,
                                    TokenKind::Documentation(documentation),
                                    span,
                                );
                            } else {
                                let mut closed = false;
                                while let Some(value) = self.next() {
                                    if value == '*' && self.peek() == Some('/') {
                                        self.next();
                                        closed = true;
                                        break;
                                    }
                                }
                                if !closed {
                                    return Err(SourceError::at(
                                        "unterminated block comment",
                                        span,
                                    ));
                                }
                            }
                        }
                        Some('>') => {
                            self.next();
                            Self::push(&mut result, TokenKind::Pipeline, span);
                        }
                        _ => Self::push(&mut result, TokenKind::Slash, span),
                    }
                }
                '&' => {
                    self.next();
                    let kind = if self.peek() == Some('&') {
                        self.next();
                        TokenKind::AndAnd
                    } else {
                        TokenKind::Ampersand
                    };
                    Self::push(&mut result, kind, span);
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
                        TokenKind::Pipe
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
                    let kind = if self.peek() == Some('+') {
                        self.next();
                        TokenKind::ColonPlus
                    } else {
                        TokenKind::Colon
                    };
                    Self::push(&mut result, kind, span);
                }
                '@' => {
                    self.next();
                    Self::push(&mut result, TokenKind::At, span);
                }
                '?' => {
                    self.next();
                    if self.peek() == Some('?') && self.input.get(self.index + 1) == Some(&'?') {
                        self.next();
                        self.next();
                        Self::push(&mut result, TokenKind::NotImplemented, span);
                    } else {
                        return Err(SourceError::at("expected ???", span));
                    }
                }
                '^' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Caret, span);
                }
                '~' => {
                    self.next();
                    Self::push(&mut result, TokenKind::Tilde, span);
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
                    } else if self.peek() == Some('<') {
                        self.next();
                        TokenKind::ShiftLeft
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
                    } else if self.peek() == Some('>') {
                        self.next();
                        TokenKind::ShiftRight
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
                    if text == "0" && self.peek() == Some('x') {
                        self.next();
                        if self.peek() == Some('"') {
                            self.next();
                            let mut digits = String::new();
                            loop {
                                match self.next() {
                                    Some('"') => break,
                                    Some(value) if value.is_ascii_hexdigit() => digits.push(value),
                                    Some(_) => {
                                        return Err(SourceError::at(
                                            "invalid hexadecimal digit in byte literal",
                                            span,
                                        ));
                                    }
                                    None => {
                                        return Err(SourceError::at(
                                            "unterminated byte literal",
                                            span,
                                        ));
                                    }
                                }
                            }
                            if digits.is_empty() || !digits.len().is_multiple_of(2) {
                                return Err(SourceError::at(
                                    "byte literal must contain one or more complete hexadecimal byte pairs",
                                    span,
                                ));
                            }
                            let bytes = (0..digits.len())
                                .step_by(2)
                                .map(|index| u8::from_str_radix(&digits[index..index + 2], 16))
                                .collect::<Result<Vec<_>, _>>()
                                .map_err(|_| {
                                    SourceError::at("invalid byte literal", span.clone())
                                })?;
                            Self::push(&mut result, TokenKind::Bytes(bytes), span);
                            continue;
                        }

                        if self.peek() == Some('_') {
                            self.next();
                        }
                        let mut digits = String::new();
                        while self
                            .peek()
                            .is_some_and(|value| value.is_ascii_hexdigit() || value == '_')
                        {
                            let value = self.next().expect("peeked character exists");
                            if value != '_' {
                                digits.push(value);
                            }
                        }
                        if digits.is_empty() {
                            return Err(SourceError::at("expected hexadecimal digit", span));
                        }
                        let value = i64::from_str_radix(&digits, 16).map_err(|_| {
                            SourceError::at("invalid hexadecimal number", span.clone())
                        })?;
                        Self::push(&mut result, TokenKind::Int(value), span);
                        continue;
                    }

                    let mut float = false;
                    if self.peek() == Some('.')
                        && self
                            .input
                            .get(self.index + 1)
                            .is_some_and(char::is_ascii_digit)
                    {
                        float = true;
                        text.push(self.next().expect("decimal point exists"));
                        while self
                            .peek()
                            .is_some_and(|value| value.is_ascii_digit() || value == '_')
                        {
                            text.push(self.next().expect("peeked character exists"));
                        }
                    }
                    if matches!(self.peek(), Some('e' | 'E')) {
                        float = true;
                        text.push(self.next().expect("exponent marker exists"));
                        if matches!(self.peek(), Some('+' | '-')) {
                            text.push(self.next().expect("exponent sign exists"));
                        }
                        if !self.peek().is_some_and(|value| value.is_ascii_digit()) {
                            return Err(SourceError::at("expected exponent digit", span));
                        }
                        while self.peek().is_some_and(|value| value.is_ascii_digit()) {
                            text.push(self.next().expect("peeked character exists"));
                        }
                    }
                    let text = text.replace('_', "");
                    if float {
                        let value = text
                            .parse()
                            .map_err(|_| SourceError::at("invalid number", span.clone()))?;
                        Self::push(&mut result, TokenKind::Float(value), span);
                    } else {
                        let value = text
                            .parse()
                            .map_err(|_| SourceError::at("invalid number", span.clone()))?;
                        Self::push(&mut result, TokenKind::Int(value), span);
                    }
                }
                '"' => Self::push(
                    &mut result,
                    TokenKind::Interpolated(self.string('"', false, span.clone())?),
                    span,
                ),
                '\'' => Self::push(
                    &mut result,
                    TokenKind::Interpolated(self.string('\'', true, span.clone())?),
                    span,
                ),
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
                        "export" => TokenKind::Export,
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
                        "struct" => TokenKind::Struct,
                        "copy" => TokenKind::Copy,
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
