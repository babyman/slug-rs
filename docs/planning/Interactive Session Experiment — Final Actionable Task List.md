# Interactive Session Experiment — Final Actionable Task List

## Goal

Finish the interactive session experiment with a clean architectural boundary between:

```text
client UI
    ↓
session protocol
    ↓
server-owned interactive session
    ↓
compiler + runtime
    ↓
shared VM
```

The experiment has already demonstrated the important architecture:

- persistent compiler state,
- persistent runtime state,
- multiple isolated sessions,
- one shared VM,
- task-backed sessions in the full runtime,
- host-driven execution in the slim runtime,
- structured diagnostics,
- protocol-framed output,
- a thin terminal client.

The remaining work should tighten the boundaries revealed by implementation rather than introduce new capabilities.

---

# 1. Move Multiline Input State Into the Server

## Goal

Interactive source accumulation belongs to the server session, not `slug-repl`.

Current shape:

```text
slug-repl
    input buffer
    source completeness logic
        ↓
      submit
        ↓
      server
```

Target:

```text
slug-repl
    read line
       ↓
    protocol
       ↓
session server
    pending source
    parser feedback
    compiler state
    runtime state
```

## Tasks

- [x] Add pending interactive source state to each session.
- [x] Remove the multiline source buffer from `slug-repl`.
- [x] Remove direct `source_is_incomplete()` usage from `slug-repl`.
- [x] Remove any parser/frontend dependency used by the REPL for multiline handling.
- [x] Send each entered line to the server.
- [x] Preserve line boundaries when accumulating fragments.
- [x] Keep pending input isolated between sessions.
- [x] Release pending input when a session closes.

## Architectural Rule

> Interactive source state belongs to the session. Clients provide input and render frontend feedback; they do not implement the Slug frontend.

---

# 2. Make `submit` Incremental

Each `submit` should contribute source to the current pending submission.

Conceptually:

```text
submit(fragment)
      ↓
append to pending source
      ↓
classify accumulated source
      │
      ├── incomplete
      ├── complete
      └── invalid
```

## Tasks

- [x] Append submitted source to the session's pending input.
- [x] Preserve newline boundaries.
- [x] Determine whether the accumulated source is complete, incomplete, or invalid.
- [x] Do not compile or execute incomplete source.
- [x] Retain incomplete source for the next request.

Example:

```text
submit "val add = fn(a, b) {"
→ incomplete

submit "a + b"
→ incomplete

submit "}"
→ complete
→ compile
→ execute
```

---

# 3. Expose Source Readiness Explicitly

The server needs a richer distinction than a simple completeness boolean.

Conceptually:

```rust
enum SourceReadiness {
    Complete,
    Incomplete,
    Invalid(SourceError),
}
```

The exact representation should follow the existing parser architecture rather than forcing this specific type.

## Tasks

- [x] Distinguish incomplete syntax from invalid syntax.
- [x] Preserve the existing structured source diagnostic.
- [x] Do not mistake semantic/type failures for incomplete source.
- [x] Keep readiness analysis in frontend/library code.
- [x] Remove this responsibility entirely from terminal clients.

`source_is_incomplete()` may remain as an implementation detail during the experiment.

---

# 4. Represent Incomplete Input Through the Protocol

When input is incomplete, `submit` succeeds as a protocol operation but does not execute anything.

For example:

```json
{
  "id": 10,
  "session": "s1",
  "method": "submit",
  "params": {
    "source": "val add = fn(a, b) {"
  }
}
```

may return:

```json
{
  "id": 10,
  "session": "s1",
  "ok": true,
  "result": {
    "state": "incomplete"
  }
}
```

The exact representation may follow the existing result schema.

## Tasks

- [x] Represent incomplete source explicitly.
- [x] Preserve the pending source.
- [x] Do not modify compiler state.
- [x] Do not modify runtime state.
- [x] Do not create an execution.

---

# 5. Complete and Execute Accumulated Source

Once accumulated source is complete:

```text
pending source
      ↓
parse
      ↓
semantic/type-check
      ↓
compile
      ↓
execute
      ↓
commit compiler state
      ↓
clear pending source
```

