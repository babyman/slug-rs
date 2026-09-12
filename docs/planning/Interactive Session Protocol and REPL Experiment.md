# Interactive Session Protocol and REPL Experiment

## Goal

Build a small interactive Slug environment around a persistent session protocol and use it to explore how interactive execution fits naturally into the existing compiler, VM, task, channel, and host-driven runtime architecture.

The experiment should prove that Slug can:

- accept source submissions incrementally,
- compile them against persistent session state,
- execute them within a running VM,
- preserve session-local bindings between submissions,
- report failures using Slug's existing structured error representation,
- stream program output as protocol events,
- support multiple logical sessions,
- explore sessions as execution contexts within a shared VM,
- work with both the full concurrent runtime and the slim host-driven runtime.

The architecture under test is:

```text
                  client
                    │
                    │ interactive session protocol
                    ▼
              session server
                    │
           ┌────────┴────────┐
           ▼                 ▼
       frontend          shared VM
                           │
                 ┌─────────┼─────────┐
                 ▼         ▼         ▼
              session   session   session
```

The **session protocol and server are the architectural experiment**.

The terminal REPL is simply the first client.

---

# Core Principles

## 1. Sessions Are Explicit

A server may own multiple logical interactive sessions.

```text
session server
      │
      ▼
   shared VM
      │
      ├── session s1
      ├── session s2
      └── session s3
```

stdin/stdout is a transport.

It is not a session.

A session identifier represents a persistent interactive execution context, not necessarily an isolated VM.

This distinction allows the same protocol to eventually support:

```text
terminal REPL
IDE
agent
test harness
remote host
embedded device
```

without changing session semantics.

---

## 2. Prefer a Shared VM

The experiment should explicitly explore multiple sessions living within one shared VM rather than starting with one VM per session.

Conceptually:

```text
                 shared VM
                    │
          ┌─────────┼─────────┐
          ▼         ▼         ▼
         s1        s2        s3
          │         │         │
       context   context   context
```

The VM provides shared runtime machinery.

Each session provides its own interactive execution context.

Do not create one VM per session unless implementation evidence demonstrates that this is necessary.

---

## 3. Session State Is Local Unless Deliberately Shared

Sessions should have independent interactive environments.

For example:

```text
s1:
    val x = 10

s2:
    x
    → unknown binding
```

This does not imply complete runtime isolation.

The intended distinction is:

```text
shared
    VM/runtime
    runtime services
    explicitly global program state
    deliberately shared channels/resources

session-local
    interactive bindings
    compiler environment
    current submission
    execution context
```

The experiment should discover the exact boundary.

---

## 4. Sessions May Be Tasks

In the full runtime, a natural implementation candidate is a long-lived Slug task or equivalent execution context for each interactive session.

Normally:

```text
s1 task ── waiting
s2 task ── waiting
s3 task ── waiting
```

When source is submitted to `s2`:

```text
s1 task ── waiting
s2 task ── runnable → execute submission
s3 task ── waiting
```

After the submission completes, the session returns to its waiting state.

Conceptually:

```text
session
   │
   ▼
waiting for submission
   │
   │ source arrives
   ▼
compile submission
   │
   ▼
make execution runnable
   │
   ▼
execute
   │
   ├── completed → result → waiting
   │
   ├── failed    → error  → waiting
   │
   └── stalled   → wait for runtime progress
```

This should reuse Slug's existing task, scheduler, channel, and progress machinery.

Do not introduce a separate REPL scheduler.

Tasks are an implementation mechanism, not part of the session protocol.

---

## 5. The Slim Runtime Must Remain Viable

The session abstraction must not require Slug-managed concurrency.

A full runtime may naturally implement sessions using tasks:

```text
full runtime

shared VM
   ├── session task s1
   ├── session task s2
   └── session task s3
```

A slim runtime may instead have the host explicitly drive session execution:

```text
slim runtime

shared VM
   │
   └── host-managed session execution
```

