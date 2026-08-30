use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{Arc, Mutex},
};

use slug_vm::{
    CallArgumentKind, Capture, CaptureListId, Chunk, GlobalNameId, MatchMapKey, MatchPatternId,
    MatchRest, ModuleLoader, NativeArity, NativeCall, NativeError, NativeModule, NativeOwnedValue,
    NativeResourceType, NativeStatus, Op, Program, RuntimeErrorKind, SchemaField, SchemaFieldsId,
    SelectCase, SourceSpan, SpanId, StructFieldsId, Value, Vm, compile,
};

fn program_with_main(main: Chunk) -> Program {
    let mut program = Program::new();
    program.add_chunk(main);
    program
}

#[test]
#[cfg(feature = "metrics")]
fn records_execution_metrics_for_the_current_dispatch_representation() {
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let two = main.constant(Value::Int(2));
    let span = SourceSpan::new("metrics.slug", 1, 1);
    main.emit_at(Op::Constant(one), span.clone())
        .emit_at(Op::Constant(two), span.clone())
        .emit_at(Op::Add, span.clone())
        .emit_at(Op::Return, span);

    let mut vm = Vm::new();
    assert_eq!(vm.run(&program_with_main(main), 0).unwrap(), Value::Int(3));

    let metrics = vm.metrics();
    assert_eq!(metrics.instruction_clones, 0);
    assert_eq!(metrics.source_span_clones, 0);
    assert_eq!(metrics.source_span_lookups, 0);
    assert!(metrics.instructions_executed >= 4);
    assert_eq!(metrics.frames_created, 1);
    assert_eq!(metrics.local_binding_cells_created, 0);
    assert_eq!(metrics.program_clones, 1);
    assert!(metrics.program_clone_bytes > 0);
}

#[test]
#[cfg(feature = "metrics")]
fn installed_program_is_shared_by_root_tasks_and_nested_nurseries() {
    let mut child = Chunk::new("child", 0);
    let result = child.constant(Value::Int(7));
    child.emit(Op::Constant(result)).emit(Op::Return);

    let mut nursery_body = Chunk::new("nursery_body", 0);
    nursery_body
        .emit(Op::MakeClosure {
            chunk: 0,
            captures: vec![],
        })
        .emit(Op::Spawn)
        .emit(Op::Pop)
        .emit(Op::Nil)
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::MakeClosure {
        chunk: 1,
        captures: vec![],
    })
    .emit(Op::Nursery { has_limit: false })
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(child);
    program.add_chunk(nursery_body);
    program.add_chunk(main);

    let mut vm = Vm::new();
    assert_eq!(
        vm.run_named_installed(&Rc::new(program), "main").unwrap(),
        Value::Nil
    );
    let metrics = vm.metrics();
    assert_eq!(metrics.program_clones, 0);
    assert_eq!(metrics.program_clone_bytes, 0);
}