## Tasks

- [x] Compile the complete accumulated submission.
- [x] Preserve persistent compiler behavior.
- [x] Preserve the session runtime environment.
- [x] Return the ordinary submission result.
- [x] Clear pending input after successful completion.

## Acceptance Test

```text
submit "val add = fn(a, b) {"
→ incomplete

submit "a + b"
→ incomplete

submit "}"
→ success

submit "add(2, 3)"
→ 5
```

---

# 6. Clear Invalid or Compile-Failing Input

A genuine syntax error terminates the pending submission.

```text
pending source
      ↓
invalid
      ↓
structured diagnostic
      ↓
clear pending source
```

Likewise, syntactically complete source that fails semantic/type checking terminates the pending submission.

## Tasks

- [x] Return the canonical structured diagnostic.
- [x] Clear pending source after genuine syntax failure.
- [x] Clear pending source after semantic/type-check failure.
- [x] Do not commit compiler state.
- [x] Do not modify persistent bindings.
- [x] Leave the session immediately reusable.

## Acceptance Test

```text
invalid submission
→ diagnostic

submit "1 + 2"
→ 3
```

This preserves the existing experiment guarantee:

> Parser, compiler, and type-check failures are transactional with respect to persistent session state.

Runtime rollback remains outside this guarantee.

---

# 7. Make `slug-repl` Fully Frontend-Agnostic

After the change, the client loop should essentially be:

```text
read line
   ↓
send submit
   ↓
response
   │
   ├── incomplete → ". "
   ├── result     → render → "> "
   └── error      → render → "> "
```

## Tasks

- [x] Remove parser imports.
- [x] Remove source completeness logic.
- [x] Remove client-owned multiline accumulation.
- [x] Render continuation prompt based solely on server response.
- [x] Render primary prompt after completion/error.
- [x] Verify the REPL has no knowledge of Slug grammar.

The terminal REPL should be:

> terminal UI + protocol client

not:

> small Slug frontend + protocol client

---

# 8. Verify the Real `slug-server` Transport

The REPL currently exercises the server engine in-process, which is valuable but does not prove the actual process transport.

The two paths are distinct:

```text
in-process

client
   ↓
protocol object/JSON
   ↓
Server::handle_line
```

versus:

```text
real transport

external process
   ↓
NDJSON stdin
   ↓
slug-server
   ↓
server engine
   ↓
NDJSON stdout
```

## Tasks

- [x] Add at least one process-level `slug-server` integration test.
- [x] Launch the actual `slug-server` binary.
- [x] Write NDJSON requests to stdin.
- [x] Read NDJSON responses/events from stdout.
- [x] Verify one JSON object per line.
- [x] Verify responses are flushed appropriately.
- [x] Verify protocol responses and events, including structured Slug diagnostics,
  use stdout exclusively; reserve stderr for non-protocol host/process diagnostics
  and logging.
- [x] Verify program output never corrupts protocol stdout.
- [x] Verify event/response ordering is usable.

> **A Slug error is protocol data. A server-host failure is process diagnostics.**

## Suggested Smoke Test

Exercise:

```text
initialize
session.open
submit
session.close
```

through the real executable boundary.

This proves the transport, while in-process tests continue to prove server semantics cheaply.

---

# 9. Review Full/Slim `SessionExecution` Duplication

The full and slim implementations now expose essentially the same semantic session states with different runtime-specific execution carriers:

```text
Idle

Active
    execution
    compilation

Stalled
    execution
    compilation
```

The duplication was appropriate while discovering whether the two runtime models would converge.

They now appear to have converged.

## Task

Investigate whether the shared semantic state can become something conceptually like:

```rust
enum SessionExecution<E> {
    Idle,

    Active {
        execution: E,
        compilation: InteractiveCompilation,
    },

    Stalled {
        execution: E,
        compilation: InteractiveCompilation,
    },
}
```

where the execution carrier differs:

```text
full runtime
    E = InteractiveTask

slim runtime
    E = InteractiveExecution
```

## Requirements