The protocol should not expose this distinction.

The architectural rule is:

> Sessions describe persistent interactive execution contexts; tasks are one possible runtime mechanism for implementing them.

---

## 6. Compilation and Execution Remain Separate

The server coordinates the existing frontend/compiler and VM.

```text
source submission
      │
      ▼
frontend/compiler
      │
      ▼
compiled fragment
      │
      ▼
VM/session
```

The compiler describes Slug.

The VM executes compiled Slug.

The session server coordinates their persistent interactive lifecycle.

---

## 7. Existing Structured Errors Are Canonical

The protocol must use Slug's existing structured error representation.

Do not create a REPL-specific error or diagnostic model.

```text
parser ─────────┐
type checker ───┤
runtime ────────┼── Slug structured error
CLI ────────────┤
session server ─┤
IDE / agent ────┘
```

The wire protocol may wrap an error to associate it with a request and session, but the underlying Slug error should cross the boundary without being flattened into display text.

---

# Protocol

## Transport

The initial transport is newline-delimited JSON over stdin/stdout.

```text
stdin
    NDJSON requests

stdout
    NDJSON responses and events

stderr
    server diagnostics/logging only
```

One JSON object per line.

No length prefixes.

No framing arrays.

No external protocol dependency.

The message model should nevertheless remain independent of the transport.

---

# Message Families

The protocol has three fundamental message families:

```text
request
response
event
```

Requests initiate operations.

Responses terminate requests.

Events represent streaming or asynchronous information.

---

# Request

```json
{
  "id": 12,
  "session": "s1",
  "method": "submit",
  "params": {}
}
```

Fields:

- `id` identifies one protocol operation.
- `method` identifies the requested operation.
- `session` identifies persistent session state when applicable.
- `params` contains method-specific arguments.

Process-level operations omit `session`.

Request IDs and session IDs deliberately have different lifetimes:

```text
request id
    one protocol operation

session id
    persistent interactive context
```

---

# Success Response

```json
{
  "id": 12,
  "session": "s1",
  "ok": true,
  "result": {}
}
```

Session-scoped responses should echo the session identifier.

This is redundant for correlation but useful for tracing, logging, and simple clients.

---

# Error Response

A failed operation wraps an existing structured Slug error.

Conceptually:

```json
{
  "id": 12,
  "session": "s1",
  "ok": false,
  "error": {
    "...": "existing Slug structured error representation"
  }
}
```

The server must not flatten the error into an ad hoc message string.

The terminal client may render the structured error for humans.

Other clients retain the underlying structured representation.

---

# Event

Events are unsolicited messages associated with a session.

```json
{
  "session": "s1",
  "event": "stdout",
  "data": "hello\n"
}
```

Events do not require request IDs unless implementation experience demonstrates a need for them.

---

# Initial Methods

## `initialize`

Process-level initialization and protocol versioning.

Request:

```json
{
  "id": 1,
  "method": "initialize",
  "params": {
    "protocol": 1
  }
}
```

Response:

```json
{
  "id": 1,
  "ok": true,
  "result": {
    "protocol": 1,
    "capabilities": {
      "sessions": true
    }
  }
}
```

This establishes a simple extension point for future capabilities without requiring elaborate negotiation.

---

## `session.open`

Create a persistent interactive session.

```json
{
  "id": 2,
  "method": "session.open"
}
```

Response:

```json
{
  "id": 2,
  "ok": true,
  "result": {
    "session": "s1"
  }
}
```

The server generates session identifiers.

---

## `submit`

Submit one executable unit to a session.

Initial source form:

```json
{
  "id": 3,
  "session": "s1",
  "method": "submit",
  "params": {
    "source": "val x = 10"
  }
}
```

Response:

```json
{
  "id": 3,
  "session": "s1",
  "ok": true,
  "result": {
    "value": null
  }
}
```

Later:

