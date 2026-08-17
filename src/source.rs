use crate::{Chunk, Op, Program, Value};

#[derive(Debug)]
pub struct SourceError(pub String);

/// Compiles the supported initial Slug source subset into a VM program.
///
/// # Errors
///
/// Returns a source error for invalid tokens or unsupported syntax.
pub fn compile(_path: &str, source: &str) -> Result<Program, SourceError> {
    let tokens = Lexer::new(source).tokens()?;
    let expressions = Parser::new(tokens).parse()?;
    let mut chunk = Chunk::new("main", 0);
    let count = expressions.len();
    for (index, expression) in expressions.iter().enumerate() {
        emit(&mut chunk, expression)?;
        if index + 1 < count {
            chunk.emit(Op::Pop);
        }
    }
    if count == 0 {
        chunk.emit(Op::Nil);
    }
    chunk.emit(Op::Return);
    let mut program = Program::new();
    program.add_chunk(chunk);
    Ok(program)
}

#[derive(Clone, Debug)]
enum Expr {
    Value(Value),
    Name(String),
    Declare(String, Box<Expr>),
    Assign(String, Box<Expr>),
    Binary(Box<Expr>, char, Box<Expr>),
    Neg(Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
}
fn emit(chunk: &mut Chunk, expression: &Expr) -> Result<(), SourceError> {
    match expression {
        Expr::Value(value) => {
            let constant = chunk.constant(value.clone());
            chunk.emit(Op::Constant(constant));
        }
        Expr::Name(name) => {
            chunk.emit(Op::GetGlobal(name.clone()));
        }
        Expr::Declare(name, value) => {
            emit(chunk, value)?;
            chunk.emit(Op::DefineGlobal(name.clone())).emit(Op::Nil);
        }
        Expr::Assign(name, value) => {
            emit(chunk, value)?;
            chunk.emit(Op::SetGlobal(name.clone())).emit(Op::Nil);
        }
        Expr::Neg(value) => {
            emit(chunk, value)?;
            chunk.emit(Op::Negate);
        }
        Expr::Binary(left, operator, right) => {
            emit(chunk, left)?;
            emit(chunk, right)?;
            chunk.emit(match operator {
                '+' => Op::Add,
                '-' => Op::Subtract,
                '*' => Op::Multiply,
                '/' => Op::Divide,
                _ => return Err(SourceError("unknown operator".into())),
            });
        }
        Expr::Call(callee, arguments) => {
            emit(chunk, callee)?;
            for argument in arguments {
                emit(chunk, argument)?;
            }
            chunk.emit(Op::Call(arguments.len()));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Int(i64),
    Str(String),
    Name(String),
    Val,
    Var,
    True,
    False,
    Nil,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    LParen,
    RParen,
    Comma,
    Sep,
    End,
}
struct Lexer<'a> {
    input: std::iter::Peekable<std::str::Chars<'a>>,
}
impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
        }
    }
    fn tokens(mut self) -> Result<Vec<Token>, SourceError> {
        let mut result = Vec::new();
        while let Some(c) = self.input.next() {
            match c {
                ' ' | '\t' | '\r' => {}
                '\n' | ';' => result.push(Token::Sep),
                '#' => while self.input.next().is_some_and(|x| x != '\n') {},
                '+' => result.push(Token::Plus),
                '-' => result.push(Token::Minus),
                '*' => result.push(Token::Star),
                '/' => result.push(Token::Slash),
                '=' => result.push(Token::Eq),
                '(' => result.push(Token::LParen),
                ')' => result.push(Token::RParen),
                ',' => result.push(Token::Comma),
                '0'..='9' => {
                    let mut text = c.to_string();
                    while self
                        .input
                        .peek()
                        .is_some_and(|x| x.is_ascii_digit() || *x == '_')
                    {
                        text.push(self.input.next().unwrap());
                    }
                    result.push(Token::Int(
                        text.replace('_', "")
                            .parse()
                            .map_err(|_| SourceError("invalid number".into()))?,
                    ));
                }
                '"' => {
                    let mut text = String::new();
                    loop {
                        match self.input.next() {
                            Some('"') => break,
                            Some('\\') => match self.input.next() {
                                Some('n') => text.push('\n'),
                                Some('"') => text.push('"'),
                                Some('\\') => text.push('\\'),
                                _ => return Err(SourceError("invalid string escape".into())),
                            },
                            Some(x) => text.push(x),
                            None => return Err(SourceError("unterminated string".into())),
                        }
                    }
                    result.push(Token::Str(text));
                }
                x if x == '_' || x.is_alphabetic() => {
                    let mut text = x.to_string();
                    while self
                        .input
                        .peek()
                        .is_some_and(|x| *x == '_' || x.is_alphanumeric())
                    {
                        text.push(self.input.next().unwrap());
                    }
                    result.push(match text.as_str() {
                        "val" => Token::Val,
                        "var" => Token::Var,
                        "true" => Token::True,
                        "false" => Token::False,
                        "nil" => Token::Nil,
                        _ => Token::Name(text),
                    });
                }
                x => return Err(SourceError(format!("unexpected character `{x}`"))),
            }
        }
        result.push(Token::End);
        Ok(result)
    }
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
}
impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        self.index += 1;
        t
    }
    fn parse(&mut self) -> Result<Vec<Expr>, SourceError> {
        let mut values = Vec::new();
        while matches!(self.peek(), Token::Sep) {
            self.next();
        }
        while !matches!(self.peek(), Token::End) {
            values.push(self.statement()?);
            if !matches!(self.peek(), Token::End | Token::Sep) {
                return Err(SourceError("expected statement separator".into()));
            }
            while matches!(self.peek(), Token::Sep) {
                self.next();
            }
        }
        Ok(values)
    }
    fn statement(&mut self) -> Result<Expr, SourceError> {
        match self.peek() {
            Token::Val | Token::Var => {
                self.next();
                let Token::Name(name) = self.next() else {
                    return Err(SourceError("expected binding name".into()));
                };
                if !matches!(self.next(), Token::Eq) {
                    return Err(SourceError("expected =".into()));
                }
                Ok(Expr::Declare(name, Box::new(self.expr()?)))
            }
            Token::Name(name) if matches!(self.tokens.get(self.index + 1), Some(Token::Eq)) => {
                let name = name.clone();
                self.next();
                self.next();
                Ok(Expr::Assign(name, Box::new(self.expr()?)))
            }
            _ => self.expr(),
        }
    }
    fn expr(&mut self) -> Result<Expr, SourceError> {
        self.binary(0)
    }
    fn binary(&mut self, min: u8) -> Result<Expr, SourceError> {
        let mut left = self.prefix()?;
        loop {
            let (op, p) = match self.peek() {
                Token::Plus => ('+', 1),
                Token::Minus => ('-', 1),
                Token::Star => ('*', 2),
                Token::Slash => ('/', 2),
                _ => break,
            };
            if p < min {
                break;
            }
            self.next();
            let right = self.binary(p + 1)?;
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }
    fn prefix(&mut self) -> Result<Expr, SourceError> {
        if matches!(self.peek(), Token::Minus) {
            self.next();
            return Ok(Expr::Neg(Box::new(self.prefix()?)));
        }
        let mut value = match self.next() {
            Token::Int(x) => Expr::Value(Value::Int(x)),
            Token::Str(x) => Expr::Value(Value::string(x)),
            Token::True => Expr::Value(Value::Bool(true)),
            Token::False => Expr::Value(Value::Bool(false)),
            Token::Nil => Expr::Value(Value::Nil),
            Token::Name(x) => Expr::Name(x),
            Token::LParen => {
                let x = self.expr()?;
                if !matches!(self.next(), Token::RParen) {
                    return Err(SourceError("expected )".into()));
                }
                x
            }
            _ => return Err(SourceError("expected expression".into())),
        };
        while matches!(self.peek(), Token::LParen) {
            self.next();
            let mut args = Vec::new();
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    args.push(self.expr()?);
                    if !matches!(self.peek(), Token::Comma) {
                        break;
                    }
                    self.next();
                }
            }
            if !matches!(self.next(), Token::RParen) {
                return Err(SourceError("expected )".into()));
            }
            value = Expr::Call(Box::new(value), args);
        }
        Ok(value)
    }
}
