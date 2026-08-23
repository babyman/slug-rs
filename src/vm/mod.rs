use std::{cmp::Ordering, collections::HashMap, rc::Rc};

use crate::{
    CallArgumentKind, Capture, Program, SourceSpan, Value,
    bytecode::Op,
    value::{BindingCell, Closure, binding_cell},
};

mod cleanup;
mod error;
mod operations;

use cleanup::{Cleanup, Deferred};
pub use error::{CallFrame, RuntimeError, RuntimeErrorKind};
use operations::{
    add, bit_not, bitwise, construct_struct, copy_struct, divide, index_value, is_map_key,
    list_append, list_prepend, matches_pattern, modulo, multiply, negate, numbers, shift,
    slice_value, subtract,
};

pub type VmResult<T> = Result<T, RuntimeError>;

type NamedArgument = (String, Value);
type ExpandedCallArguments = (Vec<Value>, Vec<NamedArgument>);

#[derive(Clone)]
struct Frame {
    closure: Rc<Closure>,
    function: String,
    call_span: Option<SourceSpan>,
    ip: usize,
    stack_base: usize,
    locals: Vec<BindingCell>,
    provided: Vec<bool>,
    scopes: Vec<Vec<Deferred>>,
    cleanup_action: bool,
    cleanup_recovers: bool,
}

