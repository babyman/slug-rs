use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use slug_vm::{Program, Vm, VmMetrics, compile};

const ITERATIONS: usize = 1_000;

struct Workload {
    name: &'static str,
    source: &'static str,
}

fn main() {
    for workload in WORKLOADS {
        let program = compile(workload.name, workload.source)
            .unwrap_or_else(|error| panic!("compile {}: {error}", workload.name));
        let (elapsed, metrics) = run(&program);
        println!(
            "{name}: {iterations} runs in {elapsed:?}; {instructions} instructions; {clones} instruction clones; {frames} frames; {cells} local cells",
            name = workload.name,
            iterations = ITERATIONS,
            instructions = metrics.instructions_executed,
            clones = metrics.instruction_clones,
            frames = metrics.frames_created,
            cells = metrics.local_binding_cells_created,
        );
    }
}

fn run(program: &Program) -> (Duration, VmMetrics) {
    let started = Instant::now();
    let mut metrics = VmMetrics::default();
    for _ in 0..ITERATIONS {
        let mut vm = Vm::new();
        black_box(
            vm.run_named(program, "main")
                .expect("run benchmark program"),
        );
        let run_metrics = vm.metrics();
        metrics.instructions_executed += run_metrics.instructions_executed;
        metrics.instruction_clones += run_metrics.instruction_clones;
        metrics.frames_created += run_metrics.frames_created;
        metrics.local_binding_cells_created += run_metrics.local_binding_cells_created;
    }
    (started.elapsed(), metrics)
}

const WORKLOADS: &[Workload] = &[
    Workload {
        name: "arithmetic-and-branches",
        source: "val sum = fn(n, total) { if (n == 0) { total } else { recur(n - 1, total + n) } }\nsum(200, 0)\n",
    },
    Workload {
        name: "calls-and-closures",
        source: "val makeAdder = fn(base) { fn(value) { base + value } }\nval add = makeAdder(1)\nadd(41)\n",
    },
    Workload {
        name: "pattern-matching",
        source: "val describe = fn(value) match { [head, second, ...] => head + second; _ => 0 }\ndescribe([1, 2, 3])\n",
    },
    Workload {
        name: "deferred-cleanup",
        source: "val work = fn(value) { defer { nil }; value + 1 }\nwork(41)\n",
    },
    Workload {
        name: "lists-and-maps",
        source: "val values = [1, 2, 3]\nval mapped = {first: values[0], last: values[2]}\nmapped[\"first\"] + mapped[\"last\"]\n",
    },
];
