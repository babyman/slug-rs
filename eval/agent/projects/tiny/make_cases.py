"""Writes this project's cases.json; checks.json, the same checks as data for
grading an implementation in Python; and python/task.md, task.md with its
interface written for Python:

    python3 eval/agent/projects/tiny/make_cases.py

Expected results are written by hand from task.md, not taken from the
reference, so a disagreement exposes a bug in one or the other.
"""

import json
from pathlib import Path

PRELUDE = r'''
val tiny = import("{module}")
var problems = []
val runs = fn(label, source, wanted) {
    defer onerror(err) {
        problems = problems :+ [label, "threw", err]
        return nil
    }
    val got = tiny.run(source)
    if (got != wanted) { problems = problems :+ [label, "got", got, "wanted", wanted] }
}
val fails = fn(label, source, kind, line, column) {
    defer onerror(err) {
        val matched = err.type == "tiny.error" && err.data.kind == kind && err.data.line == line && err.data.column == column
        if (!matched) { problems = problems :+ [label, "wrong error", err, "wanted", [kind, line, column]] }
        return nil
    }
    val got = tiny.run(source)
    problems = problems :+ [label, "no error", got]
}
val finish = fn() {
    if (len(problems) == 0) { println("PASS") } else { println("FAIL", problems) }
}
'''.strip("\n")

cases = []
checks = []


def slug_string(text):
    """A Slug double-quoted literal for `text`."""
    escaped = (
        text.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\t", "\\t")
        .replace("$", "\\$")
    )
    return f'"{escaped}"'


def source(text):
    # A raw triple-quoted Slug string drops a newline right after its opening
    # or before its closing delimiter, so sources must not start or end with one.
    assert "'''" not in text and not text.startswith("\n") and not text.endswith("\n")
    return "'''" + text + "'''"


def runs(label, program, lines):
    wanted = "[" + ", ".join(slug_string(line) for line in lines) + "]"
    return label, f"runs({slug_string(label)}, {source(program)}, {wanted})", {"source": program, "lines": lines}


def fails(label, program, kind, line, column):
    check = f"fails({slug_string(label)}, {source(program)}, {slug_string(kind)}, {line}, {column})"
    return label, check, {"source": program, "error": {"kind": kind, "line": line, "column": column}}


def case(group, *checks_, seconds=None):
    """One case per check, so a score counts checks: `group/label`."""
    for label, check, data in checks_:
        entry = {"id": f"{group}/{label.replace(' ', '-')}", "kind": "program", "source": check + "\nfinish()"}
        data = {"id": entry["id"], **data}
        if seconds:
            entry["seconds"] = data["seconds"] = seconds
        cases.append(entry)
        checks.append(data)


# Printing and values

case("print-values",
     runs("empty", "", []),
     runs("int", "print 42;", ["42"]),
     runs("negative", "print -12;", ["-12"]),
     runs("string", 'print "hi there";', ["hi there"]),
     runs("bools", "print true; print false;", ["true", "false"]),
     runs("nil", "print nil;", ["nil"]),
     runs("list", 'print [1, "a", [true, nil], []];', ['[1, "a", [true, nil], []]']),
     runs("functions", "fn named() { } print named; print fn() { }; print [named];", ["<fn named>", "<fn>", "[<fn named>]"]))
case("print-strings-in-lists",
     runs("quoted only nested", 'print "x"; print ["x"]; print [["y"]];', ["x", '["x"]', '[["y"]]']))
case("string-escapes",
     runs("escapes", r'print "a\tb\nc"; print "q\"q"; print "back\\slash";', ["a\tb\nc", 'q"q', "back\\slash"]))
case("comments-and-whitespace",
     runs("comments", "# leading\nprint 1; # trailing\n\n\t print   2 ;\r\nprint 3;", ["1", "2", "3"]),
     runs("comment at end", "print 1; # no newline", ["1"]))

# Arithmetic

case("arithmetic-precedence",
     runs("precedence", "print 1 + 2 * 3; print (1 + 2) * 3; print 10 - 3 - 2; print 2 * 3 % 4; print 100 / 10 / 5;",
          ["7", "9", "5", "2", "2"]),
     runs("unary", "print -3 * 2; print - -4; print -(1 + 2);", ["-6", "4", "-3"]))
case("division-truncates",
     runs("division", "print 7 / 2; print -7 / 2; print 7 / -2; print -7 / -2; print 0 / 5;", ["3", "-3", "-3", "3", "0"]),
     runs("remainder", "print 7 % 2; print -7 % 2; print 7 % -2; print -7 % -2; print 6 % 3;", ["1", "-1", "1", "-1", "0"]))
