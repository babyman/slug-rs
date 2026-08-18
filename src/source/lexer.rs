/// Stateful source scanner. Tokenization methods remain in the parent module
/// during this first extraction step so lexer behavior stays unchanged.
use super::ast::{Token, TokenKind};
use crate::SourceSpan;

pub(super) struct Lexer {
    pub(super) path: String,
    pub(super) input: Vec<char>,
    pub(super) index: usize,
    pub(super) line: u32,
    pub(super) column: u32,
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

    pub(super) fn span(&self) -> SourceSpan {
        SourceSpan::new(self.path.clone(), self.line, self.column)
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.input.get(self.index).copied()
    }

    pub(super) fn next(&mut self) -> Option<char> {
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

    pub(super) fn newline_continues(&self, tokens: &[Token]) -> bool {
        matches!(
            tokens.last().map(|token| &token.kind),
            Some(
                TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::EqEq
                    | TokenKind::BangEq
                    | TokenKind::Less
                    | TokenKind::LessEq
                    | TokenKind::Greater
                    | TokenKind::GreaterEq
                    | TokenKind::AndAnd
                    | TokenKind::OrOr
                    | TokenKind::Eq
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Arrow
            )
        ) || self.next_starts_infix()
    }

    fn next_starts_infix(&self) -> bool {
        matches!(
            (self.peek(), self.input.get(self.index + 1).copied()),
            (Some('&'), Some('&')) | (Some('|'), Some('|')) | (Some('='), Some('='))
                | (Some('!'), Some('=')) | (Some('<'), Some('=')) | (Some('>'), Some('='))
        ) || matches!(self.peek(), Some('+' | '-' | '*' | '/' | '<' | '>'))
    }

    pub(super) fn push(tokens: &mut Vec<Token>, kind: TokenKind, span: SourceSpan) {
        tokens.push(Token { kind, span });
    }
}
