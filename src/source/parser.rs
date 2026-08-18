use super::{ast::{Expr, ExprKind, Token, TokenKind}, SourceError};
use crate::{DeferMode, SourceSpan};

/// Stateful source parser. Grammar methods remain with the front-end during
/// the staged extraction so parsing behavior is unchanged.
pub(super) struct Parser {
    pub(super) tokens: Vec<Token>,
    pub(super) index: usize,
    pub(super) nesting: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            nesting: 0,
        }
    }

    pub(super) fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }
    pub(super) fn kind(&self) -> &TokenKind {
        &self.peek().kind
    }
    pub(super) fn next(&mut self) -> Token {
        let token = self.peek().clone();
        self.index += 1;
        token
    }
    pub(super) fn matches(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.kind()) == std::mem::discriminant(kind)
    }
    pub(super) fn consume(
        &mut self,
        kind: &TokenKind,
        message: &str,
    ) -> Result<Token, SourceError> {
        if self.matches(kind) {
            Ok(self.next())
        } else {
            Err(SourceError::at(message, self.peek().span.clone()))
        }
    }
    pub(super) fn separators(&mut self) {
        while self.matches(&TokenKind::Sep) {
            self.next();
        }
    }
    pub(super) fn enter_nesting(&mut self, span: SourceSpan) -> Result<(), SourceError> {
        if self.nesting == super::MAX_PARSE_NESTING {
            return Err(SourceError::at("source nesting limit exceeded", span));
        }
        self.nesting += 1;
        Ok(())
    }
    pub(super) fn leave_nesting(&mut self) {
        self.nesting -= 1;
    }

    pub(super) fn parse(&mut self) -> Result<Vec<Expr>, SourceError> {
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

    pub(super) fn statement(&mut self) -> Result<Expr, SourceError> {
        if matches!(self.kind(), TokenKind::Return) {
            let span = self.next().span;
            let value = self.expression()?;
            return Ok(Expr { span, kind: ExprKind::Return { value: Box::new(value) } });
        }
        if matches!(self.kind(), TokenKind::Throw) {
            let span = self.next().span;
            let value = self.expression()?;
            return Ok(Expr { span, kind: ExprKind::Throw { value: Box::new(value) } });
        }
        if matches!(self.kind(), TokenKind::Defer) {
            let span = self.next().span;
            let (mode, error_name) = if self.matches(&TokenKind::Onsuccess) {
                self.next(); (DeferMode::Success, None)
            } else if self.matches(&TokenKind::Onerror) {
                self.next(); self.consume(&TokenKind::LParen, "expected ( after onerror")?;
                let token = self.next();
                let TokenKind::Name(name) = token.kind else { return Err(SourceError::at("expected error binding name", token.span)); };
                self.consume(&TokenKind::RParen, "expected ) after error binding")?;
                (DeferMode::Error, Some(name))
            } else { (DeferMode::Always, None) };
            let value = self.expression()?;
            return Ok(Expr { span, kind: ExprKind::Defer { value: Box::new(value), mode, error_name } });
        }
        if matches!(self.kind(), TokenKind::Val | TokenKind::Var) {
            let mutable = matches!(self.next().kind, TokenKind::Var);
            if self.matches(&TokenKind::Eq) { return Err(SourceError::at("expected binding name", self.peek().span.clone())); }
            let pattern = self.pattern()?;
            self.consume(&TokenKind::Eq, "expected =")?;
            let value = self.expression()?;
            return Ok(Expr { span: value.span.clone(), kind: ExprKind::Declare { mutable, pattern, value: Box::new(value) } });
        }
        self.expression()
    }

    pub(super) fn expression(&mut self) -> Result<Expr, SourceError> {
        if let (TokenKind::Name(name), Some(Token { kind: TokenKind::Eq, .. })) =
            (self.kind().clone(), self.tokens.get(self.index + 1))
        {
            let span = self.next().span;
            self.next();
            self.enter_nesting(span.clone())?;
            let value = self.expression()?;
            self.leave_nesting();
            return Ok(Expr { span, kind: ExprKind::Assign { name, value: Box::new(value) } });
        }
        self.binary(0)
    }

}