case("large-integers",
     runs("max", "print 9223372036854775807;", ["9223372036854775807"]),
     runs("arithmetic", "print 4611686018427387903 * 2 + 1;", ["9223372036854775807"]),
     fails("too large", "print 9223372036854775808;", "syntax", 1, 7))

# Strings and lists

case("strings",
     runs("join", 'print "ab" + "cd"; print "" + "x";', ["abcd", "x"]),
     runs("index", 'let s = "hello"; print s[0]; print s[4]; print len(s); print len("");', ["h", "o", "5", "0"]),
     runs("compare", 'print "a" < "b"; print "b" < "ab"; print "abc" <= "abc"; print "Z" < "a"; print "b" > "a";',
          ["true", "false", "true", "true", "true"]))
case("lists",
     runs("join and index", "let xs = [1, 2] + [3]; print xs; print xs[2]; print len(xs); print len([]);",
          ["[1, 2, 3]", "3", "3", "0"]),
     runs("nested", "let m = [[1, 2], [3, [4]]]; print m[1][1][0]; print m[0];", ["4", "[1, 2]"]),
     runs("index expression", "let xs = [10, 20, 30]; let i = 1; print xs[i + 1]; print [5, 6][0];", ["30", "5"]))
case("equality",
     runs("same types", 'print 1 == 1; print 1 != 2; print "a" == "a"; print nil == nil; print true != false;',
          ["true", "true", "true", "true", "true"]),
     runs("different types", 'print 1 == "1"; print nil == false; print 0 == false; print [] == nil; print 1 != "1";',
          ["false", "false", "false", "false", "true"]),
     runs("lists", "print [1, [2]] == [1, [2]]; print [1] == [1, 2]; print [] == [];", ["true", "false", "true"]),
     runs("functions", "fn f() { } fn g() { } let h = f; print f == f; print f == g; print h == f; print fn() { } == fn() { };",
          ["true", "false", "true", "false"]))

# Logic and control flow

case("logic",
     runs("operators", "print true and true; print true and false; print false or true; print false or false; print not false;",
          ["true", "false", "true", "false", "true"]),
     runs("precedence", "print true or false and false; print not true or true; print 1 < 2 and 2 < 3;", ["true", "true", "true"]),
     runs("short circuit", "print false and 1; print true or 1; print false and missing(); print true or missing();",
          ["false", "true", "false", "true"]))
case("if-else",
     runs("chain", 'fn sign(n) { if (n < 0) { return "neg"; } else if (n == 0) { return "zero"; } else { return "pos"; } } print sign(-5); print sign(0); print sign(9);',
          ["neg", "zero", "pos"]),
     runs("no else", "if (false) { print 1; } print 2;", ["2"]),
     runs("nested", "if (true) { if (false) { print 1; } else { print 2; } }", ["2"]))
case("while-loops",
     runs("sum", "let i = 0; let total = 0; while (i < 10) { total = total + i; i = i + 1; } print total;", ["45"]),
     runs("never runs", "while (false) { print 1; } print 2;", ["2"]),
     runs("nested", "let i = 0; while (i < 3) { let j = 0; while (j < i) { print [i, j]; j = j + 1; } i = i + 1; }",
          ["[1, 0]", "[2, 0]", "[2, 1]"]))

# Scopes, functions, and closures

case("scopes",
     runs("block shadows", "let x = 1; { let x = 2; print x; } print x;", ["2", "1"]),
     runs("assign outer", "let x = 1; { x = 2; } print x; if (true) { x = 3; } print x;", ["2", "3"]),
     runs("let replaces", "let x = 1; let x = x + 1; print x;", ["2"]),
     runs("loop body scope", "let i = 0; while (i < 2) { let seen = i; i = i + 1; } print i;", ["2"]))
case("functions",
     runs("call", "fn add(a, b) { return a + b; } print add(2, 3);", ["5"]),
     runs("no return", "fn nothing() { print \"side\"; } print nothing();", ["side", "nil"]),
     runs("bare return", "fn early(n) { if (n > 0) { return; } print \"not\"; } print early(1);", ["nil"]),
     runs("return from loop", "fn find(xs, v) { let i = 0; while (i < len(xs)) { if (xs[i] == v) { return i; } i = i + 1; } return -1; } print find([5, 6, 7], 7); print find([], 1);",
          ["2", "-1"]),
     runs("anonymous", "let twice = fn(f, x) { return f(f(x)); }; print twice(fn(n) { return n * 3; }, 2);", ["18"]),
     runs("argument order", 'fn show(x) { print x; return x; } fn pair(a, b) { return [a, b]; } print pair(show(1), show(2));',
          ["1", "2", "[1, 2]"]))
