use crate::{DeferMode, Program, SourceSpan, Value};

use super::{Frame, LocalSlot, RuntimeError, RuntimeErrorKind, Vm, VmResult, frame_locals};

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
    Recover {
        value: Value,
        frame_depth: usize,
    },
    Resume,
    Recur {
        arguments: Vec<Value>,
        provided: Vec<bool>,
    },
    Error(RuntimeError),
}

impl Vm {
    pub(super) fn begin_return(&mut self, value: Value) -> VmResult<Option<Value>> {
        let frame_depth = self.frames.len().checked_sub(1).ok_or_else(|| {
            self.error(
                RuntimeErrorKind::InvalidBytecode,
                "return cleanup has no frame".into(),
                None,
            )
        })?;
        let returns_to_cleanup_action = !self
            .frames
            .last()
            .expect("frame was checked")
            .cleanup_action
            && self
                .frames
                .get(self.frames.len().saturating_sub(2))
                .is_some_and(|parent| parent.cleanup_action);
        let frame = self.frames.last_mut().expect("frame was checked");
        let scopes = std::mem::take(&mut frame.scopes);
        let cleanup_recovers = frame.cleanup_action && frame.cleanup_recovers;
        let cleanup_owner_depth = frame.cleanup_owner_depth;
        if cleanup_recovers && cleanup_owner_depth.is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "error cleanup has no owning frame".into(),
                None,
            ));
        }
        if returns_to_cleanup_action {
            self.cleanup.push(Cleanup::Resume);
        }
        self.cleanup.push(if cleanup_recovers {
            Cleanup::Recover {
                value,
                frame_depth: cleanup_owner_depth.unwrap_or_default(),
            }
        } else {
            Cleanup::Return(value)
        });
        self.cleanup
            .extend(scopes.into_iter().map(|actions| Cleanup::Actions {
                actions,
                success: true,
                frame_depth,
            }));
        self.drive_cleanup()
    }

    pub(super) fn recur_at(
        &mut self,
        program: &Program,
        kinds: &[crate::CallArgumentKind],
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let values = self.pop_values_at(kinds.len(), span)?;
        let (positional, named) = self.expand_call_arguments_at(values, kinds, span)?;
        let closure = self
            .frames
            .last()
            .map(|frame| crate::Value::Closure(frame.closure.clone()))
            .ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    span,
                )
            })?;
        let (arguments, provided) =
            self.bind_call_arguments_at(program, &closure, positional, named, span)?;
        self.restart_recur(arguments, Some(provided), program, span)
    }

    pub(super) fn recur_positional_at(
        &mut self,
        program: &Program,
        count: usize,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let values = self.pop_values_at(count, span)?;
        let closure = self
            .frames
            .last()
            .map(|frame| crate::Value::Closure(frame.closure.clone()))
            .ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    span,
                )
            })?;
        let exact = {
            let chunk = self.current_chunk(program)?;
            chunk.arity == count
                && !chunk
                    .parameters
                    .iter()
                    .any(|parameter| parameter.has_default || parameter.variadic)
        };
        if exact {
            #[cfg(feature = "metrics")]
            self.record_exact_positional_recur_restart();
            return self.restart_recur(values, None, program, span);
        }
        #[cfg(feature = "metrics")]
        self.record_generic_recur_argument_binding();
        let (arguments, provided) =
            self.bind_call_arguments_at(program, &closure, values, Vec::new(), span)?;
        self.restart_recur(arguments, Some(provided), program, span)
    }

    fn restart_recur(
        &mut self,
        arguments: Vec<Value>,
        provided: Option<Vec<bool>>,
        program: &Program,
        span: Option<&SourceSpan>,
    ) -> VmResult<()> {
        let arity = self.current_chunk(program)?.arity;
        let (_, local_count, stack_base) = self
            .frames
            .last()
            .map(|frame| (frame.closure.chunk, frame.locals.len(), frame.stack_base))
            .ok_or_else(|| {
                self.error_at(
                    RuntimeErrorKind::InvalidBytecode,
                    "no active call frame".into(),
                    span,
                )
            })?;
        if local_count < arity {
            return Err(self.error_at(
                RuntimeErrorKind::InvalidBytecode,
                format!("active function has {local_count} local slots for {arity} parameters"),
                span,
            ));
        }
        let nested_scopes = {
            let frame = self.frames.last_mut().expect("active frame was checked");
            frame.scope_depth = 1;
            if frame.scopes.len() > 1 {
                frame.scopes.split_off(1)
            } else {
                Vec::new()
            }
        };
        if !nested_scopes.is_empty() {
            self.cleanup.push(Cleanup::Recur {
                arguments,
                provided: provided.unwrap_or_else(|| vec![true; arity]),
            });
            self.cleanup
                .extend(nested_scopes.into_iter().map(|actions| Cleanup::Actions {
                    actions,
                    success: true,
                    frame_depth: self.frames.len() - 1,
                }));
            self.drive_cleanup()?;
            return Ok(());
        }
        if let Some(provided) = provided {
            self.finish_recur(arguments, provided, local_count, stack_base);
        } else {
            self.finish_recur_positional(arguments, local_count, stack_base);
        }
        Ok(())
    }

    pub(super) fn finish_recur(
        &mut self,
        arguments: Vec<Value>,
        provided: Vec<bool>,
        local_count: usize,
        stack_base: usize,
    ) {
        let provided = self.provided_bitmap(provided);
        self.finish_recur_with_provided(arguments, provided, local_count, stack_base);
    }

    pub(super) fn finish_recur_positional(
        &mut self,
        arguments: Vec<Value>,
        local_count: usize,
        stack_base: usize,
    ) {
        self.finish_recur_with_provided(
            arguments,
            super::ProvidedArguments::All,
            local_count,
            stack_base,
        );
    }

    fn finish_recur_with_provided(
        &mut self,
        arguments: Vec<Value>,
        provided: super::ProvidedArguments,
        local_count: usize,
        stack_base: usize,
    ) {
        self.stack.truncate(stack_base);
        let argument_count = arguments.len();
        let reusable = self.frames.last().is_some_and(|frame| {
            frame.locals.len() == local_count
                && frame
                    .locals
                    .iter()
                    .all(|local| matches!(local, LocalSlot::Direct(_)))
        });
        if reusable {
            // Captured locals are cells whose identity belongs to the prior
            // iteration, so only direct slots may be overwritten in place.
            let frame = self.frames.last_mut().expect("active frame was checked");
            let mut arguments = arguments.into_iter();
            for local in &mut frame.locals {
                *local = LocalSlot::Direct(arguments.next().unwrap_or(Value::Nil));
            }
            frame.provided = provided;
            frame.ip = 0;
            self.record_local_argument_writes(argument_count);
            self.record_recur_local_vector(true);
            return;
        }
        let locals = frame_locals(arguments, local_count);
        self.record_frame_locals(locals.capacity(), argument_count);
        self.record_recur_local_vector(false);
        let frame = self.frames.last_mut().expect("active frame was checked");
        frame.locals = locals;
        frame.provided = provided;
        frame.ip = 0;
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
        value: Value,
        frame_depth: usize,
    ) -> VmResult<Option<Value>> {
        if self.frames.get(frame_depth).is_none() {
            return Err(self.error(
                RuntimeErrorKind::InvalidBytecode,
                "error cleanup has no owning frame".into(),
                None,
            ));
        }
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
        self.frames.truncate(frame_depth + 1);
        let stack_base = self.frames[frame_depth].stack_base;
        self.stack.truncate(stack_base);
        self.cleanup = recovered;
        self.cleanup.insert(0, Cleanup::Return(value));
        self.drive_cleanup()
    }

    pub(super) fn drive_cleanup(&mut self) -> VmResult<Option<Value>> {
        loop {
            match self.cleanup.last_mut() {
                Some(Cleanup::Actions {
                    actions,
                    success,
                    frame_depth,
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
                        let frame_depth = *frame_depth;
                        return self.call_cleanup(action, mode == DeferMode::Error, frame_depth);
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
                Some(Cleanup::Recover { .. }) => {
                    let Cleanup::Recover { value, frame_depth } =
                        self.cleanup.pop().expect("cleanup exists")
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
                    return self.recover_from_error(value, frame_depth);
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

    #[allow(clippy::too_many_lines)]
    pub(super) fn call_cleanup(
        &mut self,
        action: Value,
        recovers_error: bool,
        frame_depth: usize,
    ) -> VmResult<Option<Value>> {
        match action {
            Value::Closure(closure) => {
                let frame_program = closure.program.clone().unwrap_or(self.active_program()?);
                let chunk = frame_program.chunk(closure.chunk).ok_or_else(|| {
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
                #[cfg(feature = "metrics")]
                self.record_frame(chunk.locals);
                let arguments = if recovers_error {
                    let error = self
                        .active_error()
                        .expect("error cleanup has an active error");
                    vec![Self::error_value(error)]
                } else {
                    Vec::new()
                };
                let argument_count = arguments.len();
                let locals = frame_locals(arguments, chunk.locals);
                self.record_frame_locals(locals.capacity(), argument_count);
                self.frames.push(Frame {
                    program: closure.program.clone().unwrap_or(self.active_program()?),
                    globals: closure
                        .globals
                        .clone()
                        .unwrap_or_else(|| self.globals.clone()),
                    closure,
                    call_span: None,
                    ip: 0,
                    stack_base: self.stack.len(),
                    locals,
                    provided: super::ProvidedArguments::All,
                    scope_depth: 1,
                    scopes: Vec::new(),
                    cleanup_action: true,
                    cleanup_recovers: recovers_error,
                    cleanup_owner_depth: Some(frame_depth),
                });
                Ok(None)
            }
            Value::Native(function) => {
                let arguments = if recovers_error {
                    let error = self
                        .active_error()
                        .expect("error cleanup has an active error");
                    vec![Self::error_value(error)]
                } else {
                    Vec::new()
                };
                let value = self.invoke_native(&function, &arguments, None, None)?;
                if recovers_error {
                    self.recover_from_error(value, frame_depth)
                } else {
                    self.drive_cleanup()
                }
            }
            Value::DeclaredNative {
                function,
                resource_signature,
                ..
            } => {
                let arguments = if recovers_error {
                    let error = self
                        .active_error()
                        .expect("error cleanup has an active error");
                    vec![Self::error_value(error)]
                } else {
                    Vec::new()
                };
                let value =
                    self.invoke_native(&function, &arguments, Some(&resource_signature), None)?;
                if recovers_error {
                    self.recover_from_error(value, frame_depth)
                } else {
                    self.drive_cleanup()
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
                let program = self.active_program()?;
                let value = self.call_builtin(builtin, &program, &arguments, None)?;
                if recovers_error {
                    self.recover_from_error(value, frame_depth)
                } else {
                    self.drive_cleanup()
                }
            }
            _ => unreachable!("defer validates callability"),
        }
    }
}