```json
{
  "id": 4,
  "session": "s1",
  "method": "submit",
  "params": {
    "source": "x + 5"
  }
}
```

Response:

```json
{
  "id": 4,
  "session": "s1",
  "ok": true,
  "result": {
    "value": 15
  }
}
```

`submit` means:

> Submit this executable input to this persistent interactive session.

Source is the only input form required by the initial experiment.

---

## `session.close`

Release a session and its session-local resources.

```json
{
  "id": 5,
  "session": "s1",
  "method": "session.close"
}
```

Response:

```json
{
  "id": 5,
  "session": "s1",
  "ok": true,
  "result": null
}
```

---

# Initial Events

## `stdout`

```json
{
  "session": "s1",
  "event": "stdout",
  "data": "hello\n"
}
```

## `stderr`

```json
{
  "session": "s1",
  "event": "stderr",
  "data": "warning\n"
}
```

Program output must never appear as raw unframed protocol stdout.

Protocol stdout belongs exclusively to framed protocol messages.

---

# Relationship to ACP

The protocol deliberately resembles patterns found in ACP and similar long-lived interactive protocols:

```text
long-lived process
      ↓
structured JSON
      ↓
request correlation
      ↓
explicit sessions
      ↓
capabilities
      ↓
structured errors
      ↓
events
```

This is useful prior art, not a compatibility requirement.

The Slug protocol is intentionally narrower:

```text
ACP-like concept       Slug concept
────────────────────────────────────────
session                execution session
request                source submission
output event           runtime output event
structured error       Slug structured error
capabilities           runtime capabilities
```

Where an established protocol pattern solves the same problem cleanly, prefer the boring established pattern.

Do not add features merely to resemble ACP.

---

# Future Bytecode Submission

The protocol must not assume permanently that interactive input is source text.

A future client may contain the Slug frontend itself:

```text
desktop / IDE / host
       │
       ▼
Slug frontend
       │
       │ compiled bytecode
       ▼
session protocol
       │
       ▼
slim VM
    no parser
    no type checker
    no compiler
```

This would allow interactive development against a small frontend-less Slug runtime.

For example, an embedded target could run only:

```text
small host
    ↓
slim Slug VM
    ↓
clutches / native capabilities
```

while compilation occurs elsewhere.

A future protocol may therefore distinguish submission input such as:

```text
submit
   ├── source
   └── bytecode
```

Potential future shape:

```json
{
  "id": 20,
  "session": "s1",
  "method": "submit",
  "params": {
    "input": {
      "kind": "bytecode",
      "format": "cslug-v1",
      "data": "..."
    }
  }
}
```

The exact bytecode representation, encoding, validation, compatibility, and transport are explicitly outside the initial experiment.

The important constraint is:

> Interactive Slug must not require the frontend to reside beside the VM.

Do not prematurely design the initial source protocol around the future bytecode representation. Simply avoid architectural assumptions that would prevent it.

---

# Server Responsibilities

The session server owns:

- protocol parsing,
- request routing,
- session identifier generation,
- session lifecycle,
- frontend/compiler coordination,
- VM coordination,
- submission execution,
- runtime progress,
- propagation of existing structured errors,
- stdout/stderr event generation.

The server does not own terminal presentation.

---

# Client Responsibilities

The initial terminal client owns:

- reading terminal input,
- determining when a source submission is complete,
- sending protocol requests,
- correlating responses by request ID,
- displaying returned values,
- displaying runtime output events,
- rendering structured Slug errors,
- opening and closing its session.

It should not own:

- VM state,
- session semantics,
- runtime scheduling,
- protocol-specific compiler behavior.

The terminal client should remain intentionally small and replaceable.

---

# Session Model

The initial conceptual model is:

```text
Session
    compiler/frontend environment
    interactive bindings
    execution context
    current submission
```

The VM itself is preferably shared.

Do not create a large permanent `ReplSession` abstraction before implementation reveals what state actually needs to persist.

---

# Submission Lifecycle