#[test]
#[cfg(feature = "metrics")]
fn promotes_only_locals_that_a_closure_captures() {
    let mut captured = Chunk::new("captured", 0);
    captured.emit(Op::GetCapture(0)).emit(Op::Return);

    let mut factory = Chunk::new("factory", 0);
    factory.locals = 2;
    let one = factory.constant(Value::Int(1));
    let two = factory.constant(Value::Int(2));
    factory
        .emit(Op::Constant(one))
        .emit(Op::SetLocal(0))
        .emit(Op::Constant(two))
        .emit(Op::SetLocal(1))
        .emit(Op::MakeClosure {
            chunk: 0,
            captures: vec![Capture::Local(0)],
        })
        .emit(Op::Return);

    let mut main = Chunk::new("main", 0);
    main.emit(Op::MakeClosure {
        chunk: 1,
        captures: vec![],
    })
    .emit(Op::Call(0))
    .emit(Op::Call(0))
    .emit(Op::Return);

    let mut program = Program::new();
    program.add_chunk(captured);
    program.add_chunk(factory);
    program.add_chunk(main);
    let mut vm = Vm::new();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Int(1));
    assert_eq!(vm.metrics().local_binding_cells_created, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn records_timer_and_select_cleanup_metrics() {
    let program = compile("scheduler-metrics.slug", "select { after 1; after 10 }\n")
        .expect("compile scheduler metrics source");
    let mut vm = Vm::new();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Nil);

    let metrics = vm.metrics();
    assert_eq!(metrics.timer_registrations, 2);
    assert!(metrics.timer_deadline_lookups >= 1);
    assert_eq!(metrics.timer_wakeups, 1);
    assert!(metrics.wait_registration_removals >= 2);
    assert!(metrics.timer_deadline_entries_examined >= 2);
    assert!(metrics.timer_wakeup_entries_examined >= 2);
    assert_eq!(metrics.peak_timer_waiters, 2);
    assert_eq!(metrics.source_span_clones, 1);
    assert_eq!(metrics.source_span_lookups, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn records_owned_spans_for_diagnostic_task_and_native_metric_paths() {
    fn fail(call: &mut NativeCall<'_>) -> NativeStatus {
        call.raise(NativeError::new("test.metrics", "deliberate failure"))
    }

    let span = SourceSpan::new("metric-paths.slug", 1, 1);

    let mut invalid_call = Chunk::new("invalid_call", 0);
    let value = invalid_call.constant(Value::Int(1));
    invalid_call
        .emit_at(Op::Constant(value), span.clone())
        .emit_at(Op::Call(0), span.clone())
        .emit_at(Op::Return, span.clone());
    let mut vm = Vm::new();
    let error = vm
        .run(&program_with_main(invalid_call), 0)
        .expect_err("calling an integer must fail");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidCall);
    assert_eq!(error.span, Some(span.clone()));
    assert_eq!(vm.metrics().source_span_clones, 1);
    assert_eq!(vm.metrics().source_span_lookups, 1);

    let mut thrown = Chunk::new("thrown", 0);
    let value = thrown.constant(Value::Int(1));
    thrown
        .emit_at(Op::Constant(value), span.clone())
        .emit_at(Op::Throw, span.clone())
        .emit_at(Op::Return, span.clone());
    let mut vm = Vm::new();
    let error = vm
        .run(&program_with_main(thrown), 0)
        .expect_err("throw must fail the root execution");
    assert_eq!(error.kind, RuntimeErrorKind::Thrown);
    assert_eq!(error.span, Some(span.clone()));
    assert_eq!(vm.metrics().source_span_clones, 1);
    assert_eq!(vm.metrics().source_span_lookups, 1);

    let mut child = Chunk::new("child", 0);
    child.emit(Op::Nil).emit(Op::Return);
    let mut main = Chunk::new("main", 0);
    main.emit_at(
        Op::MakeClosure {
            chunk: 0,
            captures: vec![],
        },
        span.clone(),
    )
    .emit_at(Op::Spawn, span.clone())
    .emit_at(Op::Pop, span.clone())
    .emit_at(Op::Nil, span.clone())
    .emit_at(Op::Return, span.clone());
    let mut program = Program::new();
    program.add_chunk(child);
    program.add_chunk(main);
    let mut vm = Vm::new();
    assert_eq!(vm.run_named(&program, "main").unwrap(), Value::Nil);
    assert_eq!(vm.metrics().source_span_clones, 1);
    assert_eq!(vm.metrics().source_span_lookups, 1);

    let module = NativeModule::new("test.metrics", ()).expect("native module is valid");
    let mut vm = Vm::new();
    vm.define_native(
        module
            .function("fail", NativeArity::Exact(0), fail)
            .expect("native function is valid"),
    )
    .expect("native function is unique");
    let mut native = Chunk::new("native", 0);
    native
        .emit_at(Op::GetGlobal("fail".into()), span.clone())
        .emit_at(Op::Call(0), span.clone())
        .emit_at(Op::Return, span);
    let error = vm
        .run(&program_with_main(native), 0)
        .expect_err("native failure must remain checked");
    assert_eq!(error.kind, RuntimeErrorKind::Native);
    assert_eq!(vm.metrics().source_span_clones, 1);
    assert_eq!(vm.metrics().source_span_lookups, 1);
}

#[test]
fn interns_instruction_spans_and_preserves_diagnostics() {
    let span = SourceSpan::new("interned.slug", 3, 5);
    let mut main = Chunk::new("main", 0);
    let one = main.constant(Value::Int(1));
    let zero = main.constant(Value::Int(0));
    main.emit_at(Op::Constant(one), span.clone())
        .emit_at(Op::Constant(zero), span.clone())
        .emit_at(Op::Divide, span.clone())
        .emit_at(Op::Return, span.clone());

    let program = program_with_main(main);
    assert_eq!(program.source_count(), 1);
    assert_eq!(program.span_count(), 1);
    let layout = program.layout_metrics();
    assert!(layout.program_inline_bytes > 0);
    assert_eq!(layout.instructions, 4);
    assert_eq!(layout.constant_pool_slots, 2);
    assert!(layout.chunk_storage_bytes >= layout.instruction_bytes);
    assert!(layout.constant_pool_capacity_bytes >= 2 * std::mem::size_of::<slug_vm::Constant>());
    assert!(layout.compressed_span_map_bytes < layout.inline_span_bytes);
    let error = Vm::new()
        .run(&program, 0)
        .expect_err("division by zero must retain its source span");
    assert_eq!(error.span, Some(span));
}

#[test]
fn reports_vm_runtime_layouts() {
    let layout = Vm::layout_metrics();
    assert_eq!(layout.value_size_bytes, std::mem::size_of::<Value>());
    assert_eq!(layout.value_alignment_bytes, std::mem::align_of::<Value>());
    assert_eq!(
        layout.instruction_size_bytes,
        std::mem::size_of::<slug_vm::Instruction>()
    );
    assert!(layout.local_slot_size_bytes >= layout.value_size_bytes);
    assert!(layout.frame_size_bytes > layout.local_slot_size_bytes);
    assert!(layout.closure_size_bytes > 0);
    assert!(layout.task_state_size_bytes > layout.task_size_bytes);
}

#[test]
fn rejects_missing_instruction_span_metadata_before_execution() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil).emit(Op::Return);
    main.code[0].span = Some(SpanId::new(7));

    let error = Vm::new()
        .run(&program_with_main(main), 0)
        .expect_err("missing source span must be rejected before execution");
    assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    assert!(error.message.contains("references missing source span 7"));
}

