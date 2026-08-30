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
            "{name}: {iterations} runs in {elapsed:?}; {instructions} instructions; {clones} instruction clones; {spans} source-span clones; {frames} frames; {cells} local cells; {timers} timer registrations; {lookups} deadline lookups; {wakeups} timer wakeups; {removals} wait-registration removals; {instruction_bytes} instruction bytes; {span_entries} span entries; {inline_span_bytes} inline span bytes; {compressed_span_map_bytes} compressed span-map bytes",
            name = workload.name,
            iterations = workload.iterations,
            instructions = metrics.instructions_executed,
            clones = metrics.instruction_clones,
            spans = metrics.source_span_clones,
            frames = metrics.frames_created,
            cells = metrics.local_binding_cells_created,
            timers = metrics.timer_registrations,
            lookups = metrics.timer_deadline_lookups,
            wakeups = metrics.timer_wakeups,
            removals = metrics.wait_registration_removals,
            instruction_bytes = layout.instruction_bytes,
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
        metrics.frames_created += run_metrics.frames_created;
        metrics.local_binding_cells_created += run_metrics.local_binding_cells_created;
        metrics.timer_registrations += run_metrics.timer_registrations;
        metrics.timer_deadline_lookups += run_metrics.timer_deadline_lookups;
        metrics.timer_wakeups += run_metrics.timer_wakeups;
        metrics.wait_registration_removals += run_metrics.wait_registration_removals;
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
        name: "many-timers",
        iterations: 100,
        source: many_timers,
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

fn many_timers() -> String {
    let mut source = String::from("val worker = fn() { select { after 1 } }\n");
    for index in 0..32 {
        writeln!(source, "val task{index} = spawn {{ worker() }}")
            .expect("writing to a string cannot fail");
    }
    for index in 0..32 {
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
    for _ in 0..16 {
        source.push_str("spawn { select { after 50; after 60 } }\n");
    }
    source.push_str("spawn { throw \"fail\" }\n}\n}\nattempt()\n");
    source
}
