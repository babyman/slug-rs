# Agent-Facing Direction

This document records a product direction and the dependency-ordered work it
implies. It is not a source-language specification. The documents under
[`language/`](../language/README.md) remain authoritative for syntax and
observable behavior.

## Position

Slug is a small, safe, specified language for code that agents write and humans
review. It runs under hard limits from microcontroller to server. The advantage
is not syntax. It is that Slug's behavior is legible, reproducible, and
contained, which is hard for a mainstream language to retrofit.

## Current state

Assessed on 2026-10-13 against the repository.

| Area | Exists | Gap |
| --- | --- | --- |
| Structured errors | `--diagnostic-format=json` version 1 (see [Errors](../language/errors.md)): `category`, `kind`, `message`, point `location`, `frames`, `cause`. | `kind` is null for source errors, so there are no stable codes. Locations are points, not spans. No expected-versus-actual fields or edits. The interactive server emits a separate protocol diagnostic shape (`unknown_method`, `malformed_json`). |
| Capability surface | Typed native bindings in `slug-native-manifest`, `foreign` declarations, the deny-all `NilLoader` used by `slug-restricted-host`, and a module model that retains doc blocks and evaluated tags. | As of 2026-10-14, `slug report` lists modules, imports, dynamic import sites, builtins, foreign declarations, and resources. `slug.meta` introspection, native provider identity, `cfg` keys, and limits are not reported. |
| Metering | `.cslug` decode limits only. | The VM has no instruction or memory budget. Map enumeration order is unspecified (Runtime Requirements), which blocks exact replay. |
| Sessions | The interactive server provides `initialize`, `session.open`, `session.close`, `submit`, and `session.poll`. | No snapshot, rollback, fork, or structured state inspection. Shared mutable captures and native resource handles make a full fork hard. |
| Spec size | The language specification and runtime requirements total about 12.5k words, roughly 17k tokens. | No `describe` query for a binding. |
| Value interchange | Encoding only, as of 2026-10-13: [Value JSON Form](../reference/value-json.md). | Decoding, needed for replay records and cross-VM transfer. |

## Work, in dependency order

### 1. Value JSON form

- [x] Define a JSON form for every value type, including how native and
  resource values appear when they cannot be serialized. See
  [Value JSON Form](../reference/value-json.md).
- [x] Use it for thrown payloads in JSON diagnostics and interactive results.

This comes first because items 3–5 all need it.

### 2. Capability report

- [x] Produce a structured report for a program: imported modules, dynamic
  import sites, host builtins, `foreign` declarations with signatures, and
  resources. See [Capability Report](../reference/capability-report.md).
- [x] Provide an equivalent host query over the retained module model:
  `ModuleGraph::capability_report` and `slug_frontend::capability_report`.
- [ ] Report native provider identity, `cfg` keys, tag arguments, and limits.
- [ ] Implement `slug.meta` introspection.
- [ ] Provide a report diff between two program versions for review. Plain
  `diff` of two reports works today.

This is the cheapest item, because the data already exists. It is also the most
distinctive one for reviewers.

### 3. Errors that carry a fix

- [ ] Unify CLI and interactive-server diagnostics under one versioned schema.
- [x] Assign stable codes to parse, semantic, runtime, and startup errors. See
  [Diagnostic Codes](../reference/diagnostic-codes.md).
- [ ] Split the shared `module` code into module resolution failure classes.
- [x] Give parse and semantic errors start and end ranges.
- [ ] Give runtime errors and stack frames ranges; this changes `.cslug` span
  tables.
- [ ] Report an unknown type annotation at the annotation, not the value.
- [ ] Reject a name that no scope, import, or implicit builtin declares at
  compile time with `unknown_name`. Today `println(missing)` compiles and fails
  at runtime with `name`. Host builtins registered outside `slug.builtin`, and
  top-level bindings used before they initialize, must still fail only at
  runtime.
- [ ] Add expected-versus-actual fields where the checker knows both.
- [ ] Add machine-applicable edits where a fix is unambiguous.

### 4. Deterministic metering and replay

- [ ] Specify map enumeration order, or fix it per run from a recorded seed.
- [ ] Add instruction and memory budgets. Count at loop back-edges and calls,
  not on every instruction, to protect the VM optimization work. The
  cooperative scheduler's yield points are the natural check sites.
- [ ] Record every nondeterministic input at the host boundary so a failed run
  replays exactly.

### 5. Session rollback and fork

- [ ] Expose session state as structured data using the value JSON form.
- [ ] Add snapshot and rollback within one session.
- [ ] Add fork later, with defined behavior for native resource handles.

### Cheaper follow-ups

- [ ] A `describe` query for any binding, using retained documentation and tags.
- [ ] Typed holes that report the expected type and the values in scope.
- [ ] One canonical format and a stable JSON form of the syntax tree.
- [ ] Documentation examples and simple property checks that run as tests.

## Measuring it

Compare Slug with a feature against Slug without it, not against a mainstream
language, because agents have far more training exposure to mainstream
languages. Candidate measures:

- fix attempts per error, with fix-carrying diagnostics versus message text only
- time for a reviewer to find seeded bugs, with and without the capability diff

Then get one real embedder. An agent harness that uses `slug-restricted-host`
as its scripting layer would exercise items 1–3 together.
