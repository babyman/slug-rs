use super::*;

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
    assert_eq!(metrics.packed_direct_dispatches, 4);
    assert_eq!(metrics.rich_op_fallback_dispatches, 0);
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
fn records_rich_op_fallback_dispatches_separately_from_packed_dispatches() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::Nil)
        .emit(Op::DefineGlobal("answer".into()))
        .emit(Op::Nil)
        .emit(Op::Return);

    let mut vm = Vm::new();
    assert_eq!(vm.run(&program_with_main(main), 0).unwrap(), Value::Nil);

    let metrics = vm.metrics();
    assert_eq!(metrics.instructions_executed, 4);
    assert_eq!(metrics.packed_direct_dispatches, 3);
    assert_eq!(metrics.rich_op_fallback_dispatches, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn dispatches_scope_entry_without_rich_op_unpacking() {
    let mut main = Chunk::new("main", 0);
    main.emit(Op::EnterScope)
        .emit(Op::Nil)
        .emit(Op::LeaveScope)
        .emit(Op::Return);
    let mut vm = Vm::new();
    assert_eq!(vm.run(&program_with_main(main), 0).unwrap(), Value::Nil);

    let metrics = vm.metrics();
    assert_eq!(metrics.instructions_executed, 4);
    assert_eq!(metrics.packed_direct_dispatches, 3);
    assert_eq!(metrics.rich_op_fallback_dispatches, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn records_direct_local_vector_reuse_across_recur() {
    let program = compile(
        "recur-local-reuse.slug",
        "val count = fn(remaining, total) { if (remaining == 0) { total } else { recur(remaining - 1, total + 1) } }\nval main = fn() { count(2, 0) }\n",
    )
    .expect("compile recur workload");

    let mut vm = Vm::new();
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(2));

    let metrics = vm.metrics();
    assert_eq!(metrics.recur_local_vectors_reused, 2);
    assert_eq!(metrics.recur_local_vectors_replaced, 0);
    assert_eq!(metrics.argument_values_copied_to_locals, 6);
    assert_eq!(metrics.closure_argument_vectors_created, 1);
    assert_eq!(metrics.exact_positional_stack_local_initializations, 2);
    assert_eq!(metrics.provided_argument_bitmaps_created, 1);
    assert_eq!(metrics.provided_argument_bitmap_capacity_total, 0);
    assert_eq!(metrics.exact_positional_recur_restarts, 2);
    assert_eq!(metrics.generic_recur_argument_bindings, 0);
    assert_eq!(metrics.defer_scope_stacks_created, 0);
    assert_eq!(metrics.defer_scope_entries_materialized, 0);
}

#[test]
#[cfg(feature = "metrics")]
fn resolves_ordinary_call_sites_only_while_rendering_an_error() {
    let program = compile(
        "lazy-call-span.slug",
        "val fail = fn() { 1 / 0 }\nval main = fn() { fail() }\n",
    )
    .expect("compile call-site diagnostic workload");

    let mut vm = Vm::new();
    let error = vm
        .run_program(&program)
        .expect_err("division by zero must fail");
    assert_eq!(error.kind, RuntimeErrorKind::DivideByZero);
    assert_eq!(error.frames.len(), 2);
    assert!(error.frames.iter().any(|frame| {
        frame
            .span
            .as_ref()
            .is_some_and(|span| span.path.as_ref() == "lazy-call-span.slug")
    }));

    let metrics = vm.metrics();
    assert_eq!(metrics.source_span_clones, 1);
    assert_eq!(metrics.source_span_lookups, 1);
}

#[test]
#[cfg(feature = "metrics")]
fn positional_recur_uses_generic_binding_when_defaults_are_needed() {
    let program = compile(
        "recur-default-binding.slug",
        "val count = fn(remaining, total = 0) { if (remaining == 0) { total } else { recur(remaining - 1) } }\nval main = fn() { count(2) }\n",
    )
    .expect("compile recur default workload");

    let mut vm = Vm::new();
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(0));

    let metrics = vm.metrics();
    assert_eq!(metrics.exact_positional_recur_restarts, 0);
    assert_eq!(metrics.generic_recur_argument_bindings, 2);
}

#[test]
#[cfg(feature = "metrics")]
fn materializes_defer_scope_storage_only_when_a_frame_registers_a_defer() {
    let program = compile(
        "lazy-defer-scope.slug",
        "val main = fn() { defer { nil }; 1 }\n",
    )
    .expect("compile defer workload");

    let mut vm = Vm::new();
    assert_eq!(vm.run_program(&program).unwrap(), Value::Int(1));

    let metrics = vm.metrics();
    assert_eq!(metrics.defer_scope_stacks_created, 1);
    assert_eq!(metrics.defer_scope_entries_materialized, 1);
}

#[test]
#[cfg(all(feature = "concurrency", feature = "metrics"))]
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
    let program = vm.install_named(program, "main").unwrap();
    assert_eq!(vm.metrics().program_validations, 1);
    assert_eq!(vm.run_named_installed(&program).unwrap(), Value::Nil);
    let metrics = vm.metrics();
    assert_eq!(metrics.program_clones, 0);
    assert_eq!(metrics.program_clone_bytes, 0);
    assert_eq!(metrics.program_validations, 0);
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
#[cfg(all(feature = "concurrency", feature = "metrics"))]
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
    assert_eq!(metrics.timer_deadline_entries_examined, 1);
    assert_eq!(metrics.timer_wakeup_entries_examined, 1);
    assert_eq!(metrics.timer_waiter_entries_examined, 4);
    assert_eq!(metrics.peak_timer_waiters, 2);
    assert_eq!(metrics.source_span_clones, 1);
    assert_eq!(metrics.source_span_lookups, 1);
}

#[test]
#[cfg(all(feature = "concurrency", feature = "metrics"))]
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
    assert!(layout.instruction_size_bytes < std::mem::size_of::<slug_vm::Instruction>());
    assert!(layout.local_slot_size_bytes >= layout.value_size_bytes);
    assert!(layout.frame_size_bytes > layout.local_slot_size_bytes);
    assert!(layout.closure_size_bytes > 0);
    #[cfg(feature = "concurrency")]
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
