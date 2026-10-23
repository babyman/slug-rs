"""Writes this project's cases.json:

    python3 eval/agent/projects/jobs/make_cases.py
"""

import json
from pathlib import Path

PRELUDE = r'''
val jobs = import("{module}")
val J = jobs.Job
val S = jobs.Status
val sleep = fn(ms) { select { after ms } }
val value = fn(v) { fn(inputs) { v } }
val fails = fn(message) { fn(inputs) { throw Error { type: "test", msg: message } } }
var problems = []
val expect = fn(label, actual, wanted) {
    if (actual != wanted) { problems = problems :+ [label, actual, wanted] }
}
val finish = fn() {
    if (len(problems) == 0) { println("PASS") } else { println("FAIL", problems) }
}
val outcome = fn(results, name, status, result, attempts, error) {
    val got = results[name]
    if (got == nil) {
        problems = problems :+ [name, "missing from results"]
    } else {
        expect(name + " status", got.status, status)
        expect(name + " value", got.value, result)
        expect(name + " attempts", got.attempts, attempts)
        expect(name + " error", got.error, error)
    }
}
val invalidData = fn(body) {
    defer onerror(err) {
        return if (err.type == "jobs.invalid") { err.data } else { {reason: "wrong error", error: err} }
    }
    body()
    return {reason: "no error"}
}
var active = 0
var peak = 0
var events = []
val tracked = fn(name, ms, result) {
    fn(inputs) {
        events = events :+ ["start", name]
        active = active + 1
        if (active > peak) { peak = active }
        sleep(ms)
        active = active - 1
        events = events :+ ["end", name]
        result
    }
}
'''.strip("\n")

cases = []


def case(id, source, seconds=None):
    entry = {"id": id, "kind": "program", "source": source.strip("\n")}
    if seconds:
        entry["seconds"] = seconds
    cases.append(entry)


# plan

case("plan-empty", '''
expect("plan", jobs.plan([]), [])
finish()
''')
case("plan-single", '''
expect("plan", jobs.plan([J { name: "only", action: value(1) }]), [["only"]])
finish()
''')
case("plan-chain", '''
val list = [
    J { name: "c", deps: ["b"], action: value(1) },
    J { name: "b", deps: ["a"], action: value(1) },
    J { name: "a", action: value(1) },
]
expect("plan", jobs.plan(list), [["a"], ["b"], ["c"]])
finish()
''')
case("plan-diamond", '''
val list = [
    J { name: "top", action: value(1) },
    J { name: "right", deps: ["top"], action: value(1) },
    J { name: "left", deps: ["top"], action: value(1) },
    J { name: "bottom", deps: ["left", "right"], action: value(1) },
]
expect("plan", jobs.plan(list), [["top"], ["left", "right"], ["bottom"]])
finish()
''')
case("plan-waves-sorted", '''
val list = [
    J { name: "zeta", action: value(1) },
    J { name: "alpha", action: value(1) },
    J { name: "Mid", action: value(1) },
    J { name: "beta", deps: ["zeta"], action: value(1) },
    J { name: "aardvark", deps: ["zeta", "alpha"], action: value(1) },
]
expect("plan", jobs.plan(list), [["Mid", "alpha", "zeta"], ["aardvark", "beta"]])
finish()
''')
case("plan-wave-is-longest-path", '''
val list = [
    J { name: "a", action: value(1) },
    J { name: "b", deps: ["a"], action: value(1) },
    J { name: "c", deps: ["b"], action: value(1) },
    J { name: "d", deps: ["a", "c"], action: value(1) },
    J { name: "e", deps: ["a"], action: value(1) },
]
expect("plan", jobs.plan(list), [["a"], ["b", "e"], ["c"], ["d"]])
finish()
''')
case("plan-repeated-dependency", '''
val list = [
    J { name: "a", action: value(1) },
    J { name: "b", deps: ["a", "a"], action: value(1) },
]
expect("plan", jobs.plan(list), [["a"], ["b"]])
finish()
''')
case("plan-does-not-run-actions", '''
var calls = 0
val counted = fn(inputs) { calls = calls + 1 }
jobs.plan([J { name: "a", action: counted }, J { name: "b", deps: ["a"], action: counted }])
expect("calls", calls, 0)
finish()
''')

# invalid job lists