A source submission is conceptually:

```text
source
   ↓
parse
   ↓
resolve / type-check
   ↓
compile against session environment
   ↓
execute in session context
   ↓
return result
   ↓
commit durable session changes
```

The experiment must determine exactly what compiler and runtime state belongs to the session.

---

# Transactional Session State

Initial rule:

> Parser, compiler, or type-check failure must not mutate persistent session state.

For example:

```slug
val x = 10
```

succeeds.

A later invalid submission fails.

Afterward:

```slug
x
```

must still return:

```text
10
```

Bindings introduced by the failed submission must not exist.

---

## Runtime Failure

Runtime-failure commit behavior should be explicitly determined during the experiment.

Preferred initial model:

> New top-level interactive bindings become durable only after successful submission completion.

If the current compiler/VM architecture makes this expensive or unnatural, record that result rather than hiding it behind additional machinery.

---

# Shared Communication

A shared VM creates an intentional path for communication between interactive sessions.

If shared/global program state exposes a channel:

```slug
val messages = chan<str>()
```

sessions may communicate using ordinary Slug semantics:

```text
             shared VM

session s1 ─────┐
                │
             messages
                │
session s2 ─────┘
```

The interactive protocol should not introduce its own inter-session messaging abstraction.

Where sessions communicate, prefer normal Slug facilities such as channels.

The experiment should test that deliberate shared state can be shared while ordinary interactive bindings remain session-local.

---

# Host-Driven Runtime Integration

The server should use the existing host-driven VM model.

Conceptually:

```text
submission
    ↓
start execution
    ↓
run_until_stalled
    ↓
Completed
Failed
Stalled
```

No session-specific scheduler should be introduced.

---

# Idle vs Stalled

Interactive sessions expose an important distinction:

```text
Idle
    session exists
    no submission currently executing
```

versus:

```text
Stalled
    submission is still active
    execution currently cannot make progress
```

For example:

```text
submit
   ↓
Stalled
   ↓
native ingress
   ↓
progress notification
   ↓
server pumps VM
   ↓
Completed
   ↓
Idle
```

Do not change the public VM API merely to satisfy the experiment.

Use the experiment to determine whether this distinction deserves first-class runtime representation.

---

# Milestone 0 — Protocol Types

## Tasks

- [ ] Define request envelope.
- [ ] Define success response envelope.
- [ ] Define error response envelope around existing Slug structured errors.
- [ ] Define event envelope.
- [ ] Define protocol version constant.
- [ ] Define `initialize`.
- [ ] Define `session.open`.
- [ ] Define `submit`.
- [ ] Define `session.close`.
- [ ] Define `stdout` event.
- [ ] Define `stderr` event.
- [ ] Add encode/decode tests.
- [ ] Reject malformed JSON cleanly.
- [ ] Reject unknown methods cleanly.
- [ ] Verify existing structured errors survive protocol serialization without information loss.

## Acceptance Criteria

Protocol messages can round-trip:

```text
JSON line
   ↓
protocol type
   ↓
JSON line
```

and existing Slug structured errors cross the boundary intact.

---

# Milestone 1 — Minimal Session Server

Build the smallest protocol server.

## Tasks

- [ ] Read NDJSON requests from stdin.
- [ ] Write NDJSON responses to stdout.
- [ ] Implement `initialize`.
- [ ] Implement `session.open`.
- [ ] Implement `session.close`.
- [ ] Store session metadata.
- [ ] Reject unknown sessions.
- [ ] Keep server logging off protocol stdout.

`submit` may remain unimplemented.

## Acceptance Criteria

This conversation works entirely over stdin/stdout:

```text
initialize
session.open
session.close
```

---

# Milestone 2 — First Persistent Submission

This is the first major architectural checkpoint.

## Tasks

