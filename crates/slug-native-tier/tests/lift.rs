//! Lifting compiled Slug into the native tier's IR.

use std::path::{Path, PathBuf};

use slug_native_tier::{
    ir::{Function, Local, Terminator, ValueDef},
    lift::lift,
};
use slug_vm::{Op, Program};

fn compile(source: &str) -> Program {
    slug_frontend::compile("test.slug", source).expect("test source compiles")
}

/// Lifts and checks every chunk, keyed by the order they appear.
fn lift_all(program: &Program) -> Vec<Function> {
    (0..program.chunk_count())
        .map(|chunk| {
            let function = lift(program, chunk)
                .unwrap_or_else(|message| panic!("chunk {chunk} does not lift: {message}"));
            function
                .check()
                .unwrap_or_else(|message| panic!("chunk {chunk}: {message}\n{function}"));
            function
        })
        .collect()
}

/// The lifted function with `arity` parameters whose IR mentions `needle`.
fn find<'a>(functions: &'a [Function], arity: usize, needle: &str) -> &'a Function {
    functions
        .iter()
        .find(|function| function.arity == arity && function.to_string().contains(needle))
        .unwrap_or_else(|| panic!("no function with {arity} parameters mentions {needle}"))
}

#[test]
fn a_conditional_joins_its_two_results_in_a_parameter() {
    let program = compile(
        "val fib = fn(n) { if (n < 2) { n } else { fib(n - 1) + fib(n - 2) } }\n\
         val main = fn() { println(fib(10)) }\n",
    );
    let functions = lift_all(&program);
    let fib = find(&functions, 1, "Add(");
    let joins = fib
        .blocks
        .iter()
        .filter(|block| !block.params.is_empty())
        .collect::<Vec<_>>();
    // The entry takes `n`; the join after the `if` takes its result.
    assert_eq!(joins.len(), 2, "{fib}");
    assert!(
        matches!(joins[1].terminator, Terminator::Return(value) if value == joins[1].params[0])
    );
}

#[test]
fn recur_passes_the_next_arguments_to_the_entry() {
    let program = compile(
        "val count = fn(remaining, total) {\n\
             if (remaining == 0) { total } else { recur(remaining - 1, total + 2) }\n\
         }\n\
         val main = fn() { println(count(5, 0)) }\n",
    );
    let functions = lift_all(&program);
    let count = find(&functions, 2, "RecurPositional");
    let recurs = count
        .blocks
        .iter()
        .filter_map(|block| match &block.terminator {
            Terminator::Recur(arguments) => Some(arguments.len()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(recurs, [2], "{count}");
}

#[test]
fn a_join_inside_an_expression_carries_the_stack_values_below_it() {
    // The call's callee and first argument are on the stack while the
    // `if` chooses the second argument, so both paths bring them.
    let program = compile(
        "val pair = fn(left, right) { [left, right] }\n\
         val choose = fn(flag, other) { pair(other, if (flag) { 1 } else { 2 }) }\n\
         val main = fn() { println(choose(true, 3)) }\n",
    );
    let functions = lift_all(&program);
    let choose = find(&functions, 2, "CallPositional");
    let join = choose
        .blocks
        .iter()
        .find(|block| block.params.len() == 1 && !block.insts.is_empty())
        .unwrap_or_else(|| panic!("no join takes the chosen value\n{choose}"));
    // The callee and `other` below the chosen value are the same on both
    // paths, so they are not parameters; the call reads them directly.
    let call = &join.insts[0];
    assert_eq!(call.args.len(), 3, "{choose}");
    assert!(
        matches!(choose.def(call.args[0]), ValueDef::Result { .. }),
        "{choose}"
    );
    assert!(
        matches!(choose.def(call.args[1]), ValueDef::Param { index: 1, .. }),
        "{choose}"
    );
    assert_eq!(call.args[2], join.params[0], "{choose}");
}

#[test]
fn a_captured_local_is_a_cell_and_its_uses_stay_instructions() {
    let program = compile(
        "val make = fn(start) {\n\
             var total = start\n\
             val add = fn(amount) { total = total + amount }\n\
             add(1)\n\
             total\n\
         }\n\
         val main = fn() { println(make(4)) }\n",
    );
    let functions = lift_all(&program);
    let make = find(&functions, 1, "MakeClosure");
    let cell = make
        .cells
        .iter()
        .position(|cell| *cell)
        .unwrap_or_else(|| panic!("no local is a cell\n{make}"));
    let text = make.to_string();
    assert!(text.contains(&format!("GetLocal({cell})")), "{text}");
    for block in &make.blocks {
        for inst in &block.insts {
            assert_eq!(inst.snapshot.locals[cell], Local::Cell, "{text}");
        }
    }
}

#[test]
fn snapshots_hold_the_frame_before_each_instruction() {
    let program = compile(
        "val step = fn(a, b) { val sum = a + b\n sum * (a - b) }\n\
         val main = fn() { println(step(3, 1)) }\n",
    );
    let functions = lift_all(&program);
    let step = find(&functions, 2, "Multiply");
    let mut checked = 0;
    for block in &step.blocks {
        for inst in &block.insts {
            assert_eq!(inst.snapshot.locals.len(), step.locals, "{step}");
            if matches!(inst.op, Op::Add | Op::Subtract | Op::Multiply) {
                // The operands are on top of the stack just before.
                assert!(inst.snapshot.stack.ends_with(&inst.args), "{step}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 3, "{step}");
    // `sum` is set by the time `Multiply` runs.
    let multiply = step
        .blocks
        .iter()
        .flat_map(|block| &block.insts)
        .find(|inst| matches!(inst.op, Op::Multiply))
        .expect("a multiply");
    let add = step
        .blocks
        .iter()
        .flat_map(|block| &block.insts)
        .find(|inst| matches!(inst.op, Op::Add))
        .expect("an add");
    assert_eq!(
        multiply.snapshot.locals[2],
        Local::Value(add.results[0]),
        "{step}"
    );
}

fn slug_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            slug_sources(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "slug")
        {
            found.push(path);
        }
    }
}

#[test]
fn every_chunk_of_the_repository_sources_lifts_and_checks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    for directory in [
        "tests/conformance",
        "clutch",
        "lib",
        "crates/slug-frontend/benches/source/slug",
    ] {
        slug_sources(&root.join(directory), &mut sources);
    }
    sources.sort();
    let (mut programs, mut chunks) = (0, 0);
    let mut failures = Vec::new();
    for path in &sources {
        let source = std::fs::read_to_string(path).expect("read source");
        // Sources that need their imports to compile are skipped; most do
        // not.
        let Ok(program) = slug_frontend::compile(&path.to_string_lossy(), &source) else {
            continue;
        };
        programs += 1;
        for chunk in 0..program.chunk_count() {
            chunks += 1;
            if let Err(message) = lift(&program, chunk).and_then(|function| function.check()) {
                failures.push(format!("{} chunk {chunk}: {message}", path.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        programs >= 100,
        "only {programs} of {} sources compiled",
        sources.len()
    );
    eprintln!(
        "lifted {chunks} chunks from {programs} of {} sources",
        sources.len()
    );
}