- [x] Identify actual duplicated logic rather than refactoring merely for symmetry.
- [x] Determine whether a generic representation simplifies the implementation.
- [x] Keep full/slim runtime mechanisms distinct where they genuinely differ.
- [x] Do not introduce a complicated trait hierarchy just to eliminate a small enum.

## Decision Rule

If the abstraction makes the code simpler, keep it.

If it makes the runtime distinction harder to understand, leave the duplication.

This is cleanup, not an architectural blocker.

---

# 10. Record Submission Correlation as a Protocol Question

A stalled submission currently has an interesting request-lifetime issue.

Conceptually:

```text
request 17
    submit
      ↓
    stalled

request 22
    session.poll
      ↓
    result 42
```

The result is returned as the result of request `22`, even though semantically it completes the submission initiated by request `17`.

This is coherent RPC behavior.

It may nevertheless matter to future IDE or agent clients.

## Tasks

- [ ] Document the question.
- [ ] Verify session identity is sufficient for the current protocol.
- [ ] Do not add submission IDs unless a concrete use case requires them.

Concurrent submissions within one session remain unsupported, so no immediate ambiguity exists.

This should remain a deliberately deferred protocol decision.

---

# 11. Recognize the Detachable Execution Context

The slim implementation introduced an execution object capable of retaining:

- frames,
- operand stack,
- waiter registrations,
- progress state,
- pending execution.

Although discovered through interactive sessions, this may represent a more general host-embedding abstraction.

Conceptually:

```text
host
   │
   ├── execution context A
   ├── execution context B
   └── execution context C
          │
          ▼
       shared VM
```

## Tasks

- [ ] Record that `InteractiveExecution` may be more general than REPL/session functionality.
- [ ] Watch for another embedding use case that needs the same abstraction.
- [ ] Do not rename or generalize it solely on speculation.
- [ ] Revisit only when a second consumer appears.

This is an architectural observation, not immediate work.

---

# 12. Preserve Explicit Session Attribution for Output

The existing host-owned output sink is the correct model.

Continue enforcing:

> Background/native output must not infer session identity from ambient VM state.

## Tasks

- [x] Preserve explicit session attribution.
- [x] Verify multiline changes do not bypass the output sink.
- [x] Verify stalled/resumed submissions retain correct attribution.
- [x] Test two sessions producing output independently.
- [x] Keep raw process stdout reserved for protocol framing.

---

# 13. Add Multiline Server Tests

## Required Cases

- [x] Single-line expression.
- [x] Multiline function.
- [x] Multiline block.
- [x] Nested blocks.
- [x] Incomplete delimiter.
- [x] Incomplete string where applicable.
- [x] Clearly invalid syntax.
- [x] Type error after syntactically complete multiline source.
- [x] Successful submission after previous invalid input.
- [x] Pending source isolated across sessions.
- [x] One session can remain incomplete while another executes.
- [x] Closing a session with pending input cleans up correctly.

---

# 14. Add Thin-Client Integration Tests

## Required Cases

- [x] Primary prompt for idle session.
- [x] Continuation prompt after server reports incomplete.
- [x] Multiple continuation lines.
- [x] Return to primary prompt after completion.
- [x] Return to primary prompt after structured error.
- [x] Multiline function can be called afterward.
- [x] Client contains no parser dependency.

---

# 15. Avoid Making Double Parsing Permanent

For the experiment it is acceptable to:

```text
parse
   ↓
"is this complete?"

then later

parse
   ↓
compile
```

Do not block this work on parser refactoring.

However, record the cleaner eventual model:

```text
interactive parse
       │
       ├── Complete(AST)
       ├── Incomplete
       └── Error(SourceError)
```

Then:

```text
Complete(AST)
      ↓
semantic analysis
      ↓
compile
```

without reparsing.

## Deferred Tasks

- [ ] Determine whether the parser can naturally expose this result.
- [ ] Reuse the successful AST if doing so simplifies the compiler pipeline.
- [ ] Avoid optimizing duplicate parsing without evidence that it matters.

---

# 16. Leave Room for Input Cancellation

Once the server owns pending source, a client eventually needs a way to abandon it.