case("recursion",
     runs("factorial", "fn fact(n) { if (n == 0) { return 1; } return n * fact(n - 1); } print fact(10);", ["3628800"]),
     runs("mutual", "fn even(n) { if (n == 0) { return true; } return odd(n - 1); } fn odd(n) { if (n == 0) { return false; } return even(n - 1); } print even(10); print odd(7);",
          ["true", "true"]),
     runs("local recursion", "fn outer() { fn count(n) { if (n == 0) { return 0; } return 1 + count(n - 1); } return count(5); } print outer();", ["5"]))
case("closures",
     runs("counter", "fn counter() { let n = 0; return fn() { n = n + 1; return n; }; } let c = counter(); c(); c(); print c(); let d = counter(); print d(); print c();",
          ["3", "1", "4"]),
     runs("shared", "fn pair() { let v = 0; let get = fn() { return v; }; let set = fn(x) { v = x; }; return [get, set]; } let p = pair(); p[1](7); print p[0]();",
          ["7"]),
     runs("captures scope not value", "let x = 1; fn read() { return x; } x = 2; print read();", ["2"]),
     runs("adder", "fn adder(n) { return fn(x) { return x + n; }; } let add5 = adder(5); print add5(10); print adder(1)(1);", ["15", "2"]))
case("builtins",
     runs("len and str", 'print len("abc"); print len([1, [2, 3]]); print str(12) + "!"; print str([1, "a"]); print str("s"); print str(nil);',
          ["3", "2", "12!", '[1, "a"]', "s", "nil"]),
     runs("str of function", "fn f() { } print str(f) + str(fn() { });", ["<fn f><fn>"]),
     runs("replaceable", "fn len(x) { return 99; } print len([]);", ["99"]))

# Errors: syntax

case("syntax-errors-lexical",
     fails("bad character", "let x = 1 @ 2;", "syntax", 1, 11),
     fails("bad character line 2", "print 1;\n  print $;", "syntax", 2, 9),
     fails("unterminated string", 'print "abc', "syntax", 1, 7),
     fails("string across lines", 'print "ab\ncd";', "syntax", 1, 7),
     fails("bad escape", r'print "a\qb";', "syntax", 1, 9),
     fails("too large", "print 99999999999999999999;", "syntax", 1, 7),
     fails("tab counts one", "\tprint @;", "syntax", 1, 8))
case("syntax-errors-grammar",
     fails("missing operand", "print (1 + ;", "syntax", 1, 12),
     fails("missing semicolon", "print 1\nprint 2;", "syntax", 2, 1),
     fails("end of input", "print 1", "syntax", 1, 8),
     # Slug drops a newline just before a closing `'''`, so the source
     # ends in spaces to end on line 2.
     fails("end after newline", "print 1 +\n  ", "syntax", 2, 3),
     fails("unclosed block", "if (true) { print 1;", "syntax", 1, 21),
     fails("let needs name", "let 5 = 1;", "syntax", 1, 5),
     fails("keyword as name", "let while = 1;", "syntax", 1, 5),
     fails("assign to expression", "1 = 2;", "syntax", 1, 3),
     fails("else without block", "if (true) { } else print 1;", "syntax", 1, 20),
     fails("trailing comma", "print [1, ];", "syntax", 1, 11),
     fails("parameter must be name", "fn f(1) { }", "syntax", 1, 6))
case("syntax-errors-return",
     fails("top level", "return 1;", "syntax", 1, 1),
     fails("in top-level block", "if (true) {\n  return;\n}", "syntax", 2, 3),
     runs("in nested function", "fn f() { let g = fn() { return 1; }; return g(); } print f();", ["1"]))
case("syntax-errors-before-running",
     fails("nothing runs", "print 1;\nprint 2 +;", "syntax", 2, 10))

# Errors: runtime

case("name-errors",
     fails("read", "print 1;\nprint missing;", "name", 2, 7),
     fails("assign", "  ghost = 1;", "name", 1, 3),
     fails("out of scope", "{ let inner = 1; } print inner;", "name", 1, 26),
     fails("callee", "nope(1);", "name", 1, 1))
case("type-errors",
     fails("add mixed", 'print 1 + "a";', "type", 1, 9),
     fails("subtract strings", 'print "a" - "b";', "type", 1, 11),
     fails("negate string", 'print -"a";', "type", 1, 7),
     fails("compare mixed", 'print 1 < "2";', "type", 1, 9),
     fails("compare lists", "print [1] < [2];", "type", 1, 11),
     fails("and operand", "print 1 and true;", "type", 1, 9),
     fails("or right operand", "print false or 2;", "type", 1, 13),
     fails("not operand", "print not 0;", "type", 1, 7),
     fails("if condition", "let x = 1;\nif (x) { print 1; }", "type", 2, 1),
     fails("while condition", "while (nil) { }", "type", 1, 1),
     fails("call non-function", "let x = 1; x();", "type", 1, 13),
     fails("index non-list", "let n = 5; print n[0];", "type", 1, 19),
     fails("index by string", 'print [1, 2]["0"];', "type", 1, 13),
     fails("len of number", "print len(5);", "type", 1, 10))