case("invalid-duplicate", '''
val data = invalidData(fn() {
    jobs.plan([J { name: "a", action: value(1) }, J { name: "b", action: value(1) }, J { name: "a", action: value(2) }])
})
expect("reason", data.reason, "duplicate")
expect("name", data.name, "a")
finish()
''')
case("invalid-first-duplicate", '''
val list = [
    J { name: "x", action: value(1) },
    J { name: "y", action: value(1) },
    J { name: "y", action: value(1) },
    J { name: "x", action: value(1) },
]
val data = invalidData(fn() { jobs.plan(list) })
expect("reason", data.reason, "duplicate")
expect("name", data.name, "y")
finish()
''')
case("invalid-duplicate-before-missing", '''
val list = [
    J { name: "a", deps: ["nowhere"], action: value(1) },
    J { name: "b", action: value(1) },
    J { name: "b", action: value(1) },
]
val data = invalidData(fn() { jobs.plan(list) })
expect("reason", data.reason, "duplicate")
expect("name", data.name, "b")
finish()
''')
case("invalid-missing", '''
val data = invalidData(fn() {
    jobs.plan([J { name: "a", action: value(1) }, J { name: "b", deps: ["a", "c"], action: value(1) }])
})
expect("reason", data.reason, "missing")
expect("name", data.name, "b")
expect("dependency", data.dependency, "c")
finish()
''')
case("invalid-first-missing", '''
val list = [
    J { name: "a", action: value(1) },
    J { name: "b", deps: ["q", "p"], action: value(1) },
    J { name: "c", deps: ["r"], action: value(1) },
]
val data = invalidData(fn() { jobs.plan(list) })
expect("reason", data.reason, "missing")
expect("name", data.name, "b")
expect("dependency", data.dependency, "q")
finish()
''')
case("invalid-self-cycle", '''
val data = invalidData(fn() { jobs.plan([J { name: "loop", deps: ["loop"], action: value(1) }]) })
expect("reason", data.reason, "cycle")
expect("names", data.names, ["loop"])
finish()
''')
case("invalid-cycle-names", '''
val list = [
    J { name: "free", action: value(1) },
    J { name: "up", deps: ["free"], action: value(1) },
    J { name: "p", deps: ["q", "up"], action: value(1) },
    J { name: "q", deps: ["p"], action: value(1) },
    J { name: "down", deps: ["q"], action: value(1) },
    J { name: "other", deps: ["free"], action: value(1) },
]
val data = invalidData(fn() { jobs.plan(list) })
expect("reason", data.reason, "cycle")
expect("names", data.names, ["down", "p", "q"])
finish()
''')
case("invalid-missing-before-cycle", '''
val list = [
    J { name: "a", deps: ["b"], action: value(1) },
    J { name: "b", deps: ["a"], action: value(1) },
    J { name: "c", deps: ["gone"], action: value(1) },
]
val data = invalidData(fn() { jobs.plan(list) })
expect("reason", data.reason, "missing")
expect("name", data.name, "c")
finish()
''')
case("invalid-run-runs-nothing", '''
var calls = 0
val counted = fn(inputs) { calls = calls + 1 }
val list = [
    J { name: "fine", action: counted },
    J { name: "a", deps: ["b"], action: counted },
    J { name: "b", deps: ["a"], action: counted },
]
val data = invalidData(fn() { jobs.run(list) })
expect("reason", data.reason, "cycle")
expect("names", data.names, ["a", "b"])
expect("calls", calls, 0)
finish()
''')
for id, concurrency in [("invalid-concurrency-zero", "0"), ("invalid-concurrency-fraction", "1.5"), ("invalid-concurrency-negative", "-2")]:
    case(id, f'''
var calls = 0
val data = invalidData(fn() {{ jobs.run([J {{ name: "a", action: fn(inputs) {{ calls = calls + 1 }} }}], concurrency = {concurrency}) }})
expect("reason", data.reason, "concurrency")
expect("calls", calls, 0)
finish()
''')

# running