For example, terminal `Ctrl-C` might eventually map to:

```text
session.input.cancel
```

which clears:

```text
pending source
```

without closing:

```text
session
```

## For Now

- [ ] Do not implement unless it falls out cheaply.
- [ ] Ensure the session representation does not make cancellation difficult later.
- [ ] Record it as a future protocol operation.

---

# 17. Keep Runtime Failure Semantics As-Is

Do not introduce runtime rollback machinery as part of this cleanup.

Current guarantee:

```text
parse failure       → transactional
semantic failure    → transactional
type-check failure  → transactional
compile failure     → transactional
runtime failure     → no rollback guarantee
```

`DefineGlobal` and captured binding cells make runtime rollback substantially more complicated.

## Tasks

- [ ] Preserve current semantics.
- [ ] Document the distinction clearly.
- [ ] Do not delay multiline/server ownership work to solve runtime transactions.

Measure the need later.

---

# Architectural State After This Work

The intended boundary should be:

```text
                    clients
            ┌──────────┼──────────┐
            ▼          ▼          ▼
          REPL        IDE       agent
            │          │          │
            └──────────┼──────────┘
                       │
                session protocol
                       │
                       ▼
                  slug-server
                       │
                session manager
          ┌────────────┼────────────┐
          ▼            ▼            ▼
         s1           s2           s3
          │            │            │
    pending source  pending      pending
    compiler state  compiler     compiler
    runtime env     runtime      runtime
    execution       execution    execution
          │            │            │
          └────────────┼────────────┘
                       ▼
                   shared VM
```

For the full runtime:

```text
session execution
      ↓
Slug task
      ↓
scheduler
```

For the slim runtime:

```text
session execution
      ↓
detached host execution context
      ↓
host-driven VM
```

The protocol does not care which mechanism is used.

---

# Future Language-Service Direction

Moving pending source to the server establishes the correct ownership for more than multiline input.

The same session already knows:

```text
pending source
compiler snapshot
semantic environment
imports
globals
types
```

That makes it the natural future home for:

```text
completion
signature help
type-at-position
symbol lookup
expected tokens
hover information
editor diagnostics
```

A future client should be able to ask:

```text
complete(session, cursor)
```

rather than embedding enough of Slug to answer the question itself.

This should remain future work, but the multiline implementation must preserve that direction.

---

# Priority

## Finish Before Calling the Experiment Complete

- [x] Server-owned pending source.
- [x] Incremental `submit`.
- [x] Complete/incomplete/invalid distinction.
- [x] Thin parser-free `slug-repl`.
- [x] Multiline server tests.
- [x] Thin-client integration tests.
- [x] Real `slug-server` NDJSON process test.

## Review While Here

- [x] Full/slim `SessionExecution` duplication.
- [x] Explicit output/session attribution.

## Record and Defer

- [ ] Original submission/request correlation after stalls.
- [ ] Generalization of `InteractiveExecution`.
- [ ] Parser AST reuse/double parsing.
- [ ] Input cancellation.
- [ ] Runtime transactional rollback.
- [ ] Completion and other language-service operations.

---

# Completion Criteria

The experiment is ready to close when:

- [ ] `slug-repl` contains no parser or source-completeness logic.
- [ ] pending interactive source belongs entirely to the server session.
- [ ] every entered fragment crosses the session protocol.
- [ ] the server distinguishes complete, incomplete, and invalid source.
- [ ] incomplete input preserves session-local pending source.
- [ ] invalid and compile-failing submissions clear pending input without corrupting persistent state.
- [ ] multiple sessions can independently hold pending source.
- [ ] full and slim runtimes retain equivalent session semantics.
- [ ] output remains explicitly session-attributed and protocol-safe.
- [ ] the actual `slug-server` executable has been exercised through NDJSON stdin/stdout.
- [ ] the terminal REPL remains a disposable thin client.

The final architectural invariant is:

> **The server owns Slug semantics and persistent interactive state. The client owns interaction and presentation.**

Or, even shorter:

```text
client asks
server understands Slug
VM runs Slug
```
