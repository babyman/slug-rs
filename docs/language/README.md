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
| `Automatic Semicolon Insertion (ASI) Rules.md` | Automatic statement termination rules. |
| `Errors - Mini Spec.md` | Error categories and diagnostic behavior. |
| `Deferred Work.md` | Deferred-action execution and recovery behavior. |
| `Map Syntax and Behavior - Mini Spec.md` | Map literals, keys, and operations. |
| `Match and Destructuring - Mini Spec.md` | Match expressions and destructuring patterns. |
| `Struct Syntax and Behavior - Mini Spec.md` | Schema identity, construction, defaults, field access, and equality. |
| `Strings - Mini Spec.md` | String literals and string operations. |
| `Value Pinning in match Patterns.md` | Pinning existing values in match patterns. |
| `Variadic Functions and Spread Syntax - Mini Spec.md` | Variadic parameters and spread arguments. |

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

The four files in this directory define the specification, but source-level
conformance also requires the public library and fixture suite. A complete
handoff MUST preserve this repository layout:

```text
slug/
├── docs/
│   └── language/                 # this package
├── lib/
│   └── slug/                     # public standard-library source
└── tests/
    └── vm-conformance/           # supported and error-parity fixtures
```

The implementation MUST treat `lib/slug` and `tests/vm-conformance` as
source-level contracts, not as a dependency on Go. It may implement their
behavior in another host language, provided the Slug-visible modules, exports,
values, diagnostics, and stream behavior remain compatible.

The present fixture directories predate portable sidecar metadata. Their
in-source `slug.test` assertions and supported/error-parity classification are
the current acceptance evidence. Before claiming a fully portable release,
provide the fixture metadata schema required by `runtime-requirements.md` for
every entry fixture.

## Clean-room implementation path

1. Implement lexical rules and grammar from `slug.ebnf` and the Language
   Specification.
2. Implement values, scope, functions, control flow, collections, patterns,
   and diagnostics before optimizing execution.
3. Implement module loading and the public `lib/slug` surface, including
   exports, live imports, and cyclic initialization.
4. Implement configuration from `configuration.md`, then run the supported and
   error-parity fixtures in `tests/vm-conformance`.
5. Implement structured concurrency, channels, tasks, `select`, cleanup, and
   `recur` according to Runtime Requirements.

An implementation may use an interpreter, bytecode VM, JIT, or another
strategy. It MUST NOT require the reference Go source, its internal bytecode,
goroutines, or Go object representations to satisfy this package.

## Handoff acceptance checklist

Before sending the package, verify that the recipient receives:

- this `docs/language` directory;
- `lib/slug` and `tests/vm-conformance` at the relative paths above;
- the intended Slug version or commit identifier;
- a fixture manifest or an explicit statement that the current source-only
  fixture expectations are being used; and
- no reference implementation source as a required runtime dependency.
