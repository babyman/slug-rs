# Compatibility Policy

Slug is pre-release. The repository may improve internal architecture and the
implemented source subset without preserving undocumented behavior.

## Promises

- Normative language documents define intended source-level behavior.
- Source and runtime failures are Slug diagnostics, not host panics.
- The public CLI's documented source subset is tested as a user-visible path.
- The public Rust bytecode types support in-process construction and checked
  execution during pre-release development. They are an unstable embedding and
  testing surface, not a portable representation.
- Once published, each `.cslug` artifact version is a portable compiled-module
  compatibility contract as defined in `compiled-artifacts.md`.
- Once published, each native module ABI major version is a compatibility
  contract as defined in `native-abi.md` and its released C declarations.

## Non-promises

- Bytecode instruction layouts, opcode variants and values, chunks, stack
  layout, and closure representation may change without Rust API compatibility.
- Public bytecode types must not be serialized, exchanged between versions, or
  treated as the `.cslug` compiled-module contract.
- No `.cslug` artifact version is implemented or accepted yet.
- The static Rust native facade is version 0 and no native module ABI version is
  implemented or accepted yet.
- A specification section is not proof that the current Rust VM implements it;
  the language support matrix reports that status.
- Exact diagnostic wording is not stable unless a language document explicitly
  requires it.

Any new compatibility promise requires a decision record and matching
regression coverage.