- [ ] Implement source `submit`.
- [ ] Parse submitted source.
- [ ] Type-check against session context.
- [ ] Compile submitted source.
- [ ] Execute it through the existing VM.
- [ ] Return its result.
- [ ] Preserve interactive compiler state.
- [ ] Preserve runtime state required by the session.
- [ ] Propagate existing structured errors unchanged.

## Acceptance Criteria

Submit:

```slug
val x = 10
```

Then:

```slug
x + 5
```

Result:

```text
15
```

No source replay or process restart should be required between submissions unless the experiment demonstrates that replay is the cleaner implementation.

---

# Milestone 3 — Shared VM Sessions

Explore multiple sessions against one VM.

## Tasks

- [ ] Create multiple session contexts within a shared VM.
- [ ] Determine minimum session-local state.
- [ ] Keep interactive bindings session-local.
- [ ] Verify explicitly shared runtime state remains shareable.
- [ ] Verify closing one session does not damage another.
- [ ] Avoid VM-per-session unless evidence requires it.

## Acceptance Criteria

```text
s1:
    val x = 10

s2:
    val x = 20

s1: x
→ 10

s2: x
→ 20
```

while both sessions use the same underlying runtime.

---

# Milestone 4 — Task-Backed Sessions

Explore the natural full-runtime implementation.

## Tasks

- [ ] Prototype a session as a long-lived task or equivalent execution context.
- [ ] Suspend an idle session while waiting for source.
- [ ] Make it runnable when a submission arrives.
- [ ] Return it to waiting after completion.
- [ ] Preserve a genuinely stalled submission.
- [ ] Verify one stalled session does not prevent another from progressing.
- [ ] Reuse the existing scheduler and progress machinery.

## Acceptance Criteria

Conceptually:

```text
s1 ── waiting
s2 ── executing
s3 ── stalled
```

can coexist naturally inside one VM.

---

# Milestone 5 — Failure Isolation

## Tasks

- [ ] Test parser failure.
- [ ] Test type-check failure.
- [ ] Test compiler failure where applicable.
- [ ] Verify failed compilation does not mutate session state.
- [ ] Determine runtime-failure commit semantics.
- [ ] Verify runtime failure leaves session reusable.
- [ ] Verify structured error information is preserved.

## Acceptance Criteria

Valid session state survives failed submissions and partially introduced bindings do not leak into the persistent environment.

---

# Milestone 6 — Program Output Events

## Tasks

- [ ] Capture Slug stdout.
- [ ] Emit `stdout` protocol events.
- [ ] Capture Slug stderr where applicable.
- [ ] Emit `stderr` protocol events.
- [ ] Ensure program output never appears as raw protocol stdout.
- [ ] Include session identity on every event.

## Acceptance Criteria

```slug
print("hello")
```

produces:

```json
{"session":"s1","event":"stdout","data":"hello\n"}
```

followed by the terminal response for the submission.

---

# Milestone 7 — Host-Driven and Slim Sessions

Exercise the same abstraction without requiring concurrency.

## Tasks

- [ ] Run session execution through existing host-driven VM APIs.
- [ ] Determine the host-managed equivalent of task-backed sessions.
- [ ] Preserve stalled execution.
- [ ] Resume after native ingress.
- [ ] Prevent VM re-entry from external producers.
- [ ] Verify the session protocol does not depend on `concurrency`.
- [ ] Run relevant tests with `--no-default-features`.

## Acceptance Criteria

The same logical protocol can drive a slim runtime even though its session implementation does not use Slug tasks.

---

# Milestone 8 — Shared Communication

## Tasks

- [ ] Create an explicitly shared channel/resource.
- [ ] Make it visible to two session contexts.
- [ ] Send from one session.
- [ ] Receive from another.
- [ ] Verify ordinary interactive bindings remain isolated.
- [ ] Use normal Slug semantics rather than protocol-level messaging.

## Acceptance Criteria

Two sessions can deliberately communicate through the shared runtime without gaining accidental access to one another's local interactive environment.

---

# Milestone 9 — Minimal Terminal Client