case("run-empty", '''
expect("results", jobs.run([]), {})
finish()
''')
case("run-single", '''
val results = jobs.run([J { name: "only", action: value(42) }])
outcome(results, "only", S.Ok, 42, 1, nil)
expect("keys", len(results), 1)
finish()
''')
case("run-inputs", '''
var seen = nil
val list = [
    J { name: "sum", deps: ["x", "y"], action: fn(inputs) { seen = inputs; inputs["x"] + inputs["y"] } },
    J { name: "x", action: value(2) },
    J { name: "y", deps: ["x"], action: fn(inputs) { inputs["x"] * 10 } },
]
val results = jobs.run(list)
outcome(results, "x", S.Ok, 2, 1, nil)
outcome(results, "y", S.Ok, 20, 1, nil)
outcome(results, "sum", S.Ok, 22, 1, nil)
expect("inputs", seen, {x: 2, y: 20})
finish()
''')
case("run-inputs-only-direct-deps", '''
var seen = nil
val list = [
    J { name: "a", action: value("A") },
    J { name: "b", deps: ["a"], action: value("B") },
    J { name: "c", deps: ["b"], action: fn(inputs) { seen = inputs; "C" } },
]
jobs.run(list)
expect("inputs", seen, {b: "B"})
finish()
''')
case("run-no-deps-gets-empty-map", '''
var seen = nil
jobs.run([J { name: "a", action: fn(inputs) { seen = inputs; 1 } }])
expect("inputs", seen, {})
finish()
''')
case("run-any-result-value", '''
val list = [
    J { name: "list", action: value([1, [2]]) },
    J { name: "map", action: value({k: "v"}) },
    J { name: "text", action: value("t") },
    J { name: "nothing", action: fn(inputs) { nil } },
    J { name: "uses", deps: ["list", "map", "nothing"], action: fn(inputs) { [inputs["list"][1][0], inputs["map"].k, inputs["nothing"]] } },
]
val results = jobs.run(list)
outcome(results, "list", S.Ok, [1, [2]], 1, nil)
outcome(results, "map", S.Ok, {k: "v"}, 1, nil)
outcome(results, "nothing", S.Ok, nil, 1, nil)
outcome(results, "uses", S.Ok, [2, "v", nil], 1, nil)
finish()
''')
case("run-failure", '''
val results = jobs.run([J { name: "bad", action: fails("it broke") }])
outcome(results, "bad", S.Failed, nil, 1, "it broke")
finish()
''')
case("run-failure-skips-dependents", '''
var calls = 0
val counted = fn(inputs) { calls = calls + 1 }
val list = [
    J { name: "bad", action: fails("no") },
    J { name: "child", deps: ["bad"], action: counted },
    J { name: "grandchild", deps: ["child"], action: counted },
    J { name: "mixed", deps: ["ok", "grandchild"], action: counted },
    J { name: "ok", action: value(1) },
    J { name: "fine", deps: ["ok"], action: fn(inputs) { inputs["ok"] + 1 } },
]
val results = jobs.run(list)
outcome(results, "bad", S.Failed, nil, 1, "no")
outcome(results, "child", S.Skipped, nil, 0, nil)
outcome(results, "grandchild", S.Skipped, nil, 0, nil)
outcome(results, "mixed", S.Skipped, nil, 0, nil)
outcome(results, "ok", S.Ok, 1, 1, nil)
outcome(results, "fine", S.Ok, 2, 1, nil)
expect("calls", calls, 0)
expect("keys", len(results), 6)
finish()
''')
case("run-failure-does-not-cancel-siblings", '''
val list = [
    J { name: "quick-fail", action: fails("early") },
    J { name: "slow-1", action: tracked("slow-1", 60, "one") },
    J { name: "slow-2", action: tracked("slow-2", 80, "two") },
    J { name: "after-slow", deps: ["slow-1", "slow-2"], action: fn(inputs) { inputs["slow-1"] + inputs["slow-2"] } },
]
val results = jobs.run(list)
outcome(results, "quick-fail", S.Failed, nil, 1, "early")
outcome(results, "slow-1", S.Ok, "one", 1, nil)
outcome(results, "slow-2", S.Ok, "two", 1, nil)
outcome(results, "after-slow", S.Ok, "onetwo", 1, nil)
finish()
''')
case("run-retries-succeed", '''
var calls = 0
val flaky = fn(inputs) {
    calls = calls + 1
    if (calls < 3) { throw Error { type: "test", msg: "attempt $calls" } }
    "steady"
}
val results = jobs.run([J { name: "flaky", action: flaky, retries: 5 }])
outcome(results, "flaky", S.Ok, "steady", 3, nil)
expect("calls", calls, 3)
finish()
''')
case("run-retries-exhausted", '''
var calls = 0
val broken = fn(inputs) {
    calls = calls + 1
    throw Error { type: "test", msg: "attempt $calls" }
}
val list = [J { name: "broken", action: broken, retries: 2 }, J { name: "next", deps: ["broken"], action: value(1) }]
val results = jobs.run(list)
outcome(results, "broken", S.Failed, nil, 3, "attempt 3")
outcome(results, "next", S.Skipped, nil, 0, nil)
expect("calls", calls, 3)
finish()
''')
case("run-retries-get-same-inputs", '''
var seen = []
var calls = 0
val list = [
    J { name: "src", action: value(7) },
    J { name: "use", deps: ["src"], retries: 1, action: fn(inputs) {
        seen = seen :+ inputs
        calls = calls + 1
        if (calls == 1) { throw Error { type: "test", msg: "first" } }
        inputs["src"]
    } },
]
val results = jobs.run(list)
outcome(results, "use", S.Ok, 7, 2, nil)
expect("seen", seen, [{src: 7}, {src: 7}])
finish()
''')
case("run-timeout", '''
var calls = 0
val list = [
    J { name: "slow", timeout: 40, action: fn(inputs) { sleep(400); "late" } },
    J { name: "after", deps: ["slow"], action: fn(inputs) { calls = calls + 1 } },
]
val results = jobs.run(list)
outcome(results, "slow", S.TimedOut, nil, 1, "timeout")
outcome(results, "after", S.Skipped, nil, 0, nil)
expect("calls", calls, 0)
finish()
''', seconds=10)
case("run-timeout-then-retry", '''
var calls = 0
val list = [J { name: "sometimes", timeout: 50, retries: 2, action: fn(inputs) {
    calls = calls + 1
    if (calls == 1) { sleep(300) }
    "attempt $calls"
} }]
val results = jobs.run(list)
outcome(results, "sometimes", S.Ok, "attempt 2", 2, nil)
finish()
''', seconds=10)
case("run-timeout-retries-exhausted", '''
val list = [J { name: "stuck", timeout: 30, retries: 1, action: fn(inputs) { sleep(200); 1 } }]
val results = jobs.run(list)
outcome(results, "stuck", S.TimedOut, nil, 2, "timeout")
finish()
''', seconds=10)
case("run-timeout-failure-reports-failure", '''
val list = [J { name: "fast-fail", timeout: 500, action: fails("quickly") }]
val results = jobs.run(list)
outcome(results, "fast-fail", S.Failed, nil, 1, "quickly")
finish()
''', seconds=10)
case("run-timeout-not-reached", '''
val list = [J { name: "in-time", timeout: 500, action: fn(inputs) { sleep(20); "made it" } }]
outcome(jobs.run(list), "in-time", S.Ok, "made it", 1, nil)
finish()
''', seconds=10)
case("run-zero-timeout-waits", '''
val list = [J { name: "patient", action: fn(inputs) { sleep(150); "done" } }]
outcome(jobs.run(list), "patient", S.Ok, "done", 1, nil)
finish()
''', seconds=10)
case("run-timeout-abandoned-failure-is-contained", '''
val list = [
    J { name: "slow-fail", timeout: 30, action: fn(inputs) { sleep(150); throw Error { type: "test", msg: "late" } } },
    J { name: "neighbour", action: tracked("neighbour", 250, "fine") },
]
val results = jobs.run(list)
outcome(results, "slow-fail", S.TimedOut, nil, 1, "timeout")
outcome(results, "neighbour", S.Ok, "fine", 1, nil)
finish()
''', seconds=10)

