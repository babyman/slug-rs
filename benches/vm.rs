use std::{
    fmt::Write,
    hint::black_box,
    time::{Duration, Instant},
};

use slug_vm::{Program, Vm, VmMetrics, compile};

const ITERATIONS: usize = 1_000;

struct Workload {
    name: &'static str,
    iterations: usize,
    source: fn() -> String,
}

fn main() {
    for workload in WORKLOADS {
        let source = (workload.source)();
        let program = compile(workload.name, &source)
            .unwrap_or_else(|error| panic!("compile {}: {error}", workload.name));
        let (elapsed, metrics) = run(&program, workload.iterations);
        let layout = program.layout_metrics();
        println!(
            "{name}: {iterations} runs in {elapsed:?} ({verification:?} verification, {scheduler_wait:?} scheduler wait); {instructions} instructions; {clones} instruction clones; {spans} source-span clones; {program_clones} whole-program clones ({program_clone_bytes} estimated instruction bytes); {frames} frames; {cells} local cells; {timers} timer registrations; {lookups} deadline lookups/{deadline_entries} entries; {wakeups} timer wakeups/{wakeup_entries} entries; {removals} wait-registration removals; removal entries channel/task/timer {channel_entries}/{task_entries}/{timer_entries}; peak timers/ready/channel/task {peak_timers}/{peak_ready}/{peak_channel}/{peak_task}; layout inline/chunk/constants/descriptors/metadata/sources {program_inline}/{chunk_storage}/{constant_bytes}/{descriptor_bytes}/{metadata_bytes}/{source_bytes}; {instruction_bytes} instruction bytes ({instruction_size_bytes} each); max chunk/constants/locals/metadata {largest_chunk_instructions}/{largest_constant_pool}/{largest_local_frame}/{largest_metadata_pool}; {span_entries} span entries; {inline_span_bytes} inline span bytes; {compressed_span_map_bytes} compressed span-map bytes",
            name = workload.name,
            iterations = workload.iterations,
            scheduler_wait = metrics.scheduler_wait_time,
            verification = metrics.verification_time,
            instructions = metrics.instructions_executed,
            clones = metrics.instruction_clones,
            spans = metrics.source_span_clones,
            program_clones = metrics.program_clones,
            program_clone_bytes = metrics.program_clone_bytes,
            frames = metrics.frames_created,
            cells = metrics.local_binding_cells_created,
            timers = metrics.timer_registrations,
            lookups = metrics.timer_deadline_lookups,
            wakeups = metrics.timer_wakeups,
            removals = metrics.wait_registration_removals,
            deadline_entries = metrics.timer_deadline_entries_examined,
            wakeup_entries = metrics.timer_wakeup_entries_examined,
            channel_entries = metrics.channel_waiter_entries_examined,
            task_entries = metrics.task_waiter_entries_examined,
            timer_entries = metrics.timer_waiter_entries_examined,
            peak_timers = metrics.peak_timer_waiters,
            peak_ready = metrics.peak_ready_queue,
            peak_channel = metrics.peak_channel_waiters,
            peak_task = metrics.peak_task_waiters,
            program_inline = layout.program_inline_bytes,
            chunk_storage = layout.chunk_storage_bytes,
            constant_bytes = layout.constant_pool_capacity_bytes,
            descriptor_bytes = layout.descriptor_capacity_bytes,
            metadata_bytes = layout.metadata_pool_capacity_bytes,
            source_bytes = layout.source_table_capacity_bytes,
            instruction_bytes = layout.instruction_bytes,
            instruction_size_bytes = layout.instruction_size_bytes,
            largest_chunk_instructions = layout.largest_chunk_instructions,
            largest_constant_pool = layout.largest_constant_pool,
            largest_local_frame = layout.largest_local_frame,
            largest_metadata_pool = layout.largest_metadata_pool,
            span_entries = layout.span_table_entries,
            inline_span_bytes = layout.inline_span_bytes,
            compressed_span_map_bytes = layout.compressed_span_map_bytes,
        );
    }
}

