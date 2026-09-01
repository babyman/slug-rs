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
