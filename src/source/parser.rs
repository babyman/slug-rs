use super::{ast::{Token, TokenKind}, SourceError};
use crate::SourceSpan;

/// Stateful source parser. Grammar methods remain with the front-end during
/// the staged extraction so parsing behavior is unchanged.
pub(super) struct Parser {
    pub(super) tokens: Vec<Token>,
    pub(super) index: usize,
    pub(super) nesting: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0, nesting: 0 }
    }

    pub(super) fn peek(&self) -> &Token { &self.tokens[self.index] }
    pub(super) fn kind(&self) -> &TokenKind { &self.peek().kind }
    pub(super) fn next(&mut self) -> Token { let token = self.peek().clone(); self.index += 1; token }
    pub(super) fn matches(&self, kind: &TokenKind) -> bool { std::mem::discriminant(self.kind()) == std::mem::discriminant(kind) }
    pub(super) fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<Token, SourceError> {
        if self.matches(kind) { Ok(self.next()) } else { Err(SourceError::at(message, self.peek().span.clone())) }
    }
    pub(super) fn separators(&mut self) { while self.matches(&TokenKind::Sep) { self.next(); } }
    pub(super) fn enter_nesting(&mut self, span: SourceSpan) -> Result<(), SourceError> {
        if self.nesting == super::MAX_PARSE_NESTING { return Err(SourceError::at("source nesting limit exceeded", span)); }
        self.nesting += 1; Ok(())
    }
    pub(super) fn leave_nesting(&mut self) { self.nesting -= 1; }
}