case("division-and-index-errors",
     fails("divide by zero", "let z = 0;\nprint 10 / z;", "division", 2, 10),
     fails("remainder by zero", "print 10 % 0;", "division", 1, 10),
     fails("index too large", "let xs = [1, 2];\nprint xs[2];", "index", 2, 9),
     fails("negative index", "print [1][-1];", "index", 1, 10),
     fails("string index", 'print "ab"[5];', "index", 1, 11))
case("arity-errors",
     fails("too many", "fn f(a) { return a; } f(1, 2);", "arity", 1, 24),
     fails("too few", "fn f(a, b) { } print f(1);", "arity", 1, 23),
     fails("builtin", "print len();", "arity", 1, 10))
case("errors-stop-the-run",
     fails("after output", "print 1;\nprint 2;\nprint 1 / 0;\nprint 3;", "division", 3, 9),
     fails("inside function", "fn f(x) {\n  return x[3];\n}\nprint f([1]);", "index", 2, 11),
     fails("in argument", "fn f(x) { return x; }\nprint f(1 + nil);", "type", 2, 11))

# Size

case("size-loop", runs("loop", "let i = 0; let total = 0; while (i < 20000) { total = total + i; i = i + 1; } print total;", ["199990000"]),
     seconds=10)
case("size-recursion",
     runs("fib", "fn fib(n) { if (n < 2) { return n; } return fib(n - 1) + fib(n - 2); } print fib(18);", ["2584"]),
     runs("deep", "fn down(n) { if (n == 0) { return 0; } return 1 + down(n - 1); } print down(2000);", ["2000"]),
     seconds=10)
case("size-lists",
     runs("build", "let xs = []; let i = 0; while (i < 1000) { xs = xs + [i * i]; i = i + 1; } print len(xs); print xs[999];",
          ["1000", "998001"]),
     runs("sieve", "fn primes(n) { let flags = []; let i = 0; while (i < n) { flags = flags + [true]; i = i + 1; } let out = []; let p = 2; while (p < n) { if (flags[p]) { out = out + [p]; let m = p * p; while (m < n) { let before = flags; flags = []; let k = 0; while (k < len(before)) { if (k == m) { flags = flags + [false]; } else { flags = flags + [before[k]]; } k = k + 1; } m = m + p; } } p = p + 1; } return out; } print primes(40);",
          ["[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37]"]),
     seconds=20)
case("size-program-text",
     runs("many statements", "".join(f"let v{i} = {i};\n" for i in range(800)) + "print v799 + v1;", ["800"]),
     seconds=10)

ids = [c["id"] for c in cases]
assert len(ids) == len(set(ids)), "duplicate case id"
target = Path(__file__).resolve().parent / "cases.json"
target.write_text(json.dumps({"prelude": PRELUDE, "cases": cases}, indent=1, ensure_ascii=False) + "\n")
target.with_name("checks.json").write_text(json.dumps({"checks": checks}, indent=1, ensure_ascii=False) + "\n")

# The Python task shares task.md's definition of Tiny word for word; only the
# interface before it differs.
PYTHON_INTERFACE = """\
# Tiny interpreter

Write a Python module, `./tiny.py`, that interprets Tiny, a small scripting
language defined below. It will be graded by a hidden suite of programs that
import it with `import tiny` and run under `python3` (Python 3.9), with only
the standard library.

Define `run` and `TinyError`, described below.

## `run(source)`

Runs the Tiny program `source`, a `str`, and returns the list of lines it
printed, as a `list` of `str`, in order. A program that prints nothing returns
`[]`.

An invalid or failing program raises `TinyError`, a subclass of `Exception`,
with attributes `kind`, `line`, and `column`, where `kind` is one of the kinds
below and `line` and `column` give the position the error is reported at.
Lines and columns count from 1; a column counts characters, and a tab is one
character. The first error ends the run, and nothing printed before it is
returned.

"""
task = target.with_name("task.md").read_text()
definition = task[task.index("## Lexical structure"):]
assert definition.count("`tiny.slug`") == 1
python_task = target.parent / "python" / "task.md"
python_task.write_text(PYTHON_INTERFACE + definition.replace("`tiny.slug`", "`tiny.py`"))
print(len(cases), "cases")
