use crate::{DeferMode, Program, SourceSpan, Value, value::binding_cell};

use super::{Frame, RuntimeError, RuntimeErrorKind, Vm, VmResult};

#[derive(Clone)]
pub(super) struct Deferred {
    pub(super) action: Value,
    pub(super) mode: DeferMode,
}

pub(super) enum Cleanup {
    Actions {
        actions: Vec<Deferred>,
        success: bool,
        frame_depth: usize,
    },
    Return(Value),
    Recover(Value),
    Resume,
    Recur {
        arguments: Vec<Value>,
        provided: Vec<bool>,
    },
    Error(RuntimeError),
}

impl Vm {
    pub(super) fn begin_return(
        &mut self,
        program: &Program,
        value: Value,
    ) -> VmResult<Option<Value>> {
        let frame_depth = self.frames.len().checked_sub(1).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "return cleanup has no frame".into(),
                None,
            )
        })?;
        let frame = self.frames.last_mut().expect("frame was checked");
        let scopes = std::mem::take(&mut frame.scopes);
        self.cleanup
            .push(if frame.cleanup_action && frame.cleanup_recovers {
                Cleanup::Recover(value)
            } else {
                Cleanup::Return(value)
            });
        self.cleanup
            .extend(scopes.into_iter().map(|actions| Cleanup::Actions {
                actions,
                success: true,
                frame_depth,
            }));
        self.drive_cleanup(program)
    }

    pub(super) fn recur(
        &mut self,
        program: &Program,
        kinds: Vec<crate::CallArgumentKind>,
        span: Option<SourceSpan>,
    ) -> VmResult<()> {
        let values = self.pop_values(kinds.len(), span.clone())?;
        let (positional, named) = self.expand_call_arguments(values, kinds, span.clone())?;
        let closure = self
            .frames
            .last()
            .map(|frame| crate::Value::Closure(frame.closure.clone()))
            .ok_or_else(|| {
                self.error(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    span.clone(),
                )
            })?;
        let (arguments, provided) =
            self.bind_call_arguments(program, &closure, positional, named, span.clone())?;
        let arity = self.current_chunk(program)?.arity;
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
        let nested_scopes = self
            .frames
            .last_mut()
            .expect("active frame was checked")
            .scopes
            .split_off(1);
        if !nested_scopes.is_empty() {
            self.cleanup.push(Cleanup::Recur {
                arguments,
                provided,
            });
            self.cleanup
                .extend(nested_scopes.into_iter().map(|actions| Cleanup::Actions {
                    actions,
                    success: true,
                    frame_depth: self.frames.len() - 1,
                }));
            self.drive_cleanup(program)?;
            return Ok(());
        }
        self.finish_recur(arguments, provided, local_count, stack_base);
        Ok(())
    }

    pub(super) fn finish_recur(
        &mut self,
        arguments: Vec<Value>,
        provided: Vec<bool>,
        local_count: usize,
        stack_base: usize,
    ) {
        self.stack.truncate(stack_base);
        let mut locals = arguments.into_iter().map(binding_cell).collect::<Vec<_>>();
        locals.resize_with(local_count, || binding_cell(Value::Nil));
        let frame = self.frames.last_mut().expect("active frame was checked");
        frame.locals = locals;
        frame.provided = provided;
        frame.ip = 0;
    }

    pub(super) fn current_scopes(
        &mut self,
        span: Option<SourceSpan>,
    ) -> VmResult<&mut Vec<Vec<Deferred>>> {
        if self.frames.is_empty() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "no active call frame".into(),
                span,
            ));
        }
        Ok(&mut self.frames.last_mut().expect("frame was checked").scopes)
    }

    pub(super) fn begin_error(&mut self, mut error: RuntimeError) {
        if let Some(Cleanup::Error(previous)) = self.cleanup.first() {
            error.cause = Some(Box::new(previous.clone()));
        }
        let mut cleanup = vec![Cleanup::Error(error)];
        cleanup.extend(
            self.cleanup
                .drain(..)
                .filter(|item| matches!(item, Cleanup::Actions { .. })),
        );
        for (frame_depth, frame) in self.frames.iter_mut().enumerate() {
            let scopes = std::mem::take(&mut frame.scopes);
            cleanup.extend(scopes.into_iter().map(|actions| Cleanup::Actions {
                actions,
                success: false,
                frame_depth,
            }));
        }
        self.cleanup = cleanup;
    }

    pub(super) fn active_error(&self) -> Option<RuntimeError> {
        self.cleanup.iter().find_map(|cleanup| match cleanup {
            Cleanup::Error(error) => Some(error.clone()),
            _ => None,
        })
    }

    pub(super) fn recover_from_error(
        &mut self,
        program: &Program,
        value: Value,
    ) -> VmResult<Option<Value>> {
        let frame_depth = self.frames.len().checked_sub(1).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "error cleanup has no enclosing frame".into(),
                None,
            )
        })?;
        let mut recovered = Vec::new();
        for cleanup in self.cleanup.drain(..) {
            match cleanup {
                Cleanup::Actions {
                    frame_depth: depth,
                    actions,
                    ..
                } if depth < frame_depth => self.frames[depth].scopes.push(actions),
                Cleanup::Actions {
                    frame_depth: depth,
                    actions,
                    ..
                } if depth == frame_depth => recovered.push(Cleanup::Actions {
                    actions,
                    success: true,
                    frame_depth: depth,
                }),
                _ => {}
            }
        }
        self.cleanup = recovered;
        self.cleanup.insert(0, Cleanup::Return(value));
        self.drive_cleanup(program)
    }

    pub(super) fn drive_cleanup(&mut self, program: &Program) -> VmResult<Option<Value>> {
        loop {
            match self.cleanup.last_mut() {
                Some(Cleanup::Actions {
                    actions, success, ..
                }) => match actions.pop() {
                    Some(Deferred {
                        mode: DeferMode::Success,
                        ..
                    }) if !*success => {}
                    Some(Deferred {
                        mode: DeferMode::Error,
                        ..
                    }) if *success => {}
                    Some(Deferred { action, mode }) => {
                        return self.call_cleanup(program, action, mode == DeferMode::Error);
                    }
                    None => {
                        self.cleanup.pop();
                    }
                },
                Some(Cleanup::Return(_)) => {
                    let Cleanup::Return(value) = self.cleanup.pop().expect("cleanup exists") else {
                        unreachable!();
                    };
                    let frame = self.frames.pop().ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "return cleanup has no frame".into(),
                            None,
                        )
                    })?;
                    self.stack.truncate(frame.stack_base);
                    if frame.cleanup_action {
                        continue;
                    }
                    if self.frames.is_empty() {
                        return Ok(Some(value));
                    }
                    self.stack.push(value);
                }
                Some(Cleanup::Recover(_)) => {
                    let Cleanup::Recover(value) = self.cleanup.pop().expect("cleanup exists")
                    else {
                        unreachable!();
                    };
                    let frame = self.frames.pop().ok_or_else(|| {
                        self.error(
                            RuntimeErrorKind::InvalidBytecode,
                            "recovery cleanup has no frame".into(),
                            None,
                        )
                    })?;
                    self.stack.truncate(frame.stack_base);
                    return self.recover_from_error(program, value);
                }
                Some(Cleanup::Resume) => {
                    self.cleanup.pop();
                    return Ok(None);
                }
                Some(Cleanup::Recur { .. }) => {
                    let Cleanup::Recur {
                        arguments,
                        provided,
                    } = self.cleanup.pop().expect("cleanup exists")
                    else {
                        unreachable!();
                    };
                    let (local_count, stack_base) = self
                        .frames
                        .last()
                        .map(|frame| (frame.locals.len(), frame.stack_base))
                        .ok_or_else(|| {
                            self.error(
                                RuntimeErrorKind::InvalidBytecode,
                                "recur cleanup has no frame".into(),
                                None,
                            )
                        })?;
                    self.finish_recur(arguments, provided, local_count, stack_base);
                    return Ok(None);
                }
                Some(Cleanup::Error(_)) => {
                    let Cleanup::Error(error) = self.cleanup.pop().expect("cleanup exists") else {
                        unreachable!();
                    };
                    self.frames.clear();
                    self.stack.clear();
                    return Err(error);
                }
                None => return Ok(None),
            }
        }
    }

    pub(super) fn call_cleanup(
        &mut self,
        program: &Program,
        action: Value,
        recovers_error: bool,
    ) -> VmResult<Option<Value>> {
        match action {
            Value::Closure(closure) => {
                let chunk = program.chunk(closure.chunk).ok_or_else(|| {
                    self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        "cleanup closure references missing chunk".into(),
                        None,
                    )
                })?;
                let expected_arity = usize::from(recovers_error);
                if chunk.arity != expected_arity || chunk.locals < chunk.arity {
                    return Err(self.error(
                        RuntimeErrorKind::InvalidBytecode,
                        "cleanup action has an invalid arity".into(),
                        None,
                    ));
                }
                self.frames.push(Frame {
                    closure,
                    function: chunk.name.clone(),
                    call_span: None,
                    ip: 0,
                    stack_base: self.stack.len(),
                    locals: if recovers_error {
                        let error = self
                            .active_error()
                            .expect("error cleanup has an active error");
                        let mut locals = vec![binding_cell(Self::error_value(error))];
                        locals.resize_with(chunk.locals, || binding_cell(Value::Nil));
                        locals
                    } else {
                        (0..chunk.locals)
                            .map(|_| binding_cell(Value::Nil))
                            .collect()
                    },
                    provided: vec![true; chunk.arity],
                    scopes: vec![Vec::new()],
                    cleanup_action: true,
                    cleanup_recovers: recovers_error,
                });
                Ok(None)
            }
            Value::Native(function) | Value::DeclaredNative { function, .. } => {
                let arguments = if recovers_error {
                    let error = self
                        .active_error()
                        .expect("error cleanup has an active error");
                    vec![Self::error_value(error)]
                } else {
                    Vec::new()
                };
                let value = self.invoke_native(&function, &arguments, None)?;
                if recovers_error {
                    self.recover_from_error(program, value)
                } else {
                    self.drive_cleanup(program)
                }
            }
            Value::Builtin(builtin) => {
                let arguments = if recovers_error {
                    let error = self
                        .active_error()
                        .expect("error cleanup has an active error");
                    vec![Self::error_value(error)]
                } else {
                    Vec::new()
                };
                let value = self.call_builtin(builtin, program, &arguments, None)?;
                if recovers_error {
                    self.recover_from_error(program, value)
                } else {
                    self.drive_cleanup(program)
                }
            }
            _ => unreachable!("defer validates callability"),
        }
    }
}