#[test]
fn rejects_missing_opcode_pool_metadata_before_execution() {
    let cases = [
        (
            Op::GetGlobalPooled(GlobalNameId::new(0)),
            "missing global name metadata",
        ),
        (
            Op::MakeClosurePooled {
                chunk: 0,
                captures: CaptureListId::new(0),
            },
            "missing capture metadata",
        ),
        (
            Op::StructSchemaPooled(SchemaFieldsId::new(0)),
            "missing schema field metadata",
        ),
        (
            Op::StructPooled(StructFieldsId::new(0)),
            "missing struct field metadata",
        ),
        (
            Op::TryMatchPooled {
                pattern: MatchPatternId::new(0),
                bindings: 0,
                operands: 0,
            },
            "missing match pattern metadata",
        ),
    ];
    for (op, expected) in cases {
        let mut main = Chunk::new("main", 0);
        main.emit(op).emit(Op::Return);
        let error = Vm::new()
            .run(&program_with_main(main), 0)
            .expect_err("missing pool metadata must be rejected before execution");
        assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
        assert!(error.message.contains(expected), "{}", error.message);
    }
}

fn native_make_channel(call: &mut NativeCall<'_>) -> NativeStatus {
    let capacity = match call.argument(0).and_then(slug_vm::NativeValueRef::as_i64) {
        Ok(value) => match usize::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                return call.raise(NativeError::new(
                    "native.type",
                    "channel capacity must not be negative or too large",
                ));
            }
        },
        Err(error) => return call.raise(error),
    };
    let channel = call.plain_channel(capacity);
    call.return_value(channel)
}

fn vm_with_channel_constructor() -> Vm {
    let mut vm = Vm::new();
    let module = NativeModule::new("test.channels", ()).expect("native module is valid");
    vm.define_native(
        module
            .function("make_channel", NativeArity::Exact(1), native_make_channel)
            .expect("native channel constructor is valid"),
    )
    .expect("native channel constructor is unique");
    vm
}

mod native_resource_fixture {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    struct Payload {
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
        panic_on_first_close: bool,
    }

    struct State {
        first: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        second: Rc<RefCell<Option<NativeResourceType<Payload>>>>,
        closed: Rc<Cell<usize>>,
        destroyed: Rc<Cell<usize>>,
    }

