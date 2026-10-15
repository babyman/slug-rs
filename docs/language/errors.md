# Slug Error Behavior

This supplement records the current source-visible error behavior. The
[Language Specification](language-specification.md) and
[Runtime Requirements](runtime-requirements.md) take precedence.

## Throwing and recovery

```slug
throw value

defer { cleanup() }
defer onsuccess { commit() }
defer onerror(err) { report(err) }
```

`throw` accepts any Slug value. It starts language-level error propagation.
Runtime faults, such as an invalid call, invalid index, or unknown identifier,
use the same propagation path.

A `defer` is registered in its enclosing scope. An unqualified `defer` runs
when that scope exits. `defer onsuccess` runs only after ordinary completion.
`defer onerror(name)` runs only while an error is propagating and binds its
thrown payload to `name`. If an `onerror` action returns normally, it handles
the active error. To propagate an error again, its action must execute `throw`.
An error raised by deferred work replaces the active error and retains it as a
cause.

Slug has no `try` or `catch` syntax. `defer onerror` is its recovery construct.

## Diagnostics and stack traces

A source failure is classified as a parse, semantic, module, or runtime error.
When source location information is available, the diagnostic identifies its
path, line, and column. A command-line implementation may additionally render
a source excerpt and caret when it can obtain the relevant source text; it must
fall back to a location-only diagnostic when that text is unavailable. Runtime
errors retain the thrown payload and Slug call frames beneath any excerpt.
Each frame identifies its call-site path, line, and column when available.
Frames introduced only to execute deferred work are omitted.

`stacktrace(error)` takes exactly one argument and returns a **string**. It is
valid only for the active runtime error or the payload currently bound by an
`onerror` handler; passing an unrelated value is a runtime error. It renders
the active failure, its available source location, and Slug call frames with
available call-site coordinates. If a failure replaced another while unwinding,
the rendering recursively includes the prior failure under `caused by:`.
`stacktrace` does not read source files or render source excerpts.

Diagnostic text is not a stable contract unless conformance fixture metadata
marks it exact. See Runtime Requirements, “Error observability”, for the
portable compatibility rules.

### Command-line JSON diagnostics

The `slug --diagnostic-format=json program.slug [arguments...]` command-line
mode writes each runner-generated fatal diagnostic as exactly one JSON document
to standard error. `--diagnostic-format=json` is a runner flag only before the
program path; an argument of the same spelling after the path remains a program
argument. Standard output and program-authored standard-error output retain
their usual behavior.

Version 1 documents use this shape:

```json
{
  "version": 1,
  "category": "parse",
  "kind": null,
  "code": "expected_syntax",
  "message": "expected binding name",
  "location": { "path": "example.slug", "line": 1, "column": 5 },
  "range": { "start": { "line": 1, "column": 5 }, "end": { "line": 1, "column": 6 } },
  "frames": [],
  "cause": null
}
```

`category` is `parse`, `semantic`, `module`, `runtime`, `usage`, `io`,
`host`, or `artifact`. `kind` is null for source errors and identifies runtime
or startup subtypes when available. `code` is the stable failure class listed
in [Diagnostic Codes](../reference/diagnostic-codes.md); match on `code`, not
on `message`. `location` is null when no source position is available. It is a single
point: for an operator error it is the operator, not the start of the
expression. `range` is the extent of the offending source in the same file,
from `start` up to `end`, exclusive, and starts at or before `location`. A parse
error's range covers the offending token or malformed literal; a semantic
error's range covers the offending expression, such as all of `1 + "a"`.
`range` is null for runtime and startup errors and for source errors whose
extent is not tracked. Lines and columns are 1-based and columns count Unicode
scalar values.
Runtime `frames` identify Slug call sites; `cause` recursively carries a
replacement error retained during unwinding. A document whose `kind` is
`thrown` also has a `thrown` member holding the thrown payload in the
[value JSON form](../reference/value-json.md); other documents omit it, so
`throw nil` is reported as `"thrown": null`.
