use super::{
    SourceError,
    ast::{Binary, Expr, ExprKind, MatchCase, Pattern, Prefix, RestPattern, Token, TokenKind},
};
use crate::{DeferMode, SourceSpan, Value};

/// Stateful parser for the source front end.
pub(super) struct Parser {
    tokens: Vec<Token>,
    index: usize,
    nesting: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
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
        if matches!(self.kind(), TokenKind::Throw) {
            let span = self.next().span;
            let value = self.expression()?;
            return Ok(Expr {
                span,
                kind: ExprKind::Throw {
                    value: Box::new(value),
                },
            });
        }
        if matches!(self.kind(), TokenKind::Defer) {
            let span = self.next().span;
            let (mode, error_name) = if self.matches(&TokenKind::Onsuccess) {
                self.next();
                (DeferMode::Success, None)
            } else if self.matches(&TokenKind::Onerror) {
                self.next();
                self.consume(&TokenKind::LParen, "expected ( after onerror")?;
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected error binding name", token.span));
                };
                self.consume(&TokenKind::RParen, "expected ) after error binding")?;
                (DeferMode::Error, Some(name))
            } else {
                (DeferMode::Always, None)
            };
            let value = self.expression()?;
            return Ok(Expr {
                span,
                kind: ExprKind::Defer {
                    value: Box::new(value),
                    mode,
                    error_name,
                },
            });
        }
        if matches!(self.kind(), TokenKind::Val | TokenKind::Var) {
            let mutable = matches!(self.next().kind, TokenKind::Var);
            if self.matches(&TokenKind::Eq) {
                return Err(SourceError::at(
                    "expected binding name",
                    self.peek().span.clone(),
                ));
            }
            let pattern = self.pattern()?;
            self.consume(&TokenKind::Eq, "expected =")?;
            let value = self.expression()?;
            return Ok(Expr {
                span: value.span.clone(),
                kind: ExprKind::Declare {
                    mutable,
                    pattern,
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
            let mut patterns = vec![self.pattern()?];
            while self.matches(&TokenKind::Comma) {
                self.next();
                patterns.push(self.pattern()?);
            }
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
                patterns,
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
            TokenKind::Name(name) if self.matches(&TokenKind::At) => {
                self.next();
                self.enter_nesting(token.span.clone())?;
                let pattern = self.pattern();
                self.leave_nesting();
                Ok(Pattern::At {
                    name,
                    pattern: Box::new(pattern?),
                })
            }
            TokenKind::Name(name) => Ok(Pattern::Binding(name)),
            TokenKind::Caret => {
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected pinned binding name", token.span));
                };
                Ok(Pattern::Pinned(name))
            }
            TokenKind::LBracket => self.list_pattern(&token.span),
            TokenKind::LBrace => self.map_pattern(&token.span),
            TokenKind::LExactMap => self.exact_map_pattern(&token.span),
            _ => Err(SourceError::at("expected match pattern", token.span)),
        }
    }
    fn rest_pattern(&mut self) -> RestPattern {
        self.next();
        if matches!(self.kind(), TokenKind::Name(_)) {
            let TokenKind::Name(name) = self.next().kind else {
                unreachable!("name token was checked");
            };
            RestPattern::Binding(name)
        } else {
            RestPattern::Discard
        }
    }
    fn list_pattern(&mut self, span: &SourceSpan) -> Result<Pattern, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut items = Vec::new();
        let mut rest = None;
        if !self.matches(&TokenKind::RBracket) {
            loop {
                if self.matches(&TokenKind::Ellipsis) {
                    rest = Some(self.rest_pattern());
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
                            "exact map patterns cannot contain a spread pattern",
                            self.peek().span.clone(),
                        ));
                    }
                    rest = Some(self.rest_pattern());
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
