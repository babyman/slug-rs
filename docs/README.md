# Slug Documentation

This directory is the single home for Slug's language rules, implementation
architecture, development process, and durable design decisions.

## Authority

Use the following order when sources disagree:

1. `language/language-specification.md` defines source-language semantics.
2. `language/slug.ebnf` defines accepted source syntax.
3. Focused documents in `language/` define feature-specific rules.
4. `language/runtime-requirements.md` defines observable runtime obligations.
5. `architecture.md` defines internal implementation boundaries.
6. Tests prove the implemented subset; they do not silently redefine language
   behavior.

The language documents describe the target Slug conformance surface. The Rust
implementation intentionally supports only a subset. Consult the generated
[language support matrix](generated/language-support.md) before claiming that
a specified feature is implemented.

## Documents

| Document | Purpose |
|---|---|
| [architecture.md](architecture.md) | Compiler, bytecode, VM, and diagnostic ownership. |
| [language-foundation-roadmap.md](language-foundation-roadmap.md) | Dependency-ordered implementation tasks for source compatibility. |
| [expression-foundation-inventory.md](expression-foundation-inventory.md) | Current expression-support boundary and dependency-ordered implementation slices. |
| [vm-optimization.md](vm-optimization.md) | Staged private VM and bytecode optimization plan. |
| [development.md](development.md) | Local workflow, validation ladder, and change process. |
| [testing.md](testing.md) | Test-layer selection and regression policy. |
| [compatibility.md](compatibility.md) | Promises and intentional non-promises. |
| [compiled-artifacts.md](compiled-artifacts.md) | Portable `.cslug` compiled-module contract. |
| [language/](language/README.md) | Normative language specifications and grammar. |
| [decisions/](decisions/README.md) | Durable architecture and language decisions. |

Hand-authored documents describe decisions and requirements. Files in
`generated/` are derived artifacts: edit their source manifest and regenerate
them instead of editing the rendered file directly.