for id, setting, count, wanted in [
    ("run-concurrency-default", "", 10, 4),
    ("run-concurrency-two", ", concurrency = 2", 7, 2),
    ("run-concurrency-one", ", concurrency = 1", 4, 1),
    ("run-concurrency-above-job-count", ", concurrency = 10", 5, 5),
]:
    case(id, f'''
val make = fn(i, out) {{
    if (i == {count}) {{ out }} else {{
        val name = "job-$i"
        recur(i + 1, out :+ J {{ name: name, action: tracked(name, 40, i) }})
    }}
}}
val results = jobs.run(make(0, []){setting})
expect("peak", peak, {wanted})
expect("active", active, 0)
expect("count", len(results), {count})
expect("value", results["job-3"].value, 3)
finish()
''', seconds=10)

case("run-deps-finish-before-dependents-start", '''
val list = [
    J { name: "d", deps: ["b", "c"], action: tracked("d", 10, 4) },
    J { name: "c", deps: ["a"], action: tracked("c", 10, 3) },
    J { name: "b", deps: ["a"], action: tracked("b", 50, 2) },
    J { name: "a", action: tracked("a", 30, 1) },
    J { name: "e", action: tracked("e", 5, 5) },
]
jobs.run(list, concurrency = 3)
val position = fn(event, i) {
    if (i == len(events)) { -1 } else if (events[i] == event) { i } else { recur(event, i + 1) }
}
val before = fn(dep, job) {
    expect(dep + " ends before " + job + " starts", position(["end", dep], 0) < position(["start", job], 0), true)
}
before("a", "b")
before("a", "c")
before("b", "d")
before("c", "d")
expect("events", len(events), 10)
finish()
''', seconds=10)
case("run-independent-jobs-overlap", '''
val list = [
    J { name: "root", action: tracked("root", 10, 0) },
    J { name: "left", deps: ["root"], action: tracked("left", 80, 1) },
    J { name: "right", deps: ["root"], action: tracked("right", 80, 2) },
]
jobs.run(list, concurrency = 2)
expect("peak", peak, 2)
finish()
''', seconds=10)

