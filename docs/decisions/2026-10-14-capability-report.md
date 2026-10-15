# Programs have a capability report

## Status

Accepted

## Context

A reviewer of agent-written Slug code needs to know what the code can reach
beyond its own Slug code: which modules it loads, which host functions it
declares, and which host-supplied globals it uses. The information exists in
compiled module metadata and the frontend's module graph, but no tool presented
it. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

`slug report` writes a versioned JSON capability report, defined in
[Capability Report](../reference/capability-report.md), for a source program or
a compiled `.cslug` artifact.

- The report is built from the same whole-program walk that `slug compile`
  uses, so it lists exactly the modules a compiled artifact would contain.
- Host-supplied globals are found from each module's referenced global names
  minus its own bindings. A name that is neither must be implicit, or the
  module would not compile.
- `import` is a special form, so every call site is visible to the frontend. A
  call with a non-literal argument is reported as a dynamic import with its
  location, and a source report is marked open. A compiled artifact resolves
  imports only through its closed table, so its report is marked closed.
- Foreign parameter details come from source. Artifacts keep only arity, so an
  artifact report omits them rather than changing the `.cslug` format.
- Modules are named by their import names and the entry by the path as given,
  never by resolved filesystem paths, so the same program reports identically
  on any machine.
- The frontend owns the report's content as plain Rust data; the CLI crate
  owns its JSON rendering, so the frontend gains no serialization dependency.

## Consequences

### Positive

- Reviewers can compare two reports to see a change's new reach.
- Embedders can build the same report through `ModuleGraph::capability_report`
  or `slug_frontend::capability_report`.

### Negative

- Native provider identity, configuration keys, tag arguments, and limits are
  not reported yet.
- A source report's module list is not a bound when `dynamic_imports` is
  non-empty.

### Neutral

`Program::global_references` exposes the pooled global names already stored
in every program.

## Migration

None.
