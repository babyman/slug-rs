use super::*;

fn traced_run(source: &str, mode: &str, threshold: Option<&str>) -> (String, String) {
    let path = fixture_path(&format!("native-{mode}-{}", threshold.unwrap_or("default")));
    fs::write(&path, source).expect("write native tier fixture");
    let mut command = slug();
    command
        .env("SLUG_NATIVE", mode)
        .env("SLUG_NATIVE_TRACE", "1")
        .arg(&path);
    match threshold {
        Some(threshold) => command.env("SLUG_NATIVE_THRESHOLD", threshold),
        None => command.env_remove("SLUG_NATIVE_THRESHOLD"),
    };
    let output = command.output().expect("run slug");
    fs::remove_file(&path).expect("remove native tier fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

/// The trace's report of each hot chunk; it also says whether each compiled.
fn hot_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("native: hot "))
        .collect()
}

const LOOPS: &str = "val step = fn(value) { value + 1 }\n\
val count = fn(remaining, total) {\n\
    if (remaining == 0) { total } else { recur(remaining - 1, step(total)) }\n\
}\n\
val once = fn() { 7 }\n\
val main = fn() { println(count(50, 0), once()) }\n";

#[test]
fn calls_and_recurs_make_a_chunk_hot_once() {
    let (stdout, stderr) = traced_run(LOOPS, "on", Some("40"));
    assert_eq!(stdout, "50 7\n");
    // `step` is called 50 times and `count` entered once and recurred 50
    // times; each is reported once. `once`, `main`, and the module body
    // stay below 40. Functions bound by `val` are anonymous chunks.
    let hot = hot_lines(&stderr);
    assert_eq!(hot.len(), 2, "{stderr}");
}

#[test]
fn all_makes_every_entered_chunk_hot_at_its_first_entry() {
    let (stdout, stderr) = traced_run(LOOPS, "all", None);
    assert_eq!(stdout, "50 7\n");
    // The module body and its four functions, each once.
    let hot = hot_lines(&stderr);
    assert_eq!(hot.len(), 5, "{stderr}");
    let mut unique = hot.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 5, "{stderr}");
}

