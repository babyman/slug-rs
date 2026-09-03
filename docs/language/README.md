# Slug Language Documentation

## Implementation status

These documents define Slug's target conformance surface. The Rust VM in this
repository implements only the subset listed in
[`../generated/language-support.md`](../generated/language-support.md). A
specified feature is not implemented merely because it appears in this
directory.

## Purpose

This directory is the normative language package for an independent Slug
implementation. It contains the grammar and behavioral requirements a remote
developer needs without consulting the Go implementation, bytecode, or host
object model.

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
| `maps.md` | Map literals, keys, and operations. |
| `match-and-destructuring.md` | Match expressions and destructuring patterns. |
| `structs.md` | Schema identity, construction, defaults, field access, and equality. |
| `strings.md` | String literals and string operations. |
| `standard-input.md` | Process-standard-input stream and interactive helpers. |
| `value-pinning-in-match-patterns.md` | Pinning existing values in match patterns. |
| `variadic-functions-and-spread-syntax.md` | Variadic parameters and spread arguments. |

The package also includes focused supplemental notes for automatic semicolon
insertion, errors, deferred work, maps, matching and destructuring, structs,
strings, value pinning, and variadic functions with spread syntax. They expand individual topics. If a
supplemental note conflicts with the Language Specification or Runtime
Requirements, the latter documents take precedence.

The Language Specification and Runtime Requirements take precedence over the
EBNF when a structural grammar limitation prevents the EBNF from expressing an
observable rule. Record any disagreement as a defect instead of choosing an
implementation-specific interpretation.

## Required handoff contents

The normative documents in this directory define the specification, but
source-level conformance also requires the public library and fixture suite. A
complete handoff MUST preserve this repository layout:

```text
slug/
├── docs/
│   └── language/                 # this package
├── lib/
│   └── slug/                     # public standard-library source
└── tests/
    └── conformance/              # source fixtures and `.fixture.toml` sidecars
```

The implementation MUST treat `lib/slug` and `tests/conformance` as
source-level contracts, not as a dependency on Go. It may implement their
behavior in another host language, provided the Slug-visible modules, exports,
values, diagnostics, and stream behavior remain compatible.

Each entry fixture is a `.slug` source file with an adjacent, versioned
`<stem>.fixture.toml` sidecar. The schema-1 sidecar identifies the expected
outcome and can specify exact streams, module and library roots, a timeout, and
an exact diagnostic for failures. The complete metadata contract is in
[`../reference/conformance-fixtures.md`](../reference/conformance-fixtures.md).

## Clean-room implementation path

1. Implement lexical rules and grammar from `slug.ebnf` and the Language
   Specification.
2. Implement values, scope, functions, control flow, collections, patterns,
   and diagnostics before optimizing execution.
3. Implement module loading and the public `lib/slug` surface, including
   exports, live imports, and cyclic initialization.
4. Implement configuration from `configuration.md`, then run the versioned
   fixtures in `tests/conformance`.
5. Implement structured concurrency, channels, tasks, `select`, cleanup, and
   `recur` according to Runtime Requirements.

An implementation may use an interpreter, bytecode VM, JIT, or another
strategy. It MUST NOT require the reference Go source, its internal bytecode,
goroutines, or Go object representations to satisfy this package.

## Handoff acceptance checklist

Before sending the package, verify that the recipient receives:

- this `docs/language` directory;
- `lib/slug` and `tests/conformance` at the relative paths above;
- the intended Slug version or commit identifier;
- the fixture sidecars that describe each entry source; and
- no reference implementation source as a required runtime dependency.
