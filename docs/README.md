# Slug Documentation

This directory is the single home for Slug's language rules, implementation
architecture, development process, and durable design decisions.

## Authority

Use the following order when sources disagree:

1. `language/language-specification.md` defines source-language semantics.
2. `language/slug.ebnf` defines accepted source syntax.
3. Focused documents in `language/` define feature-specific rules.
4. `language/runtime-requirements.md` defines observable runtime obligations.
5. `engineering/architecture.md` defines internal implementation boundaries.
6. Tests prove the implemented subset; they do not silently redefine language
   behavior.

The language documents describe the target Slug conformance surface. The Rust
implementation intentionally supports only a subset. Consult the generated
[language support matrix](generated/language-support.md) before claiming that
a specified feature is implemented.

## Documentation areas

| Area | Purpose |
|---|---|
| [language/](language/README.md) | Normative language specifications, grammar, and runtime requirements. |
| [reference/](reference/) | Public compatibility and host-interface contracts. |
| [engineering/](engineering/) | Rust implementation architecture and contributor workflow. |
| [planning/](planning/) | Living implementation roadmaps and completed plans retained for context. |
| [decisions/](decisions/README.md) | Immutable records of durable architecture and language decisions. |
| [generated/](generated/) | Derived implementation-status artifacts. |

### Reference

| Document | Purpose |
|---|---|
| [compatibility.md](reference/compatibility.md) | Promises and intentional non-promises. |
| [compiled-artifacts.md](reference/compiled-artifacts.md) | Portable `.cslug` compiled-module contract. |
| [conformance-fixtures.md](reference/conformance-fixtures.md) | Portable fixture-sidecar contract. |
| [native-abi.md](reference/native-abi.md) | Native calls, values, resources, threading, and future binary ABI contract. |

### Engineering

| Document | Purpose |
|---|---|
| [architecture.md](engineering/architecture.md) | Compiler, bytecode, VM, and diagnostic ownership. |
| [development.md](engineering/development.md) | Local workflow, validation ladder, and change process. |
| [testing.md](engineering/testing.md) | Test-layer selection and regression policy. |
| [vm-optimization.md](planning/vm-optimization.md) | Staged private VM and bytecode optimization plan. |

### Planning

| Document | Purpose |
|---|---|
| [language-foundation-roadmap.md](planning/language-foundation-roadmap.md) | Dependency-ordered implementation tasks for source compatibility. |
| [expression-foundation-inventory.md](planning/expression-foundation-inventory.md) | Current expression-support boundary and dependency-ordered implementation slices. |
| [type-system-plan.md](planning/type-system-plan.md) | Dependency-ordered plan for the next static-checking milestones. |
| [numeric-representation-decision.md](planning/numeric-representation-decision.md) | Outstanding numeric semantics, representation, and VM-performance decision plan. |
| [ai-assisted-development.md](planning/ai-assisted-development.md) | Plan for trustworthy agent guidance, bounded working sets, and reproducible validation. |
| [completed/](planning/completed/) | Retained plans for completed implementation work. |

Hand-authored documents describe decisions and requirements. Files in
`generated/` are derived artifacts: edit their source manifest and regenerate
them instead of editing the rendered file directly.