#[test]
fn off_installs_no_tier() {
    let (stdout, stderr) = traced_run(LOOPS, "off", Some("1"));
    assert_eq!(stdout, "50 7\n");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn a_bad_setting_is_a_startup_error() {
    let path = fixture_path("native-bad");
    fs::write(&path, "val main = fn() { 1 }\n").expect("write fixture");
    let output = slug()
        .env("SLUG_NATIVE", "sometimes")
        .arg(&path)
        .output()
        .expect("run slug");
    fs::remove_file(&path).expect("remove fixture");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SLUG_NATIVE must be on, all, or off"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dump_prints_the_ir_of_each_hot_chunk_and_every_chunk_lifts() {
    let path = fixture_path("native-dump");
    fs::write(&path, LOOPS).expect("write native tier fixture");
    let output = slug()
        .env("SLUG_NATIVE", "all")
        .env("SLUG_NATIVE_TRACE", "1")
        .env("SLUG_NATIVE_DUMP", "1")
        .arg(&path)
        .output()
        .expect("run slug");
    fs::remove_file(&path).expect("remove native tier fixture");
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert_eq!(
        stderr.matches("\nfunction ").count() + usize::from(stderr.starts_with("function ")),
        5,
        "{stderr}"
    );
    assert!(stderr.contains("recur("), "{stderr}");
    assert!(!stderr.contains("cannot lift"), "{stderr}");
}

/// Runs `source` interpreted and with every qualifying chunk compiled at
/// its first entry, and requires the same output, errors, and status.
fn same_with_and_without_native_code(kind: &str, source: &str) -> String {
    let path = fixture_path(&format!("native-same-{kind}"));
    fs::write(&path, source).expect("write native tier fixture");
    let run = |mode: &str| {
        slug()
            .env("SLUG_NATIVE", mode)
            .env_remove("SLUG_NATIVE_TRACE")
            .env_remove("SLUG_NATIVE_DUMP")
            .arg(&path)
            .output()
            .expect("run slug")
    };
    let interpreted = run("off");
    let compiled = run("all");
    let traced = slug()
        .env("SLUG_NATIVE", "all")
        .env("SLUG_NATIVE_TRACE", "1")
        .arg(&path)
        .output()
        .expect("run slug");
    fs::remove_file(&path).expect("remove native tier fixture");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    assert_eq!(
        text(&compiled.stdout),
        text(&interpreted.stdout),
        "{kind}: stdout"
    );
    assert_eq!(
        text(&compiled.stderr),
        text(&interpreted.stderr),
        "{kind}: stderr"
    );
    assert_eq!(
        compiled.status.code(),
        interpreted.status.code(),
        "{kind}: status"
    );
    text(&traced.stderr)
}

#[test]
fn compiled_arithmetic_matches_the_interpreter() {
    let trace = same_with_and_without_native_code(
        "arithmetic",
        "val mix = fn(a, b) {\n\
             [a + b, a - b, a * b, a / b, a % b, a == b, a < b, a > b]\n\
         }\n\
         val ints = fn(a, b) { a * 3 - b + 7 }\n\
         val floats = fn(a, b) { a * 0.5 + b - 0.25 }\n\
         val mixed = fn(a, b) { a + b * 2 }\n\
         val compare = fn(a, b) { if (a < b) { 1 } else { if (a == b) { 0 } else { -1 } } }\n\
         val bits = fn(a, b) { (a & b) + (a | b) + (a ^ b) + (a << 2) + (b >> 1) }\n\
         val divide = fn(a, b) { a / b + a % b }\n\
         val main = fn() {\n\
             println(ints(5, 2), ints(-9, 4), floats(1.5, 2.0), mixed(1, 2.5), mixed(1.5, 2))\n\
             println(compare(1, 2), compare(2, 2), compare(3.5, 2), compare(\"a\", \"b\"))\n\
             println(bits(12, 10), divide(7, 2), divide(7.5, 2), mix(6, 4))\n\
         }\n",
    );
    assert!(trace.contains("native: compiled"), "{trace}");
}

#[test]
fn compiled_division_and_mixed_numbers_match_the_interpreter() {
    // Exact and inexact integer quotients, mixed operands both ways and
    // against constants, and equality of an integer and a float.
    let trace = same_with_and_without_native_code(
        "division",
        "val divide = fn(a, b) { a / b }\n\
         val half = fn(a) { a / 2 }\n\
         val ratio = fn(a) { 1.0 / a }\n\
         val mixed = fn(a, b) { [a + b, a - b, a * b, a < b, a > b, a == b] }\n\
         val near = fn(a) { [a + 1, a < 2, a == 2, a == 2.0, a > 1.5] }\n\
         val main = fn() {\n\
             println(divide(12, 4), divide(7, 2), divide(-7, 2), divide(-9223372036854775807 - 1, 2))\n\
             println(divide(7.5, 2), divide(3, 0.5), divide(-1, 3.0), half(9), half(10), half(5.0))\n\
             println(ratio(4), ratio(0.5), ratio(-3))\n\
             println(mixed(1, 2.5), mixed(2.5, 1), mixed(2, 2.0), mixed(-0.0, 0))\n\
             println(near(2), near(2.0), near(1.5))\n\
         }\n",
    );
    assert!(trace.contains("native: compiled"), "{trace}");
    // Each failure stops the program, so each has its own.
    for (kind, body) in [
        ("divide-int-zero", "divide(1, 0)"),
        ("divide-float-zero", "divide(1.5, 0.0)"),
        ("divide-mixed-zero", "divide(1, -0.0)"),
        ("divide-overflow", "divide(-9223372036854775807 - 1, -1)"),
        ("divide-non-finite", "divide(1.0e300, 1.0e-300)"),
        ("divide-string", "divide(\"a\", 2)"),
    ] {
        same_with_and_without_native_code(
            kind,
            &format!(
                "val divide = fn(a, b) {{ a / b }}\n\
                 val main = fn() {{ println({body}) }}\n"
            ),
        );
    }
}

#[test]
fn compiled_reads_of_globals_captures_and_collections_match_the_interpreter() {
    // Module values and variables, a variable changed between reads, a
    // closure's captures, index reads of each kind with constant and value
    // keys, string and function constants, `not`, and negation.
    let trace = same_with_and_without_native_code(
        "reads",
        "val table = {\"a\": 1, \"b\": [10, 20, 30]}\n\
         var counter = 0\n\
         val bump = fn() { counter = counter + 1 }\n\
         val peek = fn() { counter * 2 }\n\
         val look = fn(key) { [table[key], table[\"a\"], table.b[1], table[\"missing\"]] }\n\
         val local = fn(items, at) { [items[at], items[0], items[-1]] }\n\
         val words = fn(name) { if (name == \"x\") { \"is x\" } else { name + \"!\" } }\n\
         val maker = fn(base) {\n\
             val items = [base, base + 1]\n\
             fn(offset) { [base + offset, items[offset], items[1]] }\n\
         }\n\
         val invert = fn(value) { [!value, -value] }\n\
         val choose = fn(flag) { if (!(flag > 2)) { \"small\" } else { \"large\" } }\n\
         val pick = fn() { fn(value) { value * 3 } }\n\
         val main = fn() {\n\
             println(look(\"a\"), look(\"b\"), local([4, 5, 6], 1), local(\"text\", 2))\n\
             println(peek()); bump(); bump(); println(peek())\n\
             println(words(\"x\"), words(\"y\"))\n\
             val add = maker(5)\n\
             println(add(0), add(1))\n\
             println(invert(3), invert(-2.5), invert(0), choose(1), choose(5))\n\
             println(!nil, !false, !\"\", ![], pick()(4))\n\
         }\n",
    );
    // All but `bump` (a global write), `maker` and `pick` (closures),
    // and `main` (calls of closure values) compile.
    let compiled = trace.matches("native: compiled").count();
    assert!(compiled >= 8, "{trace}");
    // Each failure stops the program, so each has its own.
    for (kind, body) in [
        ("index-out-of-range", "read([1, 2], 5)"),
        ("index-not-a-collection", "read(7, 0)"),
        ("index-bad-key", "read({\"a\": 1}, [1])"),
        ("negate-string", "flip(\"a\")"),
        ("negate-smallest", "flip(-9223372036854775807 - 1)"),
    ] {
        same_with_and_without_native_code(
            kind,
            &format!(
                "val read = fn(items, at) {{ items[at] }}\n\
                 val flip = fn(value) {{ -value }}\n\
                 val main = fn() {{ println({body}) }}\n"
            ),
        );
    }
}

#[test]
fn a_slow_path_value_that_owns_memory_is_counted_and_dropped() {
    // Strings and lists go through the interpreter's own `+`; the loop
    // carries them through `recur` and a join.
    same_with_and_without_native_code(
        "owning",
        "val build = fn(remaining, text, items) {\n\
             if (remaining == 0) { [text, items] } else {\n\
                 recur(remaining - 1, text + \"ab\", if (remaining > 3) { items + [remaining] } else { items })\n\
             }\n\
         }\n\
         val main = fn() { println(build(10, \"\", [])) }\n",
    );
}

#[test]
fn truthiness_matches_the_interpreter() {
    same_with_and_without_native_code(
        "truthiness",
        "val pick = fn(value) { if (value) { 1 } else { 2 } }\n\
         val main = fn() { println(pick(nil), pick(false), pick(true), pick(0), pick(0.0), pick(\"\"), pick([])) }\n",
    );
}

#[test]
fn integer_overflow_in_compiled_code_reports_as_the_interpreter_does() {
    // The overflow happens with the first `recur` argument already on the
    // stack, so leaving native code must hand that value over too.
    let trace = same_with_and_without_native_code(
        "overflow",
        "val grow = fn(remaining, value) {\n\
             if (remaining == 0) { value } else { recur(remaining - 1, value * 1000) }\n\
         }\n\
         val main = fn() { println(grow(40, 1)) }\n",
    );
    assert!(trace.contains("native: compiled"), "{trace}");
}

#[test]
fn a_non_finite_float_in_compiled_code_reports_as_the_interpreter_does() {
    same_with_and_without_native_code(
        "non-finite",
        "val grow = fn(remaining, value) {\n\
             if (remaining == 0) { value } else { recur(remaining - 1, value * 1.0e100) }\n\
         }\n\
         val main = fn() { println(grow(10, 1.0)) }\n",
    );
}

#[test]
fn a_long_compiled_loop_gives_the_interpreters_result() {
    same_with_and_without_native_code(
        "long-loop",
        "val advance = fn(remaining, position, velocity) {\n\
             if (remaining == 0) { position } else { recur(remaining - 1, position + velocity, velocity + 0.000_001) }\n\
         }\n\
         val count = fn(remaining, total) { if (remaining == 0) { total } else { recur(remaining - 1, total + 3) } }\n\
         val main = fn() { println(advance(100_000, 0.0, 0.01), count(100_000, 0)) }\n",
    );
}

#[test]
fn compiled_calls_give_the_interpreters_results() {
    let trace = same_with_and_without_native_code(
        "calls",
        "val fib = fn(n) { if (n < 2) { n } else { fib(n - 1) + fib(n - 2) } }\n\
         val twice = fn(value) { value * 2 }\n\
         val both = fn(a, b) { twice(a) + twice(b) + fib(5) }\n\
         val sum = fn(remaining, total) {\n\
             if (remaining == 0) { total } else { recur(remaining - 1, total + both(remaining, 1)) }\n\
         }\n\
         val main = fn() { println(fib(20), sum(1000, 0), both(\"a\", \"b\")) }\n",
    );
    assert!(trace.contains("native: compiled"), "{trace}");
}

#[test]
fn a_callee_that_leaves_native_code_takes_its_callers_along() {
    // `show` calls `println`, which native code does not call: it leaves
    // before that call, and `run` and `step` leave after theirs, so the
    // interpreter finishes all three frames.
    same_with_and_without_native_code(
        "cascade",
        "val show = fn(value) { println(value)\n value + 1 }\n\
         val step = fn(value) { show(value * 2) + 1 }\n\
         val run = fn(remaining, total) {\n\
             if (remaining == 0) { total } else { recur(remaining - 1, total + step(remaining)) }\n\
         }\n\
         val main = fn() { println(run(5, 0)) }\n",
    );
}

#[test]
fn an_error_in_a_compiled_callee_reports_as_the_interpreter_does() {
    // The overflow is three native calls deep; the trace names each frame.
    same_with_and_without_native_code(
        "callee-error",
        "val grow = fn(value) { value * 1_000_000_000 }\n\
         val middle = fn(value) { grow(value) + 1 }\n\
         val outer = fn(remaining, value) {\n\
             if (remaining == 0) { value } else { recur(remaining - 1, middle(value)) }\n\
         }\n\
         val main = fn() { println(outer(5, 3)) }\n",
    );
}

#[test]
fn recursion_deeper_than_native_calls_go_continues_in_the_interpreter() {
    same_with_and_without_native_code(
        "deep",
        "val sum = fn(n) { if (n == 0) { 0 } else { n + sum(n - 1) } }\n\
         val main = fn() { println(sum(3000)) }\n",
    );
}

#[test]
fn deep_recursion_in_a_fork_stays_within_its_threads_stack() {
    // Forks run on threads with smaller stacks than the main thread's;
    // native calls stop at their budget and the interpreter goes on.
    same_with_and_without_native_code(
        "deep-fork",
        "val sum = fn(n) { if (n == 0) { 0 } else { n + sum(n - 1) } }\n\
         val main = fn() {\n\
             val a = fork { sum(20000) }\n\
             val b = fork { sum(30000) }\n\
             println(select { await a }, select { await b })\n\
         }\n",
    );
}

#[test]
fn a_fork_cancelled_during_native_recursion_stops() {
    // `fib(60)` would run for hours; its sibling's failure cancels it, and
    // compiled code sees that at its next call as the interpreter would.
    let path = fixture_path("native-cancelled-recursion");
    fs::write(
        &path,
        "val fib = fn(n) { if (n < 2) { n } else { fib(n - 1) + fib(n - 2) } }\n\
         val main = fn() {\n\
             val run = fn():num {\n\
                 defer onerror(err) { 0 }\n\
                 nursery {\n\
                     fork { fib(60) }\n\
                     fork { select { after 100 }; throw \"stop\" }\n\
                 }\n\
                 1\n\
             }\n\
             println(run())\n\
         }\n",
    )
    .expect("write cancelled recursion source");
    let mut child = slug()
        .env("SLUG_NATIVE", "all")
        .arg(&path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run cancelled recursion source");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().expect("poll slug").is_none() {
        if std::time::Instant::now() > deadline {
            child.kill().expect("stop slug");
            child.wait().expect("reap slug");
            fs::remove_file(&path).expect("remove cancelled recursion source");
            panic!("the cancelled fork kept running");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("read slug's output");
    fs::remove_file(path).expect("remove cancelled recursion source");

    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "0\n");
}

#[test]
fn an_error_below_many_native_and_interpreted_frames_reports_as_the_interpreter_does() {
    // The recursion passes the native stack budget several times, so the
    // trace mixes frames rebuilt from native code with interpreted ones.
    same_with_and_without_native_code(
        "deep-error",
        "val down = fn(n, value) { if (n == 0) { value * 9_000_000_000_000_000 } else { 1 + down(n - 1, value) } }\n\
         val main = fn() { println(down(5000, 7)) }\n",
    );
}