fn run(program: &Program, iterations: usize) -> (Duration, VmMetrics) {
    let started = Instant::now();
    let mut metrics = VmMetrics::default();
    for _ in 0..iterations {
        let mut vm = Vm::new();
        black_box(
            vm.run_named(program, "main")
                .expect("run benchmark program"),
        );
        let run_metrics = vm.metrics();
        metrics.instructions_executed += run_metrics.instructions_executed;
        metrics.instruction_clones += run_metrics.instruction_clones;
        metrics.source_span_clones += run_metrics.source_span_clones;
        metrics.program_clones += run_metrics.program_clones;
        metrics.program_clone_bytes += run_metrics.program_clone_bytes;
        metrics.frames_created += run_metrics.frames_created;
        metrics.local_binding_cells_created += run_metrics.local_binding_cells_created;
        metrics.timer_registrations += run_metrics.timer_registrations;
        metrics.timer_deadline_lookups += run_metrics.timer_deadline_lookups;
        metrics.timer_wakeups += run_metrics.timer_wakeups;
        metrics.wait_registration_removals += run_metrics.wait_registration_removals;
        metrics.timer_deadline_entries_examined += run_metrics.timer_deadline_entries_examined;
        metrics.timer_wakeup_entries_examined += run_metrics.timer_wakeup_entries_examined;
        metrics.channel_waiter_entries_examined += run_metrics.channel_waiter_entries_examined;
        metrics.task_waiter_entries_examined += run_metrics.task_waiter_entries_examined;
        metrics.timer_waiter_entries_examined += run_metrics.timer_waiter_entries_examined;
        metrics.peak_timer_waiters = metrics
            .peak_timer_waiters
            .max(run_metrics.peak_timer_waiters);
        metrics.peak_ready_queue = metrics.peak_ready_queue.max(run_metrics.peak_ready_queue);
        metrics.peak_channel_waiters = metrics
            .peak_channel_waiters
            .max(run_metrics.peak_channel_waiters);
        metrics.peak_task_waiters = metrics.peak_task_waiters.max(run_metrics.peak_task_waiters);
        metrics.scheduler_wait_time += run_metrics.scheduler_wait_time;
        metrics.verification_time += run_metrics.verification_time;
    }
    (started.elapsed(), metrics)
}

const WORKLOADS: &[Workload] = &[
    Workload {
        name: "arithmetic-and-branches",
        iterations: ITERATIONS,
        source: || {
            "val sum = fn(n, total) { if (n == 0) { total } else { recur(n - 1, total + n) } }\nsum(200, 0)\n".into()
        },
    },
    Workload {
        name: "calls-and-closures",
        iterations: ITERATIONS,
        source: || {
            "val makeAdder = fn(base) { fn(value) { base + value } }\nval add = makeAdder(1)\nadd(41)\n".into()
        },
    },
    Workload {
        name: "pattern-matching",
        iterations: ITERATIONS,
        source: || {
            "val describe = fn(value) match { [head, second, ...] => head + second; _ => 0 }\ndescribe([1, 2, 3])\n".into()
        },
    },
    Workload {
        name: "deferred-cleanup",
        iterations: ITERATIONS,
        source: || "val work = fn(value) { defer { nil }; value + 1 }\nwork(41)\n".into(),
    },
    Workload {
        name: "lists-and-maps",
        iterations: ITERATIONS,
        source: || {
            "val values = [1, 2, 3]\nval mapped = {first: values[0], last: values[2]}\nmapped[\"first\"] + mapped[\"last\"]\n".into()
        },
    },
    Workload {
        name: "many-timers-8",
        iterations: 100,
        source: many_timers_8,
    },
    Workload {
        name: "many-timers-32",
        iterations: 100,
        source: many_timers_32,
    },
    Workload {
        name: "many-timers-128",
        iterations: 25,
        source: many_timers_128,
    },
    Workload {
        name: "many-select-cases",
        iterations: 100,
        source: many_select_cases,
    },
    Workload {
        name: "cancel-suspended-waits",
        iterations: 10,
        source: cancel_suspended_waits,
    },
];

fn many_timers_8() -> String {
    many_timers(8)
}

fn many_timers_32() -> String {
    many_timers(32)
}

fn many_timers_128() -> String {
    many_timers(128)
}

fn many_timers(count: usize) -> String {
    let mut source = String::from("val worker = fn() { select { after 1 } }\n");
    for index in 0..count {
        writeln!(source, "val task{index} = spawn {{ worker() }}")
            .expect("writing to a string cannot fail");
    }
    for index in 0..count {
        writeln!(source, "select {{ await task{index} }}")
            .expect("writing to a string cannot fail");
    }
    source.push_str("nil\n");
    source
}

fn many_select_cases() -> String {
    let mut source = String::from("select {\n");
    for milliseconds in 1..=16 {
        writeln!(source, "after {milliseconds}").expect("writing to a string cannot fail");
    }
    source.push_str("}\n");
    source
}

fn cancel_suspended_waits() -> String {
    let mut source =
        String::from("val attempt = fn() {\ndefer onerror(error) { nil }\nnursery {\n");
    source.push_str("spawn { throw \"fail\" }\n");
    for _ in 0..16 {
        source.push_str("spawn { select { after 50; after 60 } }\n");
    }
    source.push_str("}\n}\nattempt()\n");
    source
}
