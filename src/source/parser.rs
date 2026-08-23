use super::{
    SourceError,
    ast::{
        Binary, CallArgument, Expr, ExprKind, ListElement, MapPatternKey, MatchCase, Parameter,
        Pattern, Prefix, RestPattern, StringPart, StructSchemaField, Token, TokenKind,
    },
};
use crate::{DeferMode, SourceSpan, Value};

/// Stateful parser for the source front end.
pub(super) struct Parser {
    tokens: Vec<Token>,
    index: usize,
    nesting: usize,
    match_subject_nesting: Option<usize>,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            nesting: 0,
            match_subject_nesting: None,
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
                TokenKind::Percent => (Binary::Modulo, 6),
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
                let arguments = self.call_arguments()?;
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
                value = self.index_or_slice(value, span)?;
                self.leave_nesting();
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
            } else if self.matches(&TokenKind::LBrace) && self.starts_struct_init() {
                let span = self.next().span;
                value = self.struct_init(value, span)?;
            } else if self.matches(&TokenKind::Copy) {
                let span = self.next().span;
                self.consume(&TokenKind::LBrace, "expected { after copy")?;
                value = self.struct_copy(value, span)?;
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

    fn index_or_slice(&mut self, collection: Expr, span: SourceSpan) -> Result<Expr, SourceError> {
        let start = if self.matches(&TokenKind::Colon) {
            None
        } else {
            Some(Box::new(self.expression()?))
        };
        if self.matches(&TokenKind::Colon) {
            self.next();
            let end = if self.matches(&TokenKind::Colon) || self.matches(&TokenKind::RBracket) {
                None
            } else {
                Some(Box::new(self.expression()?))
            };
            let step = if self.matches(&TokenKind::Colon) {
                self.next();
                if self.matches(&TokenKind::RBracket) {
                    None
                } else {
                    Some(Box::new(self.expression()?))
                }
            } else {
                None
            };
            self.consume(&TokenKind::RBracket, "expected ]")?;
            return Ok(Expr {
                span,
                kind: ExprKind::Slice {
                    collection: Box::new(collection),
                    start,
                    end,
                    step,
                },
            });
        }
        let Some(index) = start else {
            unreachable!("a bracket expression is either a slice or has an index")
        };
        self.consume(&TokenKind::RBracket, "expected ]")?;
        Ok(Expr {
            span,
            kind: ExprKind::Index {
                collection: Box::new(collection),
                index,
            },
        })
    }
    fn call_arguments(&mut self) -> Result<Vec<CallArgument>, SourceError> {
        let mut arguments = Vec::new();
        let mut has_named = false;
        while !self.matches(&TokenKind::RParen) {
            if self.matches(&TokenKind::Ellipsis) {
                let spread = self.next();
                if has_named {
                    return Err(SourceError::at(
                        "spread argument cannot appear after a named argument",
                        spread.span,
                    ));
                }
                arguments.push(CallArgument::Spread(self.expression()?));
            } else if let (
                TokenKind::Name(name),
                Some(Token {
                    kind: TokenKind::Eq,
                    ..
                }),
            ) = (self.kind().clone(), self.tokens.get(self.index + 1))
            {
                self.next();
                self.next();
                has_named = true;
                arguments.push(CallArgument::Named {
                    name,
                    value: self.expression()?,
                });
            } else {
                if has_named {
                    return Err(SourceError::at(
                        "positional argument cannot appear after a named argument",
                        self.peek().span.clone(),
                    ));
                }
                arguments.push(CallArgument::Positional(self.expression()?));
            }
            if !self.matches(&TokenKind::Comma) {
                break;
            }
            self.next();
        }
        Ok(arguments)
    }
    fn primary(&mut self) -> Result<Expr, SourceError> {
        let token = self.next();
        let span = token.span.clone();
        let kind = match token.kind {
            TokenKind::Int(value) => ExprKind::Value(Value::Int(value)),
            TokenKind::Float(value) => ExprKind::Value(Value::Float(value)),
            TokenKind::Bytes(value) => ExprKind::Value(Value::Bytes(value.into())),
            TokenKind::Interpolated(parts) => {
                if let [StringPart::Text(value)] = parts.as_slice() {
                    ExprKind::Value(Value::string(value.clone()))
                } else {
                    ExprKind::Interpolate(parts)
                }
            }
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
            TokenKind::Struct => return self.struct_schema(span),
            _ => return Err(SourceError::at("expected expression", span)),
        };
        Ok(Expr { kind, span })
    }
    fn starts_struct_init(&self) -> bool {
        if self.match_subject_nesting != Some(self.nesting) {
            return true;
        }
        matches!(
            (
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                self.tokens.get(self.index + 2).map(|token| &token.kind),
            ),
            (Some(TokenKind::Name(_)), Some(TokenKind::Colon))
        )
    }
    fn struct_schema(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        let delimiter = self.consume(&TokenKind::LBrace, "expected { after struct")?;
        self.enter_nesting(delimiter.span)?;
        let mut fields = Vec::new();
        self.separators();
        while !self.matches(&TokenKind::RBrace) {
            let token = self.next();
            let TokenKind::Name(name) = token.kind else {
                return Err(SourceError::at("expected struct field name", token.span));
            };
            if self.matches(&TokenKind::Colon) {
                return Err(SourceError::at(
                    "struct field type annotations are not supported",
                    self.peek().span.clone(),
                ));
            }
            let default = if self.matches(&TokenKind::Eq) {
                self.next();
                Some(self.expression()?)
            } else {
                None
            };
            fields.push(StructSchemaField { name, default });
            self.struct_field_separator()?;
        }
        self.next();
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::StructSchema(fields),
            span,
        })
    }
    fn struct_init(&mut self, schema: Expr, span: SourceSpan) -> Result<Expr, SourceError> {
        let fields = self.struct_fields(&span)?;
        Ok(Expr {
            kind: ExprKind::StructInit {
                schema: Box::new(schema),
                fields,
            },
            span,
        })
    }
    fn struct_copy(&mut self, value: Expr, span: SourceSpan) -> Result<Expr, SourceError> {
        let fields = self.struct_fields(&span)?;
        Ok(Expr {
            kind: ExprKind::StructCopy {
                value: Box::new(value),
                fields,
            },
            span,
        })
    }
    fn struct_fields(&mut self, span: &SourceSpan) -> Result<Vec<(String, Expr)>, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut fields = Vec::new();
        self.separators();
        while !self.matches(&TokenKind::RBrace) {
            let token = self.next();
            let TokenKind::Name(name) = token.kind else {
                return Err(SourceError::at("expected struct field name", token.span));
            };
            self.consume(&TokenKind::Colon, "expected : after struct field name")?;
            fields.push((name, self.expression()?));
            self.struct_field_separator()?;
        }
        self.next();
        self.leave_nesting();
        Ok(fields)
    }
    fn struct_field_separator(&mut self) -> Result<(), SourceError> {
        if self.matches(&TokenKind::Comma) {
            self.next();
            self.separators();
        } else if self.matches(&TokenKind::Sep) {
            self.separators();
            if !self.matches(&TokenKind::RBrace) {
                return Err(SourceError::at(
                    "expected , between struct fields",
                    self.peek().span.clone(),
                ));
            }
        } else if !self.matches(&TokenKind::RBrace) {
            return Err(SourceError::at(
                "expected , between struct fields",
                self.peek().span.clone(),
            ));
        }
        Ok(())
    }
    fn list(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        self.enter_nesting(span.clone())?;
        let mut values = Vec::new();
        if !self.matches(&TokenKind::RBracket) {
            loop {
                if self.matches(&TokenKind::Ellipsis) {
                    self.next();
                    values.push(ListElement::Spread(self.expression()?));
                } else {
                    values.push(ListElement::Value(self.expression()?));
                }
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
        let mut has_variadic = false;
        if !self.matches(&TokenKind::RParen) {
            loop {
                let variadic = if self.matches(&TokenKind::Ellipsis) {
                    let token = self.next();
                    if has_variadic {
                        return Err(SourceError::at(
                            "function can have only one variadic parameter",
                            token.span,
                        ));
                    }
                    true
                } else {
                    false
                };
                let token = self.next();
                let TokenKind::Name(name) = token.kind else {
                    return Err(SourceError::at("expected parameter name", token.span));
                };
                if name == "_" {
                    return Err(SourceError::at(
                        "discard parameters are not supported yet",
                        token.span,
                    ));
                }
                let default = if self.matches(&TokenKind::Eq) {
                    self.next();
                    Some(self.expression()?)
                } else {
                    None
                };
                if variadic && default.is_some() {
                    return Err(SourceError::at(
                        "variadic parameters cannot have defaults",
                        token.span,
                    ));
                }
                parameters.push(Parameter {
                    name,
                    default,
                    variadic,
                });
                has_variadic |= variadic;
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.next();
                if self.matches(&TokenKind::RParen) {
                    break;
                }
                if has_variadic {
                    return Err(SourceError::at(
                        "variadic parameter must be final",
                        self.peek().span.clone(),
                    ));
                }
            }
        }
        self.consume(&TokenKind::RParen, "expected )")?;
        let body = if self.matches(&TokenKind::Match) {
            let match_span = self.next().span;
            let subject = if parameters.len() == 1 {
                Expr {
                    kind: ExprKind::Name(parameters[0].name.clone()),
                    span: match_span.clone(),
                }
            } else {
                Expr {
                    kind: ExprKind::List(
                        parameters
                            .iter()
                            .map(|parameter| {
                                ListElement::Value(Expr {
                                    kind: ExprKind::Name(parameter.name.clone()),
                                    span: match_span.clone(),
                                })
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
        let arguments = self.call_arguments()?;
        self.consume(&TokenKind::RParen, "expected )")?;
        self.leave_nesting();
        Ok(Expr {
            kind: ExprKind::Recur(arguments),
            span,
        })
    }
    fn match_expression(&mut self, span: SourceSpan) -> Result<Expr, SourceError> {
        let previous = self.match_subject_nesting.replace(self.nesting);
        let subject = self.expression();
        self.match_subject_nesting = previous;
        let subject = subject?;
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
            TokenKind::Float(value) => Ok(Pattern::Literal(Value::Float(value))),
            TokenKind::Bytes(value) => Ok(Pattern::Literal(Value::Bytes(value.into()))),
            TokenKind::Interpolated(parts) => {
                let [StringPart::Text(value)] = parts.as_slice() else {
                    return Err(SourceError::at(
                        "interpolated strings are not match patterns",
                        token.span,
                    ));
                };
                Ok(Pattern::Literal(Value::string(value.clone())))
            }
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
                let key = if self.matches(&TokenKind::LBracket) {
                    self.next();
                    let key = self.expression()?;
                    self.consume(&TokenKind::RBracket, "expected ]")?;
                    MapPatternKey::Computed(key)
                } else {
                    let token = self.next();
                    let TokenKind::Name(name) = token.kind else {
                        return Err(SourceError::at("expected map pattern key", token.span));
                    };
                    MapPatternKey::String(name)
                };
                let pattern = if self.matches(&TokenKind::Colon) {
                    self.next();
                    self.pattern()?
                } else {
                    let MapPatternKey::String(name) = &key else {
                        return Err(SourceError::at(
                            "expected : after computed map pattern key",
                            self.peek().span.clone(),
                        ));
                    };
                    Pattern::Binding(name.clone())
                };
                entries.push((key, pattern));
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