/// A small, checked stack VM for compiler-produced Slug bytecode.
#[derive(Default)]
pub struct Vm {
    globals: HashMap<String, Value>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    cleanup: Vec<Cleanup>,
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
        self.cleanup.clear();
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
            provided: vec![false; chunk.arity],
            scopes: vec![Vec::new()],
            cleanup_action: false,
            cleanup_recovers: false,
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
            match self.execute_raw(program) {
                Ok(value) => return Ok(value),
                Err(error) if self.frames.is_empty() => return Err(error),
                Err(error) => {
                    self.begin_error(error);
                    if let Some(value) = self.drive_cleanup(program)? {
                        return Ok(value);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn execute_raw(&mut self, program: &Program) -> VmResult<Value> {
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
                Op::Interpolate(parts) => {
                    let values = self.pop_values(parts.len().saturating_sub(1), span.clone())?;
                    let mut output = String::new();
                    for (index, text) in parts.into_iter().enumerate() {
                        output.push_str(&text);
                        if let Some(value) = values.get(index) {
                            output.push_str(&value.to_string());
                        }
                    }
                    self.stack.push(Value::string(output));
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
                Op::BitAnd => {
                    self.binary(span, |left, right| bitwise(left, right, |a, b| a & b))?;
                }
                Op::BitOr => self.binary(span, |left, right| bitwise(left, right, |a, b| a | b))?,
                Op::BitXor => {
                    self.binary(span, |left, right| bitwise(left, right, |a, b| a ^ b))?;
                }
                Op::ShiftLeft => {
                    self.binary(span, |left, right| shift(left, right, i64::checked_shl))?;
                }
                Op::ShiftRight => {
                    self.binary(span, |left, right| shift(left, right, i64::checked_shr))?;
                }
                Op::ListAppend => self.binary(span, |list, value| {
                    list_append(list, value).map_err(|message| (RuntimeErrorKind::Type, message))
                })?,
                Op::ListPrepend => self.binary(span, |value, list| {
                    list_prepend(value, list).map_err(|message| (RuntimeErrorKind::Type, message))
                })?,
                Op::List(count) => {
                    let values = self.pop_values(count, span.clone())?;
                    self.stack.push(Value::List(Rc::new(values)));
                }
                Op::ListSpread(spreads) => self.list_spread(spreads, span)?,
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
                Op::StructSchema(fields) => {
                    let default_count = fields.iter().filter(|field| field.has_default).count();
                    let defaults = self.pop_values(default_count, span.clone())?;
                    let mut defaults = defaults.into_iter();
                    let mut names = Vec::with_capacity(fields.len());
                    let mut schema_fields = Vec::with_capacity(fields.len());
                    for field in fields {
                        if names.contains(&field.name) {
                            return Err(self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("duplicate struct schema field '{}'", field.name),
                                span,
                            ));
                        }
                        names.push(field.name.clone());
                        schema_fields.push(crate::StructField {
                            name: field.name.into(),
                            default: field.has_default.then(|| {
                                defaults
                                    .next()
                                    .expect("default count was derived from field metadata")
                            }),
                        });
                    }
                    self.stack
                        .push(Value::StructSchema(Rc::new(crate::StructSchema {
                            fields: schema_fields,
                        })));
                }
                Op::Struct(fields) => {
                    let values = self.pop_values(fields.len(), span.clone())?;
                    let schema = self.pop(span.clone())?;
                    self.stack.push(
                        construct_struct(schema, &fields, &values)
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::StructCopy(fields) => {
                    let replacements = self.pop_values(fields.len(), span.clone())?;
                    let value = self.pop(span.clone())?;
                    self.stack.push(
                        copy_struct(value, &fields, &replacements)
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
                }
                Op::GetIndex => {
                    let (collection, index) = self.pop_pair(span.clone())?;
                    self.stack
                        .push(index_value(collection, &index).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
                }
                Op::GetSlice {
                    has_start,
                    has_end,
                    has_step,
                } => {
                    let count =
                        usize::from(has_start) + usize::from(has_end) + usize::from(has_step);
                    let mut values = self.pop_values(count + 1, span.clone())?.into_iter();
                    let collection = values
                        .next()
                        .expect("slice operation includes a collection");
                    let start = has_start.then(|| values.next().expect("slice start is present"));
                    let end = has_end.then(|| values.next().expect("slice end is present"));
                    let step = has_step.then(|| values.next().expect("slice step is present"));
                    self.stack.push(
                        slice_value(collection, start.as_ref(), end.as_ref(), step.as_ref())
                            .map_err(|message| self.error(RuntimeErrorKind::Type, message, span))?,
                    );
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
                Op::BitNot => {
                    let value = self.pop(span.clone())?;
                    self.stack
                        .push(bit_not(&value).map_err(|message| {
                            self.error(RuntimeErrorKind::Type, message, span)
                        })?);
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
                Op::JumpIfProvided { slot, target } => {
                    if self
                        .frames
                        .last()
                        .and_then(|frame| frame.provided.get(slot))
                        .copied()
                        == Some(true)
                    {
                        self.jump(target, span)?;
                    }
                }
                Op::Call(count) => self.call(program, count, None, span)?,
                Op::CallSpread(kinds) => self.call_spread(program, kinds, span)?,
                Op::PipelineCall(kinds) => self.pipeline_call(program, kinds, span)?,
                Op::TryMatch {
                    pattern,
                    bindings,
                    operands,
                } => {
                    let operands = self.pop_values(operands, span.clone())?;
                    let value = self.pop(span.clone())?;
                    let mut values = Vec::new();
                    let matched = matches_pattern(&pattern, &value, &operands, &mut values)
                        .map_err(|(kind, message)| self.error(kind, message, span.clone()))?;
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
                Op::Throw => {
                    let value = self.pop(span.clone())?;
                    return Err(self.thrown(value, span));
                }
                Op::EnterScope => self.current_scopes(span)?.push(Vec::new()),
                Op::LeaveScope => {
                    let actions = self.current_scopes(span.clone())?.pop().ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active scope".into(),
                            span.clone(),
                        )
                    })?;
                    if self.frames.last().is_some_and(|frame| frame.cleanup_action) {
                        self.cleanup.push(Cleanup::Resume);
                    }
                    self.cleanup.push(Cleanup::Actions {
                        actions,
                        success: true,
                        frame_depth: self.frames.len() - 1,
                    });
                    if let Some(value) = self.drive_cleanup(program)? {
                        return Ok(value);
                    }
                }
                Op::Defer { mode } => {
                    let action = self.pop(span.clone())?;
                    if !matches!(action, Value::Closure(_) | Value::Native { .. }) {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            "defer expects a callable action".into(),
                            span,
                        ));
                    }
                    let Some(frame) = self.frames.last_mut() else {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active call frame".into(),
                            span,
                        ));
                    };
                    let Some(scope) = frame.scopes.last_mut() else {
                        return Err(self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "no active scope".into(),
                            None,
                        ));
                    };
                    scope.push(Deferred { action, mode });
                }
                Op::Recur(kinds) => self.recur(program, kinds, span)?,
                Op::Return => {
                    let value = self.pop(span.clone())?;
                    if let Some(value) = self.begin_return(program, value)? {
                        return Ok(value);
                    }
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

    fn call(
        &mut self,
        program: &Program,
        count: usize,
        provided: Option<Vec<bool>>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
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
                    provided: provided.unwrap_or_else(|| vec![true; chunk.arity]),
                    scopes: vec![Vec::new()],
                    cleanup_action: false,
                    cleanup_recovers: false,
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

    fn list_spread(&mut self, spreads: Vec<bool>, span: Option<SourceSpan>) -> VmResult<()> {
        let values = self.pop_values(spreads.len(), span.clone())?;
        let mut result = Vec::new();
        for (value, spread) in values.into_iter().zip(spreads) {
            if spread {
                let Value::List(values) = value else {
                    return Err(self.error(
                        RuntimeErrorKind::Type,
                        "list spread expects a list".into(),
                        span,
                    ));
                };
                result.extend(values.iter().cloned());
            } else {
                result.push(value);
            }
        }
        self.stack.push(Value::List(Rc::new(result)));
        Ok(())
    }

    fn call_spread(
        &mut self,
        program: &Program,
        kinds: Vec<CallArgumentKind>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let required = kinds.len().checked_add(1).ok_or_else(|| {
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
        let values = self.stack.split_off(base + 1);
        self.stack.truncate(base);
        let (positional, named) = self.expand_call_arguments(values, kinds, span.clone())?;
        let (arguments, provided) =
            self.bind_call_arguments(program, &callee, positional, named, span.clone())?;
        self.stack.push(callee);
        self.stack.extend(arguments);
        let count = self.stack.len() - base - 1;
        self.call(program, count, Some(provided), span)
    }

    fn pipeline_call(
        &mut self,
        program: &Program,
        kinds: Vec<CallArgumentKind>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let required = kinds.len().checked_add(2).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline argument count is too large".into(),
                span.clone(),
            )
        })?;
        let base = self.stack.len().checked_sub(required).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        let arguments = self.stack.split_off(base + 2);
        let callee = self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        let value = self.stack.pop().ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "pipeline has too few stack values".into(),
                span.clone(),
            )
        })?;
        self.stack.push(callee);
        self.stack.push(value);
        self.stack.extend(arguments);
        let mut kinds = kinds;
        kinds.insert(0, CallArgumentKind::Positional);
        self.call_spread(program, kinds, span)
    }

    fn expand_call_arguments(
        &self,
        values: Vec<Value>,
        kinds: Vec<CallArgumentKind>,
        span: Option<SourceSpan>,
    ) -> VmResult<ExpandedCallArguments> {
        let mut positional = Vec::new();
        let mut named = Vec::new();
        for (value, kind) in values.into_iter().zip(kinds) {
            match kind {
                CallArgumentKind::Positional => positional.push(value),
                CallArgumentKind::Spread => {
                    let Value::List(values) = value else {
                        return Err(self.error(
                            RuntimeErrorKind::Type,
                            "call spread expects a list".into(),
                            span,
                        ));
                    };
                    positional.extend(values.iter().cloned());
                }
                CallArgumentKind::Named(name) => named.push((name, value)),
            }
        }
        Ok((positional, named))
    }

    fn bind_call_arguments(
        &self,
        program: &Program,
        callee: &Value,
        mut positional: Vec<Value>,
        named: Vec<(String, Value)>,
        span: Option<SourceSpan>,
    ) -> VmResult<(Vec<Value>, Vec<bool>)> {
        let Value::Closure(closure) = callee else {
            if named.is_empty() {
                return Ok((positional.clone(), vec![true; positional.len()]));
            }
            return Err(self.error(
                RuntimeErrorKind::Arity,
                "native functions do not accept named arguments".into(),
                span,
            ));
        };
        let chunk = program.chunk(closure.chunk).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "closure references missing chunk".into(),
                span.clone(),
            )
        })?;
        if chunk.parameters.is_empty() {
            return Ok((positional.clone(), vec![true; positional.len()]));
        }
        let variadic = chunk
            .parameters
            .last()
            .filter(|parameter| parameter.variadic);
        let fixed = chunk.parameters.len() - usize::from(variadic.is_some());
        if positional.len() > chunk.parameters.len() && variadic.is_none() {
            return Err(self.error(
                RuntimeErrorKind::Arity,
                format!("`{}` received too many positional arguments", chunk.name),
                span,
            ));
        }
        let rest = positional.split_off(fixed.min(positional.len()));
        let mut bound = positional.into_iter().map(Some).collect::<Vec<_>>();
        bound.resize_with(chunk.parameters.len(), || None);
        for (name, value) in named {
            let slot = chunk
                .parameters
                .iter()
                .position(|parameter| parameter.name == name)
                .ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::Name,
                        format!("unknown parameter `{name}`"),
                        span.clone(),
                    )
                })?;
            if bound[slot].is_some() {
                return Err(self.error(
                    RuntimeErrorKind::Arity,
                    format!("parameter `{name}` was assigned more than once"),
                    span,
                ));
            }
            if chunk.parameters[slot].variadic && !matches!(value, Value::List(_)) {
                return Err(self.error(
                    RuntimeErrorKind::Type,
                    format!("variadic parameter `{name}` expects a list"),
                    span,
                ));
            }
            bound[slot] = Some(value);
        }
        if variadic.is_some() && bound[fixed].is_none() {
            bound[fixed] = Some(Value::List(Rc::new(rest)));
        }
        let provided = bound.iter().map(Option::is_some).collect::<Vec<_>>();
        let values = bound
            .into_iter()
            .enumerate()
            .map(|(slot, value)| {
                value
                    .or_else(|| chunk.parameters[slot].has_default.then_some(Value::Nil))
                    .ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::Arity,
                            format!(
                                "missing required parameter `{}`",
                                chunk.parameters[slot].name
                            ),
                            span.clone(),
                        )
                    })
            })
            .collect::<VmResult<Vec<_>>>()?;
        Ok((values, provided))
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
}