# log file

case("log-lines", '''
val fs = import("slug.io.fs")
val path = "jobs-log.txt"
val list = [
    J { name: "a", action: value(1) },
    J { name: "b", deps: ["a"], action: fails("x"), retries: 1 },
    J { name: "c", deps: ["b"], action: value(1) },
    J { name: "t", timeout: 20, action: fn(inputs) { sleep(200) } },
    J { name: "d", deps: ["a"], action: value(1) },
]
jobs.run(list, log = path)
val file = fs.openRead(path)
val read = fn(out) {
    val line = fs.readLine(file)
    if (line == nil) { out } else { recur(out :+ line) }
}
val lines = read([])
fs.close(file)
val s = import("slug.string")
expect("lines", s.sort(lines), ["a ok 1", "b failed 2", "c skipped 0", "d ok 1", "t timed_out 1"])
val index = fn(prefix, i) {
    if (i == len(lines)) { -1 } else if (lines[i][:len(prefix)] == prefix) { i } else { recur(prefix, i + 1) }
}
expect("a before b", index("a ", 0) < index("b ", 0), true)
expect("b before c", index("b ", 0) < index("c ", 0), true)
expect("a before d", index("a ", 0) < index("d ", 0), true)
finish()
''', seconds=10)
case("log-truncates", '''
val fs = import("slug.io.fs")
val path = "jobs-log.txt"
val old = fs.openWrite(path)
fs.write(old, "stale line one\\nstale line two\\nstale line three\\n")
fs.close(old)
jobs.run([J { name: "only", action: value(1) }], log = path)
val file = fs.openRead(path)
val first = fs.readLine(file)
val second = fs.readLine(file)
fs.close(file)
expect("first", first, "only ok 1")
expect("second", second, nil)
finish()
''')
case("log-empty-run", '''
val fs = import("slug.io.fs")
val path = "jobs-log.txt"
jobs.run([], log = path)
val file = fs.openRead(path)
val first = fs.readLine(file)
fs.close(file)
expect("first", first, nil)
finish()
''')

# size

case("size-plan-wide", '''
val make = fn(i, out) {
    if (i == 2000) { out } else { recur(i + 1, out :+ J { name: "n$i", action: value(i) }) }
}
val waves = jobs.plan(make(0, []))
expect("waves", len(waves), 1)
expect("width", len(waves[0]), 2000)
expect("first", waves[0][0], "n0")
expect("sorted", waves[0][1], "n1")
expect("sorted later", waves[0][2], "n10")
finish()
''', seconds=10)
case("size-plan-chain", '''
val make = fn(i, out) {
    if (i == 400) { out } else {
        val prev = i - 1
        val deps = if (i == 0) { [] } else { ["n$prev"] }
        recur(i + 1, [J { name: "n$i", deps: deps, action: value(i) }] + out)
    }
}
val waves = jobs.plan(make(0, []))
expect("waves", len(waves), 400)
expect("last", waves[399], ["n399"])
finish()
''', seconds=10)
case("size-run-chain", '''
val make = fn(i, out) {
    if (i == 300) { out } else {
        val name = fn(k) { "n$k" }
        val deps = if (i == 0) { [] } else { [name(i - 1)] }
        val dep = if (i == 0) { nil } else { name(i - 1) }
        recur(i + 1, out :+ J { name: name(i), deps: deps, action: fn(inputs) { if (dep == nil) { 0 } else { inputs[dep] + 1 } } })
    }
}
val results = jobs.run(make(0, []))
outcome(results, "n299", S.Ok, 299, 1, nil)
finish()
''', seconds=10)
case("size-run-wide", '''
val make = fn(i, out) {
    if (i == 1000) { out } else { recur(i + 1, out :+ J { name: "w$i", action: value(i) }) }
}
val list = make(0, []) :+ J { name: "total", deps: ["w0", "w500", "w999"], action: fn(inputs) { inputs["w0"] + inputs["w500"] + inputs["w999"] } }
val results = jobs.run(list, concurrency = 16)
expect("count", len(results), 1001)
outcome(results, "total", S.Ok, 1499, 1, nil)
finish()
''', seconds=10)

ids = [c["id"] for c in cases]
assert len(ids) == len(set(ids)), "duplicate case id"
target = Path(__file__).resolve().parent / "cases.json"
target.write_text(json.dumps({"prelude": PRELUDE, "cases": cases}, indent=1, ensure_ascii=False) + "\n")
print(len(cases), "cases")
