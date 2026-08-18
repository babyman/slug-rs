use std::{cmp::Ordering, collections::HashMap, fmt, rc::Rc};

use crate::{
    Capture, MatchPattern, Program, SourceSpan, Value,
    bytecode::Op,
    value::{BindingCell, Closure, binding_cell},
};

pub type VmResult<T> = Result<T, RuntimeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeErrorKind {
    InvalidBytecode,
    Type,
    Name,
    Arity,
    DivideByZero,
    InvalidCall,
    Native,
    Match,
}

/// A Slug-level runtime error, never a host panic.
#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub frames: Vec<CallFrame>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallFrame {
    pub function: String,
    pub span: Option<SourceSpan>,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "runtime error: {}", self.message)?;
        if let Some(span) = &self.span {
            write!(f, " at {}:{}:{}", span.path, span.line, span.column)?;
        }
        for frame in &self.frames {
            write!(f, "\n  in {}", frame.function)?;
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone)]
struct Frame {
    closure: Rc<Closure>,
    function: String,
    call_span: Option<SourceSpan>,
    ip: usize,
    stack_base: usize,
    locals: Vec<BindingCell>,
}

/// A small, checked stack VM for compiler-produced Slug bytecode.
#[derive(Default)]
pub struct Vm {
    globals: HashMap<String, Value>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
}

impl Vm {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn define_native(&mut self, name: impl Into<String>, function: crate::NativeFunction) {
        let name = name.into();
        self.globals.insert(
            name.clone(),
            Value::Native {
                name: Rc::from(name),
                function,
            },
        );
    }

