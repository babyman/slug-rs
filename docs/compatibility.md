# Compatibility Policy

Slug is pre-release. The repository may improve internal architecture and the
implemented source subset without preserving undocumented behavior.

## Promises

- Normative language documents define intended source-level behavior.
- Source and runtime failures are Slug diagnostics, not host panics.
- The public CLI's documented source subset is tested as a user-visible path.
- Once published, each `.cslug` artifact version is a portable compiled-module
  compatibility contract as defined in `compiled-artifacts.md`.

## Non-promises

- Bytecode instructions, opcode values, chunks, stack layout, and closure
  representation are internal implementation details.
- No `.cslug` artifact version is implemented or accepted yet.
- A specification section is not proof that the current Rust VM implements it;
  the language support matrix reports that status.
- Exact diagnostic wording is not stable unless a language document explicitly
  requires it.

Any new compatibility promise requires a decision record and matching
regression coverage.
