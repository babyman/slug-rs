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
fn on_compiles_in_the_background_and_then_runs_the_code() {
    // The loop keeps going in the interpreter while `step` and `count`
    // compile on the tier's thread, and enters their code once installed.
    let source = "val step = fn(value) { value + 1 }\n\
                  val count = fn(remaining, total) {\n\
                      if (remaining == 0) { total } else { recur(remaining - 1, step(total)) }\n\
                  }\n\
                  val main = fn() { println(count(2000000, 0)) }\n";
    let (stdout, stderr) = traced_run(source, "on", Some("10"));
    assert_eq!(stdout, "2000000\n");
    let summary = stderr
        .lines()
        .find(|line| line.contains(" hot, "))
        .unwrap_or_else(|| panic!("no summary: {stderr}"));
    assert!(summary.contains("2 hot, 2 compiled"), "{stderr}");
    // Far fewer entries than calls: the loop ran in native code.
    let entries: u64 = summary
        .split("; ")
        .nth(1)
        .and_then(|part| part.split(' ').next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("no entry count: {summary}"));
    assert!(entries < 100_000, "{summary}");
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
fn compiled_closures_and_captured_locals_match_the_interpreter() {
    // Closures capturing locals, parameters, and captures; a captured
    // `var` changed before and after the capture, through the closure and
    // around it; a captured parameter of a function that recurs, so each
    // iteration has fresh cells; index reads and arithmetic on captured
    // locals; and a capture written by an inner closure.
    let trace = same_with_and_without_native_code(
        "closures",
        "val counter = fn(start) {\n\
             var count = start\n\
             val bump = fn() { count = count + 1; count }\n\
             bump()\n\
             count = count + 10\n\
             [bump(), count, bump(), count * 2]\n\
         }\n\
         val loop = fn(n, total, keep) {\n\
             if (n == 0) { [total, keep()] } else {\n\
                 val items = [n, n * 2]\n\
                 val first = fn() { items[0] + n }\n\
                 recur(n - 1, total + first() + items[1], first)\n\
             }\n\
         }\n\
         val nested = fn(x) {\n\
             var total = 0\n\
             val outer = fn(y) {\n\
                 val inner = fn(z) { total = total + x + y + z; total }\n\
                 inner(1)\n\
             }\n\
             outer(2)\n\
             outer(3)\n\
             [total, total + 1]\n\
         }\n\
         val main = fn() {\n\
             var index = 0\n\
             val drive = fn(k, sum) {\n\
                 if (k == 0) { sum } else {\n\
                     val c = counter(k)\n\
                     recur(k - 1, sum + c[0] + c[1] + c[2] + c[3] + loop(4, 0, fn() { 0 })[0] + nested(k)[1])\n\
                 }\n\
             }\n\
             println(drive(300, 0), loop(3, 0, fn() { 0 }), nested(5))\n\
         }\n",
    );
    // All but the module body and `inner`, whose `total = total + ...`
    // is a fused update of a capture, compile.
    let compiled = trace.matches("native: compiled").count();
    assert!(compiled >= 8, "{trace}");
    // A failure with a captured local in the frame leaves its cell to the
    // interpreter, which reports it and its trace.
    same_with_and_without_native_code(
        "closures-error",
        "val fails = fn(x) {\n\
             var v = x\n\
             val f = fn() { v }\n\
             v = v + \"s\"\n\
             f()\n\
         }\n\
         val main = fn() { println(fails(5)) }\n",
    );
}

#[test]
fn compiled_appends_slices_structs_and_copies_match_the_interpreter() {
    // Appends and prepends in a loop, slices of each kind, struct
    // construction and copies, a module variable's struct copied in place,
    // a captured list appended to, and a captured counter updated.
    let trace = same_with_and_without_native_code(
        "operations",
        "val Point = struct { x, y }\n\
         val Player = struct { name, score, items }\n\
         var player = Player { name: \"a\", score: 0, items: [] }\n\
         val build = fn(n, acc) { if (n == 0) { acc } else { recur(n - 1, acc :+ n) } }\n\
         val front = fn(n, acc) { if (n == 0) { acc } else { recur(n - 1, n +: acc) } }\n\
         val slices = fn(items) { [items[1:3], items[2:], items[:2], \"hello\"[1:4]] }\n\
         val point = fn(n) { Point { x: n, y: n * 2 } }\n\
         val moved = fn(p, n) { p copy { x: p.x + n } }\n\
         val award = fn(n) {\n\
             player = player copy { score: n }\n\
             player.score\n\
         }\n\
         val gather = fn(n) {\n\
             var items = []\n\
             var count = 0\n\
             val add = fn(v) { items = items :+ v; count = count + 1 }\n\
             val go = fn(k) { if (k == 0) { [len(items), count] } else { add(k); recur(k - 1) } }\n\
             go(n)\n\
         }\n\
         val main = fn() {\n\
             println(len(build(2000, [])), build(5, []), front(4, []))\n\
             println(slices([1, 2, 3, 4, 5]), point(3), moved(point(2), 5))\n\
             val loop = fn(k, total) { if (k == 0) { total } else { recur(k - 1, total + award(k)) } }\n\
             println(loop(100, 0), player, gather(50))\n\
         }\n",
    );
    let compiled = trace.matches("native: compiled").count();
    assert!(compiled >= 10, "{trace}");
    for (kind, body) in [
        ("append-not-a-list", "build(3, \"x\")"),
        ("slice-not-a-collection", "slices(5)"),
        ("struct-bad-field", "point(\"a\") copy { z: 1 }"),
        ("copy-unknown-field", "moved(7, 1)"),
    ] {
        same_with_and_without_native_code(
            kind,
            &format!(
                "val Point = struct {{ x, y }}\n\
                 val build = fn(n, acc) {{ if (n == 0) {{ acc }} else {{ recur(n - 1, acc :+ n) }} }}\n\
                 val slices = fn(items) {{ items[1:3] }}\n\
                 val point = fn(n) {{ Point {{ x: n, y: n }} }}\n\
                 val moved = fn(p, n) {{ p copy {{ x: p.x + n }} }}\n\
                 val main = fn() {{ println({body}) }}\n"
            ),
        );
    }
}

#[test]
fn compiled_code_calls_builtins_and_reports_their_errors_as_the_interpreter_does() {
    // `len` and `println` run once per call from native code, and a
    // builtin's error several compiled frames deep reports with the
    // interpreter's trace.
    let trace = same_with_and_without_native_code(
        "builtins",
        "val count = fn(items) { len(items) + len(\"abc\") }\n\
         val show = fn(n) { println(\"n =\", n); n }\n\
         val deeper = fn(x) { len(x) + 1 }\n\
         val middle = fn(x) { deeper(x) * 2 }\n\
         val loop = fn(k, total) {\n\
             if (k == 0) { total } else { recur(k - 1, total + count([k, k]) + middle([1, 2])) }\n\
         }\n\
         val main = fn() {\n\
             println(loop(200, 0))\n\
             show(1)\n\
             show(2)\n\
             println(middle(5))\n\
         }\n",
    );
    assert!(
        trace.contains("0 calls to functions without native code"),
        "{trace}"
    );
}

#[test]
fn compiled_matches_defaults_throws_and_maps_match_the_interpreter() {
    // `match` with literal, list, rest, map, struct, typed, and wildcard
    // cases; a parameter's default taken and skipped; map literals; and a
    // `throw` reported from compiled code.
    let trace = same_with_and_without_native_code(
        "matches",
        "val Point = struct { x, y }\n\
         val describe = fn(v) {\n\
             match v {\n\
                 0 => \"zero\"\n\
                 [a, b] => \"pair \" + a + b\n\
                 [first, ...rest] => \"list \" + first + len(rest)\n\
                 {\"k\": k} => \"map \" + k\n\
                 p: struct<Point> => \"point \" + (p.x + p.y)\n\
                 s: str => \"str \" + s\n\
                 _ => \"other\"\n\
             }\n\
         }\n\
         val area = fn(w, h = 2) { w * h }\n\
         val build = fn(n) { {\"n\": n, \"double\": n * 2} }\n\
         val checked = fn(n) { if (n > 100) { throw \"too big\" } else { n } }\n\
         val loop = fn(k, total) {\n\
             if (k == 0) { total } else {\n\
                 recur(k - 1, total + len(describe(k % 4)) + area(k) + area(k, 3) + build(k)[\"double\"] + checked(k % 50))\n\
             }\n\
         }\n\
         val main = fn() {\n\
             println(loop(300, 0))\n\
             println(describe(0), describe([1, 2]), describe([1, 2, 3]), describe({\"k\": \"v\"}))\n\
             println(describe(Point { x: 1, y: 2 }), describe(\"s\"), describe(nil), build(3))\n\
             println(checked(500))\n\
         }\n",
    );
    let compiled = trace.matches("native: compiled").count();
    assert!(compiled >= 6, "{trace}");
    same_with_and_without_native_code(
        "map-repeated-key",
        "val build = fn(k) { {k: 1, \"a\": 2} }\n\
         val main = fn() { println(build(\"b\"), build(\"a\")) }\n",
    );
}

#[test]
fn compiled_calls_move_last_reads_and_restore_them_when_declined() {
    // `add` takes the map its caller reads for the last time, and adds to
    // it in place; `keep` reads its map again after the call, so `add` must
    // not change it. `grow`, called without its default, is declined, and
    // its moved argument goes back for the interpreter to pass.
    let trace = same_with_and_without_native_code(
        "moved-arguments",
        "val grow = fn(m, k, extra = 0) { m + {[k]: k + extra} }\n\
         val add = fn(m, k) { m + {[k]: k} }\n\
         val fill = fn(n, m) {\n\
             if (n == 0) { m } else { recur(n - 1, add(grow(m, n), n + 1000)) }\n\
         }\n\
         val keep = fn(n, m) {\n\
             if (n == 0) { m } else {\n\
                 val bigger = add(m, n)\n\
                 recur(n - 1, if (len(bigger) > len(m) + 1) { bigger } else { m })\n\
             }\n\
         }\n\
         val main = fn() {\n\
             val filled = fill(300, {})\n\
             println(len(filled), filled[7], filled[1007], keep(300, {\"a\": 1}))\n\
         }\n",
    );
    assert!(
        !trace.contains(" 0 calls to functions without native code"),
        "{trace}"
    );
}

#[test]
fn direct_calls_follow_redefinitions_exits_and_the_stack_budget() {
    // `inner`, called directly once its site has a record, exits at its
    // declined call to `area`; `pick` is reassigned, which changes the
    // module table, so its site calls the new function; and `depth`'s
    // direct calls stop at the stack budget, leaving to the interpreter.
    same_with_and_without_native_code(
        "direct-calls",
        "val area = fn(w, h = 2) { w * h }\n\
         val inner = fn(k) { area(k) + 1 }\n\
         val outer = fn(n, t) { if (n == 0) { t } else { recur(n - 1, t + inner(n)) } }\n\
         var pick = fn(x) { x + 1 }\n\
         val use = fn(n, t) { if (n == 0) { t } else { recur(n - 1, t + pick(n)) } }\n\
         val depth = fn(n) { if (n == 0) { 0 } else { 1 + depth(n - 1) } }\n\
         val main = fn() {\n\
             println(outer(300, 0), use(300, 0))\n\
             pick = fn(x) { x * 2 }\n\
             println(use(300, 0), depth(100000))\n\
         }\n",
    );
}

#[test]
fn compiled_remainders_match_the_interpreter() {
    // Integer `%` runs inline: truncated, with the sign of the dividend;
    // `i64::MIN % -1`, floats, and a zero divisor go to the interpreter's
    // operation, the last an error it reports.
    same_with_and_without_native_code(
        "remainders",
        "val check = fn(a, b) { a % b }\n\
         val lcg = fn(i, seed, acc) {\n\
             if (i == 0) { acc } else {\n\
                 val next = (seed * 1103515245 + 12345) % 2147483648\n\
                 recur(i - 1, next, acc + next % 97)\n\
             }\n\
         }\n\
         val main = fn() {\n\
             println(check(7, 3), check(-7, 3), check(7, -3), check(-7, -3))\n\
             println(check(-9223372036854775807 - 1, -1), check(7.5, 2), check(-7, 2.5))\n\
             println(lcg(500, 42, 0))\n\
             println(check(1, 0))\n\
         }\n",
    );
}

#[test]
fn compiled_cleanup_actions_match_the_interpreter() {
    // Deferred actions that compile run without an interpreter frame: one
    // that returns, one that recovers an error with its value, one that
    // leaves to the interpreter at a call (`note` does not compile), and
    // one that throws. Each runs twice, since the first entry compiles it.
    same_with_and_without_native_code(
        "cleanup",
        "var log = []\n\
         val note = fn(entry) { log = log :+ entry }\n\
         val slow = fn(n) {\n\
             defer { note(-n) }\n\
             n * 2\n\
         }\n\
         val recovers = fn(n) {\n\
             defer onerror(err) { err.code + n }\n\
             if (n % 2 == 0) { throw {code: 100} }\n\
             n\n\
         }\n\
         val counts = fn(n) {\n\
             var done = 0\n\
             defer { done = done + 1 }\n\
             defer { note(slow(n)) }\n\
             defer { note(done) }\n\
             n + 1\n\
         }\n\
         val rethrows = fn(n) {\n\
             defer onerror(err) { err.code + n }\n\
             defer { if (n > 2) { throw {code: n * 10} } }\n\
             n\n\
         }\n\
         val main = fn() {\n\
             println(counts(1), counts(2), recovers(3), recovers(4), recovers(5), recovers(6))\n\
             println(rethrows(1), rethrows(2))\n\
             println(log)\n\
             println(counts(3), counts(4))\n\
             rethrows(5)\n\
         }\n",
    );
}

#[test]
fn compiled_int_bit_not_variable_writes_and_map_updates_match_the_interpreter() {
    // `int` of integers and floats, `~`, a module variable only ever
    // written by the function that compiles, and a map added to in a loop
    // (in place, its last read moving it).
    let trace = same_with_and_without_native_code(
        "updates",
        "var last = 0\n\
         val note = fn(n) { last = n }\n\
         val word = fn(i) { \"w\" + (i * 7) % 13 }\n\
         val count = fn(n, counts) {\n\
             if (n == 0) { counts } else {\n\
                 val key = word(n)\n\
                 val seen = counts[key]\n\
                 note(n)\n\
                 recur(n - 1, counts + {[key]: if (seen == nil) { int(1.9) } else { seen + int(n * 0 + 1) }})\n\
             }\n\
         }\n\
         val flip = fn(n) { ~n }\n\
         val main = fn() {\n\
             val counts = count(500, {})\n\
             println(counts, last, flip(5), flip(-1))\n\
         }\n",
    );
    assert!(trace.matches("native: compiled").count() >= 5, "{trace}");
    for (kind, body) in [
        ("int-of-a-string", "int(\"7\")"),
        ("int-out-of-range", "int(1e300)"),
        ("bit-not-of-a-float", "~1.5"),
    ] {
        same_with_and_without_native_code(
            kind,
            &format!(
                "val apply = fn(f) {{ f() }}\n\
                 val main = fn() {{ println(apply(fn() {{ {body} }})) }}\n"
            ),
        );
    }
}

#[test]
fn compiled_interpolation_and_destructuring_match_the_interpreter() {
    // Strings built by interpolation of each kind of value, and `val`
    // destructuring of lists and maps, which matches without branching.
    let trace = same_with_and_without_native_code(
        "interpolation",
        "val describe = fn(n, items) {\n\
             val [first, ...rest] = items\n\
             val {name} = {name: \"x\" + n}\n\
             \"$name: $first then ${len(rest)} of $items, ${n * 1.5}, ${n > 2}\"\n\
         }\n\
         val loop = fn(k, total) { if (k == 0) { total } else { recur(k - 1, total + len(describe(k, [k, 2, 3]))) } }\n\
         val main = fn() { println(loop(300, 0), describe(4, [nil, \"s\"])) }\n",
    );
    assert!(trace.matches("native: compiled").count() >= 2, "{trace}");
    same_with_and_without_native_code(
        "destructuring-fails",
        "val first = fn(items) { val [a, b] = items\n a + b }\n\
         val main = fn() { println(first([1, 2]), first([1])) }\n",
    );
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