    /// Executes a zero-argument entry chunk.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is invalid or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run(&mut self, program: &Program, entry: usize) -> VmResult<Value> {
        let chunk = program.chunk(entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("entry chunk {entry} does not exist"),
                None,
            )
        })?;
        if chunk.arity != 0 {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!(
                    "entry function `{}` expects {} arguments",
                    chunk.name, chunk.arity
                ),
                None,
            ));
        }
        if chunk.locals < chunk.arity {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!(
                    "entry function `{}` has {} local slots for {} parameters",
                    chunk.name, chunk.locals, chunk.arity
                ),
                None,
            ));
        }
        self.stack.clear();
        self.frames.clear();
        self.frames.push(Frame {
            closure: Rc::new(Closure {
                chunk: entry,
                captures: Vec::new(),
            }),
            function: chunk.name.clone(),
            call_span: None,
            ip: 0,
            stack_base: 0,
            locals: (0..chunk.locals)
                .map(|_| binding_cell(Value::Nil))
                .collect(),
        });
        self.execute(program)
    }

    /// Executes a zero-argument chunk selected by name.
    ///
    /// # Errors
    ///
    /// Returns a Slug runtime error when the entry is absent or evaluation
    /// encounters invalid bytecode or a language-level runtime fault.
    pub fn run_named(&mut self, program: &Program, entry: &str) -> VmResult<Value> {
        let index = program.find_chunk(entry).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::Name,
                format!("unknown entry `{entry}`"),
                None,
            )
        })?;
        self.run(program, index)
    }

    #[allow(clippy::too_many_lines)]
    fn execute(&mut self, program: &Program) -> VmResult<Value> {
        loop {
            let (op, span) = self.next_instruction(program)?;
            match op {
                Op::Constant(index) => {
                    let chunk = self.current_chunk(program)?;
                    let value = match chunk.constants.get(index) {
                        Some(crate::Constant::Value(value)) => value.clone(),
                        Some(crate::Constant::Function(function)) => {
                            Value::Closure(Rc::new(Closure {
                                chunk: *function,
                                captures: Vec::new(),
                            }))
                        }
                        None => {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("constant {index} does not exist"),
                                span,
                            ));
                        }
                    };
                    self.stack.push(value);
                }
                Op::Nil => self.stack.push(Value::Nil),
                Op::True => self.stack.push(Value::Bool(true)),
                Op::False => self.stack.push(Value::Bool(false)),
                Op::Pop => {
                    self.pop(span)?;
                }
                Op::Duplicate => self.stack.push(self.peek(span)?.clone()),
                Op::GetLocal(slot) => self.stack.push(self.local(slot, span)?.borrow().clone()),
                Op::SetLocal(slot) => {
                    let value = self.pop(span.clone())?;
                    self.set_local(slot, value, span)?;
                }
                Op::GetCapture(slot) => {
                    let value = self
                        .frames
                        .last()
                        .and_then(|frame| frame.closure.captures.get(slot))
                        .map(|cell| cell.borrow().clone())
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("capture {slot} does not exist"),
                                span.clone(),
                            )
                        })?;
                    self.stack.push(value);
                }
                Op::SetCapture(slot) => {
                    let value = self.pop(span.clone())?;
                    let capture = self
                        .frames
                        .last()
                        .and_then(|frame| frame.closure.captures.get(slot))
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("capture {slot} does not exist"),
                                span.clone(),
                            )
                        })?;
                    *capture.borrow_mut() = value;
                }
                Op::GetGlobal(name) => {
                    let value = self.globals.get(&name).cloned().ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            span.clone(),
                        )
                    })?;
                    self.stack.push(value);
                }
                Op::DefineGlobal(name) => {
                    let value = self.pop(span.clone())?;
                    self.globals.insert(name, value);
                }
                Op::SetGlobal(name) => {
                    if !self.globals.contains_key(&name) {
                        return Err(self.error(
                            RuntimeErrorKind::Name,
                            format!("unknown name `{name}`"),
                            span,
                        ));
                    }
                    let value = self.pop(None)?;
                    self.globals.insert(name, value);
                }
                Op::MakeClosure { chunk, captures } => {
                    program.chunk(chunk).ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            format!("function chunk {chunk} does not exist"),
                            span.clone(),
                        )
                    })?;
                    let captures = captures
                        .into_iter()
                        .map(|capture| match capture {
                            Capture::Local(slot) => self.local(slot, span.clone()),
                            Capture::Capture(slot) => self
                                .frames
                                .last()
                                .and_then(|frame| frame.closure.captures.get(slot))
                                .cloned()
                                .ok_or_else(|| {
                                    self.error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        format!("capture {slot} does not exist"),
                                        span.clone(),
                                    )
                                }),
                        })
                        .collect::<VmResult<Vec<_>>>()?;
                    self.stack
                        .push(Value::Closure(Rc::new(Closure { chunk, captures })));
                }
                Op::Add => self.binary(span, add)?,
                Op::Subtract => self.binary(span, subtract)?,
                Op::Multiply => self.binary(span, multiply)?,
                Op::Divide => self.binary(span, divide)?,
                Op::Modulo => self.binary(span, modulo)?,
                Op::List(count) => {
                    let values = self.pop_values(count, span.clone())?;
                    self.stack.push(Value::List(Rc::new(values)));
                }
                Op::Map(count) => {
                    let values = self.pop_values(count.saturating_mul(2), span.clone())?;
                    let mut entries = Vec::with_capacity(count);
                    for pair in values.chunks_exact(2) {
                        if !is_map_key(&pair[0]) {
                            return Err(self.error(
                                RuntimeErrorKind::Type,
                                format!("{} cannot be used as a map key", pair[0].type_name()),
                                span,
                            ));
                        }
                        entries.push((pair[0].clone(), pair[1].clone()));
                    }
                    self.stack.push(Value::Map(Rc::new(entries)));
                }
                Op::GetIndex => {
                    let (collection, index) = self.pop_pair(span.clone())?;
                    self.stack
                        .push(index_value(collection, &index).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::Negate => {
                    let value = self.pop(span.clone())?;
                    self.stack
                        .push(negate(value).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::Not => {
                    let value = self.pop(span)?;
                    self.stack.push(Value::Bool(!value.is_truthy()));
                }
                Op::Equal => {
                    let (left, right) = self.pop_pair(span.clone())?;
                    self.stack.push(Value::Bool(left == right));
                }
                Op::Greater => self.compare(span, Ordering::Greater)?,
                Op::Less => self.compare(span, Ordering::Less)?,
                Op::Jump(target) => self.jump(target, span)?,
                Op::JumpIfFalse(target) => {
                    if !self.peek(span.clone())?.is_truthy() {
                        self.jump(target, span)?;
                    }
                }
                Op::Call(count) => self.call(program, count, span)?,
                Op::TryMatch { pattern, bindings } => {
                    let value = self.pop(span.clone())?;
                    let mut values = Vec::new();
                    let matched = matches_pattern(&pattern, &value, &mut values);
                    if matched && values.len() != bindings {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "match pattern binding count is invalid".into(),
                            span,
                        ));
                    }
                    if matched {
                        self.stack.extend(values);
                    } else {
                        self.stack.extend((0..bindings).map(|_| Value::Nil));
                    }
                    self.stack.push(Value::Bool(matched));
                }
                Op::MatchFailure => {
                    return Err(self.error(
                        RuntimeErrorKind::Match,
                        "destructuring pattern did not match".into(),
                        span,
                    ));
                }
                Op::Recur(count) => self.recur(program, count, span)?,
                Op::Return => {
                    let value = self.pop(span.clone())?;
                    let frame = self
                        .frames
                        .pop()
                        .expect("VM always has a frame while executing");
                    self.stack.truncate(frame.stack_base);
                    if self.frames.is_empty() {
                        return Ok(value);
                    }
                    self.stack.push(value);
                }
            }
        }
    }

    fn next_instruction(&mut self, program: &Program) -> VmResult<(Op, Option<SourceSpan>)> {
        let (chunk_index, ip) = self
            .frames
            .last()
            .map(|frame| (frame.closure.chunk, frame.ip))
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    None,
                )
            })?;
        let chunk = program.chunk(chunk_index).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "active chunk does not exist".into(),
                None,
            )
        })?;
        let instruction = chunk
            .code
            .get(ip)
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("function `{}` ended without Return", chunk.name),
                    None,
                )
            })?
            .clone();
        self.frames.last_mut().expect("active frame was checked").ip += 1;
        Ok((instruction.op, instruction.span))
    }

    fn current_chunk<'a>(&self, program: &'a Program) -> VmResult<&'a crate::Chunk> {
        let chunk = self
            .frames
            .last()
            .map(|frame| frame.closure.chunk)
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    None,
                )
            })?;
        program.chunk(chunk).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "active chunk does not exist".into(),
                None,
            )
        })
    }

    fn call(&mut self, program: &Program, count: usize, span: Option<SourceSpan>) -> VmResult<()> {
        let required = count.checked_add(1).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "call argument count is too large".into(),
                span.clone(),
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "call has too few stack values".into(),
                span.clone(),
            )
        })?;
        let callee = self.stack[base].clone();
        match callee {
            Value::Closure(closure) => {
                let chunk = program.chunk(closure.chunk).ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        "closure references missing chunk".into(),
                        span.clone(),
                    )
                })?;
                if chunk.arity != count {
                    return Err(self.error(
                        RuntimeErrorKind::Arity,
                        format!(
                            "`{}` expects {} arguments, got {count}",
                            chunk.name, chunk.arity
                        ),
                        span,
                    ));
                }
                if chunk.locals < chunk.arity {
                    return Err(self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        format!(
                            "function `{}` has {} local slots for {} parameters",
                            chunk.name, chunk.locals, chunk.arity
                        ),
                        span,
                    ));
                }
                let mut locals = self.stack[base + 1..]
                    .iter()
                    .cloned()
                    .map(binding_cell)
                    .collect::<Vec<_>>();
                locals.resize_with(chunk.locals, || binding_cell(Value::Nil));
                self.frames.push(Frame {
                    closure,
                    function: chunk.name.clone(),
                    call_span: span,
                    ip: 0,
                    stack_base: base,
                    locals,
                });
            }
            Value::Native { name, function } => {
                let arguments = self.stack[base + 1..].to_vec();
                let result = function(&arguments).map_err(|message| {
                    self.error(
                        RuntimeErrorKind::Native,
                        format!("native `{name}`: {message}"),
                        span,
                    )
                })?;
                self.stack.truncate(base);
                self.stack.push(result);
            }
            value => {
                return Err(self.error(
                    RuntimeErrorKind::InvalidCall,
                    format!("cannot call {}", value.type_name()),
                    span,
                ));
            }
        }
        Ok(())
    }

    fn recur(&mut self, program: &Program, count: usize, span: Option<SourceSpan>) -> VmResult<()> {
        let arity = self.current_chunk(program)?.arity;
        if count != arity {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("recur expects {arity} arguments, got {count}"),
                span,
            ));
        }
        let (_, local_count, stack_base) = self
            .frames
            .last()
            .map(|frame| (frame.closure.chunk, frame.locals.len(), frame.stack_base))
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    span.clone(),
                )
            })?;
        if local_count < arity {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("active function has {local_count} local slots for {arity} parameters"),
                span,
            ));
        }
        let arguments = self.pop_values(count, span.clone())?;
        self.stack.truncate(stack_base);
        let mut locals = arguments.into_iter().map(binding_cell).collect::<Vec<_>>();
        locals.resize_with(local_count, || binding_cell(Value::Nil));
        let frame = self.frames.last_mut().expect("active frame was checked");
        frame.locals = locals;
        frame.ip = 0;
        Ok(())
    }

    fn binary(
        &mut self,
        span: Option<SourceSpan>,
        operation: fn(Value, Value) -> Result<Value, (RuntimeErrorKind, String)>,
    ) -> VmResult<()> {
        let (left, right) = self.pop_pair(span.clone())?;
        self.stack.push(
            operation(left, right).map_err(|(kind, message)| self.error(kind, message, span))?,
        );
        Ok(())
    }
    fn compare(&mut self, span: Option<SourceSpan>, expected: Ordering) -> VmResult<()> {
        let (left, right) = self.pop_pair(span.clone())?;
        let result = if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
            left.cmp(right) == expected
        } else {
            let (left, right) = numbers(left, right)
                .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?;
            left.partial_cmp(&right)
                .is_some_and(|ordering| ordering == expected)
        };
        self.stack.push(Value::Bool(result));
        Ok(())
    }
    fn pop_pair(&mut self, span: Option<SourceSpan>) -> VmResult<(Value, Value)> {
        let right = self.pop(span.clone())?;
        let left = self.pop(span)?;
        Ok((left, right))
    }
    fn pop_values(&mut self, count: usize, span: Option<SourceSpan>) -> VmResult<Vec<Value>> {
        if self.stack.len() < count {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            ));
        }
        Ok(self.stack.split_off(self.stack.len() - count))
    }
    fn pop(&mut self, span: Option<SourceSpan>) -> VmResult<Value> {
        self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }
    fn peek(&self, span: Option<SourceSpan>) -> VmResult<&Value> {
        self.stack.last().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "stack underflow".into(),
                span,
            )
        })
    }
    fn local(&self, slot: usize, span: Option<SourceSpan>) -> VmResult<BindingCell> {
        self.frames
            .last()
            .and_then(|frame| frame.locals.get(slot))
            .cloned()
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("local {slot} does not exist"),
                    span,
                )
            })
    }
    fn set_local(&mut self, slot: usize, value: Value, span: Option<SourceSpan>) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        if self
            .frames
            .last()
            .is_none_or(|frame| slot >= frame.locals.len())
        {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                format!("local {slot} does not exist"),
                None,
            ));
        }
        *self
            .frames
            .last_mut()
            .expect("active frame was checked")
            .locals[slot]
            .borrow_mut() = value;
        Ok(())
    }
    fn jump(&mut self, target: usize, span: Option<SourceSpan>) -> VmResult<()> {
        if self.frames.last().is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        self.frames.last_mut().expect("active frame was checked").ip = target;
        Ok(())
    }
    #[allow(clippy::needless_pass_by_value)]
    fn error(
        &self,
        kind: RuntimeErrorKind,
        message: String,
        span: Option<SourceSpan>,
    ) -> RuntimeError {
        RuntimeError {
            kind,
            message,
            span: span.clone(),
            frames: self
                .frames
                .iter()
                .rev()
                .map(|frame| CallFrame {
                    function: frame.function.clone(),
                    span: frame.call_span.clone(),
                })
                .collect(),
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn numbers(left: Value, right: Value) -> Result<(f64, f64), String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok((a as f64, b as f64)),
        (Value::Int(a), Value::Float(b)) => Ok((a as f64, b)),
        (Value::Float(a), Value::Int(b)) => Ok((a, b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok((a, b)),
        (a, b) => Err(format!(
            "expected numbers, got {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}
fn add(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a
            .checked_add(b)
            .map(Value::Int)
            .ok_or((RuntimeErrorKind::Type, "integer overflow".into())),
        (Value::Str(a), Value::Str(b)) => Ok(Value::string(format!("{a}{b}"))),
        (a, b) => {
            let (a, b) = numbers(a, b).map_err(|message| (RuntimeErrorKind::Type, message))?;
            Ok(Value::Float(a + b))
        }
    }
}
fn subtract(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    integer_or_float(left, right, i64::checked_sub, |a, b| a - b)
}
fn multiply(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    integer_or_float(left, right, i64::checked_mul, |a, b| a * b)
}
fn divide(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        if *right == 0 {
            return Err((RuntimeErrorKind::DivideByZero, "division by zero".into()));
        }
        if let Some(value) = left.checked_div(*right) {
            if left % right == 0 {
                return Ok(Value::Int(value));
            }
        } else {
            return Err((RuntimeErrorKind::Type, "integer overflow".into()));
        }
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    if b == 0.0 {
        Err((RuntimeErrorKind::DivideByZero, "division by zero".into()))
    } else {
        Ok(Value::Float(a / b))
    }
}
fn modulo(left: Value, right: Value) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        return left.checked_rem(*right).map(Value::Int).ok_or_else(|| {
            if *right == 0 {
                (RuntimeErrorKind::DivideByZero, "division by zero".into())
            } else {
                (RuntimeErrorKind::Type, "integer overflow".into())
            }
        });
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    if b == 0.0 {
        Err((RuntimeErrorKind::DivideByZero, "division by zero".into()))
    } else {
        Ok(Value::Float(a % b))
    }
}

fn matches_pattern(pattern: &MatchPattern, value: &Value, bindings: &mut Vec<Value>) -> bool {
    match pattern {
        MatchPattern::Literal(expected) => value == expected,
        MatchPattern::Wildcard => true,
        MatchPattern::Binding => {
            bindings.push(value.clone());
            true
        }
        MatchPattern::List { items, rest } => {
            let Value::List(values) = value else {
                return false;
            };
            if values.len() < items.len() || (!rest && values.len() != items.len()) {
                return false;
            }
            let binding_start = bindings.len();
            for (item, value) in items.iter().zip(values.iter()) {
                if !matches_pattern(item, value, bindings) {
                    bindings.truncate(binding_start);
                    return false;
                }
            }
            if *rest {
                bindings.push(Value::List(Rc::new(values[items.len()..].to_vec())));
            }
            true
        }
        MatchPattern::Map(patterns) => {
            let Value::Map(entries) = value else {
                return false;
            };
            let binding_start = bindings.len();
            for (key, pattern) in patterns {
                let key = Value::string(key.clone());
                let Some((_, value)) = entries.iter().rev().find(|(entry, _)| entry == &key) else {
                    bindings.truncate(binding_start);
                    return false;
                };
                if !matches_pattern(pattern, value, bindings) {
                    bindings.truncate(binding_start);
                    return false;
                }
            }
            true
        }
    }
}

fn is_map_key(value: &Value) -> bool {
    matches!(
        value,
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Bytes(_)
    )
}

fn index_value(collection: Value, index: &Value) -> Result<Value, String> {
    match collection {
        Value::List(values) => {
            let Value::Int(index) = index else {
                return Err("list index must be an integer".into());
            };
            let length = i64::try_from(values.len()).map_err(|_| "list is too large".to_owned())?;
            let index = if *index < 0 { length + *index } else { *index };
            usize::try_from(index)
                .ok()
                .and_then(|index| values.get(index))
                .cloned()
                .ok_or_else(|| "list index is out of bounds".into())
        }
        Value::Map(entries) => Ok(entries
            .iter()
            .rev()
            .find(|(key, _)| key == index)
            .map_or(Value::Nil, |(_, value)| value.clone())),
        value => Err(format!("cannot index {}", value.type_name())),
    }
}
fn integer_or_float(
    left: Value,
    right: Value,
    integer_operation: fn(i64, i64) -> Option<i64>,
    operation: fn(f64, f64) -> f64,
) -> Result<Value, (RuntimeErrorKind, String)> {
    if let (Value::Int(left), Value::Int(right)) = (&left, &right) {
        return integer_operation(*left, *right)
            .map(Value::Int)
            .ok_or((RuntimeErrorKind::Type, "integer overflow".into()));
    }
    let (a, b) = numbers(left, right).map_err(|message| (RuntimeErrorKind::Type, message))?;
    Ok(Value::Float(operation(a, b)))
}
fn negate(value: Value) -> Result<Value, String> {
    match value {
        Value::Int(value) => value
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow".into()),
        Value::Float(value) => Ok(Value::Float(-value)),
        value => Err(format!("expected number, got {}", value.type_name())),
    }
}
