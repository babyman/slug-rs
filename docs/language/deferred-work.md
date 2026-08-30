# Deferred Work

`defer expression` registers a zero-argument action for its enclosing lexical
scope. `defer onsuccess expression` registers an action that runs only after
successful scope completion. `defer onerror(name) expression` registers a
one-argument action that runs only during error unwinding.

Actions run in last-in, first-out order. Plain actions run for both outcomes.
When an `onerror` action runs, `name` receives the thrown Slug value, or a
fault map with string keys `type`, `msg`, and `data` for a checked VM fault.

An `onerror` action that returns normally recovers the active error. Its result
becomes its enclosing function's result and the caller continues; pending
actions in that function then run as successful cleanup. Throwing from a deferred action
replaces the active error and records that error as its cause.

The language-wide semantics and runtime requirements in
`language-specification.md` and `runtime-requirements.md` take precedence if
this focused note conflicts with them.
