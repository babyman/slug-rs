# Slug Language Documentation

## Implementation status

These documents define Slug's target conformance surface. The Rust VM in this
repository implements only the subset listed in
[`../generated/language-support.md`](../generated/language-support.md). A
specified feature is not implemented merely because it appears in this
directory.

## Purpose

This directory is the normative language package. The Rust implementation in
this repository is Slug's reference implementation; the earlier Go
implementation is legacy and no longer consulted. The reference does not
define the language, though: these documents do, and another implementation
needs only them, the public library sources, and the conformance fixtures, not
the Rust source, its bytecode, or its value representations.

Start with this file, then read the documents in the order shown below. The
keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** in the requirements are
normative.

## Contents

| File | Role |
|---|---|
| `language-specification.md` | Source syntax and language semantics. |
| `slug.ebnf` | Parser grammar derived from the accepted source syntax. |
| `runtime-requirements.md` | Observable evaluation, module, diagnostic, configuration, and concurrency requirements. |
| `configuration.md` | The `cfg` contract, configuration sources, namespaces, precedence, and conversions. |
| `automatic-semicolon-insertion.md` | Automatic statement termination rules. |
| `errors.md` | Error categories and diagnostic behavior. |
| `deferred-work.md` | Deferred-action execution and recovery behavior. |
| `filesystem.md` | Opaque file resources and explicit lifecycle. |
| `embedded-host-modules.md` | Optional GPIO and blocking-delay module contracts for selected hosts. |
| `nominal-types.md` | Nominal native resource handles, fieldless enums, and transparent aliases. |
| `maps.md` | Map literals, keys, and operations. |
| `match-and-destructuring.md` | Match expressions and destructuring patterns. |
| `structs.md` | Schema identity, construction, defaults, field access, and equality. |
| `strings.md` | String literals and string operations. |
| `standard-input.md` | Process-standard-input stream and interactive helpers. |
| `signals.md` | Interrupt and terminate requests as channel messages. |
| `networking.md` | TCP listeners and connections, reading, writing, and backpressure. |
| `value-pinning-in-match-patterns.md` | Pinning existing values in match patterns. |
| `variadic-functions-and-spread-syntax.md` | Variadic parameters and spread arguments. |
| `testing.md` | Guide, not normative: testing a program's logic in one `slug` run with `slug.test`. |

The package also includes focused supplemental notes for automatic semicolon
insertion, errors, deferred work, maps, matching and destructuring, structs,
strings, value pinning, and variadic functions with spread syntax. They expand individual topics. If a
supplemental note conflicts with the Language Specification or Runtime
Requirements, the latter documents take precedence.

The Language Specification and Runtime Requirements take precedence over the
EBNF when a structural grammar limitation prevents the EBNF from expressing an
observable rule. Record any disagreement as a defect instead of choosing an
implementation-specific interpretation.

## Source-level contracts

The normative documents define the language, but source-level conformance
also depends on the public library and the fixture suite:

```text
slug/
├── docs/
│   └── language/                 # this package
├── lib/
│   └── slug/                     # the `slug.builtin` contract and `slug.channel`
├── clutch/                       # the standard library, as clutches
│   └── slug.core.clutch/         # `slug.std`, `slug.string`, `slug.test`, ...
└── tests/
    └── conformance/              # source fixtures and `.fixture.toml` sidecars
```

An implementation MUST treat `lib/slug`, the standard-library clutches, and
`tests/conformance` as source-level contracts. It may implement their native
parts in another host language, provided the Slug-visible modules, exports,
values, diagnostics, and stream behavior remain compatible.

Each entry fixture is a `.slug` source file with an adjacent, versioned
`<stem>.fixture.toml` sidecar. The schema-1 sidecar identifies the expected
outcome and can specify exact streams, module and library roots, a timeout, and
an exact diagnostic for failures. The complete metadata contract is in
[`../reference/conformance-fixtures.md`](../reference/conformance-fixtures.md).
Each clutch declares its own Slug test files, run with `slug test`.

## Implementation path

1. Implement lexical rules and grammar from `slug.ebnf` and the Language
   Specification.
2. Implement values, scope, functions, control flow, collections, patterns,
   and diagnostics before optimizing execution.
3. Implement module loading, including exports, live imports, and cyclic
   initialization.
4. Implement configuration from `configuration.md`, then run the versioned
   fixtures in `tests/conformance`.
5. Implement clutch loading and run the standard-library clutches' tests.
6. Implement structured concurrency, channels, tasks, `fork`, `select`,
   cleanup, and `recur` according to Runtime Requirements.

An implementation may use an interpreter, bytecode VM, JIT, or another
strategy. It MUST NOT require the reference implementation's source, private
bytecode, or Rust value representations to satisfy this package.