    pub fn module(closed: Rc<Cell<usize>>, destroyed: Rc<Cell<usize>>) -> NativeModule {
        let first = Rc::new(RefCell::new(None));
        let second = Rc::new(RefCell::new(None));
        let module = NativeModule::new(
            "test.shared_resources",
            State {
                first: first.clone(),
                second: second.clone(),
                closed,
                destroyed,
            },
        )
        .unwrap();
        *first.borrow_mut() = Some(module.resource_type("first", close, destroy).unwrap());
        *second.borrow_mut() = Some(module.resource_type("second", close, destroy).unwrap());
        module
    }

    pub fn install(vm: &mut Vm, module: &NativeModule) {
        for (name, arity, callback) in [
            (
                "make_resource",
                NativeArity::Exact(0),
                make_resource as for<'call> fn(&mut NativeCall<'call>) -> NativeStatus,
            ),
            ("wrong_resource", NativeArity::Exact(1), wrong_resource),
            ("close_resource", NativeArity::Exact(1), close_resource),
            ("busy_then_close", NativeArity::Exact(1), busy_then_close),
            ("retry_close", NativeArity::Exact(1), retry_close),
            (
                "fail_with_resource",
                NativeArity::Exact(0),
                fail_with_resource,
            ),
            (
                "make_panicking_resource",
                NativeArity::Exact(0),
                make_panicking_resource,
            ),
        ] {
            vm.define_native(module.function(name, arity, callback).unwrap())
                .unwrap();
        }
    }

    fn close(payload: &mut Payload) {
        if payload.panic_on_first_close {
            payload.panic_on_first_close = false;
            panic!("first close fails");
        }
        payload.closed.set(payload.closed.get() + 1);
    }

    fn destroy(payload: Payload) {
        let Payload { destroyed, .. } = payload;
        destroyed.set(destroyed.get() + 1);
    }

    fn make_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: false,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.return_value(value),
            Err(error) => call.raise(error),
        }
    }

    fn make_panicking_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: true,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.return_value(value),
            Err(error) => call.raise(error),
        }
    }

    fn wrong_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .second
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        match call.with_resource(0, &resource_type, |_| ()) {
            Ok(()) => call.return_value(NativeOwnedValue::nil()),
            Err(error) => call.raise(error),
        }
    }

    fn close_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn busy_then_close(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        let nested = match call.with_resource(0, &resource_type, |_| {
            call.close_resource(0, &resource_type)
        }) {
            Ok(result) => result,
            Err(error) => return call.raise(error),
        };
        if nested.is_ok() {
            return call.raise(NativeError::new(
                "test.expected_busy",
                "overlapping close unexpectedly succeeded",
            ));
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn retry_close(call: &mut NativeCall<'_>) -> NativeStatus {
        let resource_type = call
            .state::<State>()
            .unwrap()
            .first
            .borrow()
            .as_ref()
            .unwrap()
            .clone();
        if call.close_resource(0, &resource_type).is_ok() {
            return call.raise(NativeError::new(
                "test.expected_close_failure",
                "first close unexpectedly succeeded",
            ));
        }
        if let Err(error) = call.close_resource(0, &resource_type) {
            return call.raise(error);
        }
        call.return_value(NativeOwnedValue::nil())
    }

    fn fail_with_resource(call: &mut NativeCall<'_>) -> NativeStatus {
        let state = call.state::<State>().unwrap();
        let resource_type = state.first.borrow().as_ref().unwrap().clone();
        let payload = Payload {
            closed: state.closed.clone(),
            destroyed: state.destroyed.clone(),
            panic_on_first_close: false,
        };
        match call.resource(&resource_type, payload) {
            Ok(value) => call.raise(
                NativeError::new("test.resource_error", "resource in error data").with_data(value),
            ),
            Err(error) => call.raise(error),
        }
    }
}

// Feature-oriented VM test modules. Shared bytecode and native-resource
// fixtures remain here because several independent execution boundaries use
// them.
#[path = "vm/bytecode.rs"]
mod bytecode;
#[path = "vm/calls_and_native.rs"]
mod calls_and_native;
#[path = "vm/collections.rs"]
mod collections;
#[path = "vm/concurrency.rs"]
mod concurrency;