Only after the protocol and session model work.

## Tasks

- [ ] Launch or connect to the session server.
- [ ] Send `initialize`.
- [ ] Open one session.
- [ ] Display a prompt.
- [ ] Read source.
- [ ] Send `submit`.
- [ ] Display returned values.
- [ ] Render output events.
- [ ] Render structured Slug errors.
- [ ] Close the session on exit.

## Initial UX

```text
Slug REPL

> val x = 10
> x + 5
15
>
```

No sophisticated terminal editing is required.

---

# Milestone 10 — Multiline Input

Multiline detection should remain a frontend/client concern rather than wire-protocol syntax.

## Tasks

- [ ] Determine whether the parser can distinguish incomplete from invalid source.
- [ ] Support multiline functions and blocks.
- [ ] Continue prompting while input is incomplete.
- [ ] Avoid magic blank-line termination unless necessary.

Example:

```text
> val add = fn(a, b) {
.   a + b
. }
> add(2, 3)
5
```

---

# Milestone 11 — Error Presentation

The structured error model already exists.

This milestone concerns presentation only.

## Tasks

- [ ] Verify parser errors expose sufficient structured information.
- [ ] Verify type errors expose sufficient structured information.
- [ ] Verify runtime errors expose sufficient structured information.
- [ ] Preserve source spans through the protocol.
- [ ] Render useful terminal diagnostics.
- [ ] Preserve the untouched structured representation for non-terminal clients.

---

# Test Matrix

## Protocol

- [ ] `initialize` succeeds.
- [ ] Invalid JSON does not terminate the server.
- [ ] Unknown method produces a structured error.
- [ ] Unknown session produces a structured error.
- [ ] Existing Slug errors serialize without information loss.

## Sessions

- [ ] Open session.
- [ ] Close session.
- [ ] Submit after close fails.
- [ ] Multiple sessions coexist.
- [ ] Closing one session leaves others usable.

## Persistent State

- [ ] Define value then read it.
- [ ] Define function then call it later.
- [ ] Earlier bindings survive compilation failure.
- [ ] Failed bindings are not committed.
- [ ] Imports behave consistently across submissions.

## Shared VM

- [ ] Multiple sessions share one runtime.
- [ ] Interactive bindings remain session-local.
- [ ] Deliberately shared state is accessible.
- [ ] Shared channel communication works.

## Runtime

- [ ] Ordinary expression returns a value.
- [ ] Runtime error leaves session usable.
- [ ] Stalled session remains alive.
- [ ] One stalled session does not block another.
- [ ] Native ingress can wake stalled execution.
- [ ] Full runtime works.
- [ ] Slim runtime works.

## Output

- [ ] stdout is emitted as an event.
- [ ] stderr is emitted as an event where applicable.
- [ ] raw program output never corrupts protocol stdout.

## Errors

- [ ] Parser failure retains existing structured error.
- [ ] Type-check failure retains existing structured error.
- [ ] Runtime failure retains existing structured error.
- [ ] Terminal client can render the error.
- [ ] Machine clients receive the same underlying representation.

---

# Suggested Initial Implementation Shape

Keep implementation boundaries provisional.

```text
src/
    repl/
        protocol.rs
        server.rs
        session.rs
        client.rs
```

or equivalent modules within the existing architecture.

Do not create separate crates simply to satisfy this diagram.

Let implementation pressure reveal the permanent boundaries.

---

# First Vertical Slice

The first meaningful implementation target is deliberately tiny.

Input:

```json
{"id":1,"method":"initialize","params":{"protocol":1}}
{"id":2,"method":"session.open"}
{"id":3,"session":"s1","method":"submit","params":{"source":"val x = 10"}}
{"id":4,"session":"s1","method":"submit","params":{"source":"x + 5"}}
```

Expected output:

```json
{"id":1,"ok":true,"result":{"protocol":1,"capabilities":{"sessions":true}}}
{"id":2,"ok":true,"result":{"session":"s1"}}
{"id":3,"session":"s1","ok":true,"result":{"value":null}}
{"id":4,"session":"s1","ok":true,"result":{"value":15}}
```

If this works with genuine persistent compiler and runtime state, the experiment has crossed its first important architectural boundary.

---

# Explicit Non-Goals

Do not implement during the initial experiment:

- TCP transport,
- HTTP/WebSocket transport,
- remote authentication,
- ACP compatibility,
- IDE integration,
- agent integration,
- bytecode submission,
- bytecode compatibility negotiation,
- debugger support,
- syntax highlighting,
- sophisticated completion,
- sophisticated terminal editing,
- session persistence across server restarts,
- clutch hot reload,
- session migration,
- concurrent submissions within one session,
- production-grade protocol compatibility guarantees.

---

# Questions the Experiment Should Answer

## Session State

- What compiler state actually needs to persist?
- What runtime state belongs to a session?
- What belongs to the shared VM?
- Are interactive globals sufficient?
- Does each submission require a synthetic module?
- How do imports behave across submissions?
- How are closures from previous submissions retained?

## Shared VM

- Can several interactive contexts naturally coexist in one VM?
- Is a session naturally represented by a task?
- What needs to be session-local?
- What can safely remain global?
- Can channels provide deliberate communication without compromising local bindings?

## Compilation

- Can the compiler naturally compile against an existing environment?
- Is a compiler-session abstraction justified?
- Can compiled submissions reference previous bindings directly?
- What state must be committed after successful execution?

## Runtime

- Can a session repeatedly execute submissions cleanly?
- Does completion leave its execution context reusable?
- Does `Idle` need first-class representation?
- How should a stalled submission interact with subsequent protocol requests?
- Does the full task-backed implementation and slim host-driven implementation share enough machinery?

## Values

- Which Slug values naturally serialize to JSON?
- How should non-JSON values be represented?
- How should closures/functions be displayed?
- How should resource handles be represented?

## Future Bytecode

- What would a compiled submission need to reference existing session state?
- What bytecode compatibility guarantees would be necessary?
- How would a frontend discover runtime capabilities?
- Can an external frontend provide a genuinely interactive experience against a frontend-less slim VM?

Do not answer these speculatively where implementation can answer them cheaply.

---

# Completion Criteria

The experiment succeeds when:

- a small server owns persistent Slug sessions,
- a small terminal REPL communicates only through the protocol,
- NDJSON stdin/stdout transport works reliably,
- existing Slug structured errors cross the boundary intact,
- state persists across submissions,
- multiple sessions coexist within a shared runtime,
- session-local bindings remain isolated,
- deliberately shared runtime state can be shared,
- the full runtime can naturally use task-backed sessions,
- the slim runtime can support the same session abstraction through host-driven execution,
- stalled execution works with existing progress machinery,
- program output is represented as protocol events,
- no separate REPL execution model is required.

The target architecture is:

```text
                        clients
               ┌──────────┼──────────┐
               ▼          ▼          ▼
             REPL        IDE       agent
               │          │          │
               └──────────┼──────────┘
                          │
                interactive session
                     protocol
                          │
                          ▼
                   session server
                    ┌─────┴─────┐
                    ▼           ▼
                frontend     shared VM
                                │
                     ┌──────────┼──────────┐
                     ▼          ▼          ▼
                    s1         s2         s3
                  context    context    context
                     │          │          │
                     └──── shared runtime ────┘
```

And a future frontend-less deployment remains possible:

```text
                  development host
                        │
                    frontend
                        │
                     bytecode
                        │
                session protocol
                        │
                        ▼
                  embedded host
                        │
                    slim VM
                        │
                 native clutches
```

The central experiment is:

> **Can Slug support multiple persistent interactive execution contexts within a shared VM, exposed through a small structured session protocol, while naturally reusing its compiler, tasks, channels, structured errors, and host-driven runtime machinery?**