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
| [compiled-artifacts.md](reference/compiled-artifacts.md) | VM-first `.cslug` whole-program artifact contract. |
| [compiled-modules.md](reference/compiled-modules.md) | Version-2 contract for importable compiled modules and closed linking. |
| [experimental-clutches.md](reference/experimental-clutches.md) | Version-0 local clutch composition experiment. |
| [conformance-fixtures.md](reference/conformance-fixtures.md) | Portable fixture-sidecar contract. |
| [native-abi.md](reference/native-abi.md) | Native calls, values, resources, threading, and future binary ABI contract. |

### Engineering

| Document | Purpose |
|---|---|
| [architecture.md](engineering/architecture.md) | Compiler, bytecode, VM, and diagnostic ownership. |
| [development.md](engineering/development.md) | Local workflow, validation ladder, and change process. |
| [testing.md](engineering/testing.md) | Test-layer selection and regression policy. |
| [benchmarking.md](engineering/benchmarking.md) | Internal and end-to-end benchmark layers. |
| [benchmark-baseline-2026-09-16.md](engineering/benchmark-baseline-2026-09-16.md) | Initial local source-benchmark baseline. |
| [vm-optimization.md](planning/vm-optimization.md) | Staged private VM and bytecode optimization plan. |

### Planning

| Document | Purpose |
|---|---|
| [language-foundation-roadmap.md](planning/language-foundation-roadmap.md) | Dependency-ordered implementation tasks for source compatibility. |
| [type-system-plan.md](planning/type-system-plan.md) | Dependency-ordered plan for the next static-checking milestones. |
| [inferred-overload-alternatives-follow-up.md](planning/inferred-overload-alternatives-follow-up.md) | Deferred work after finite inferred `+` alternatives. |
| [host-owned-module-resolution.md](planning/host-owned-module-resolution.md) | Staged extraction of filesystem-free module resolution with desktop and embedded hosts. |
| [immutable-collection-value-redesign.md](planning/immutable-collection-value-redesign.md) | Staged collection and `Value` representation work that preserves immutable semantics. |
| [interactive-slim-retained-cells.md](planning/interactive-slim-retained-cells.md) | Retained interactive cells with one execution model across runtime configurations. |
| [numeric-representation-decision.md](planning/numeric-representation-decision.md) | Outstanding numeric semantics, representation, and VM-performance decision plan. |
| [agent-facing-direction.md](planning/agent-facing-direction.md) | Product direction for agent-written, human-reviewed code, with ordered work and measures. |
| [completed/](planning/completed/) | Retained plans for completed or superseded implementation work, including: |
| [cslug-whole-program-artifacts.md](planning/completed/cslug-whole-program-artifacts.md) | Completed `.cslug` codec, verifier, host persistence, and firmware artifact path. |
| [esp32-s3-static-clutch-gpio.md](planning/completed/esp32-s3-static-clutch-gpio.md) | Completed ESP32-S3 static-provider GPIO plan and on-device verification record. |
| [general-frozen-firmware-programs.md](planning/completed/general-frozen-firmware-programs.md) | Superseded private frozen-image firmware plan. |
| [esp32-s3-proof-of-concept.md](planning/completed/esp32-s3-proof-of-concept.md) | Bounded bare-metal ESP32-S3 plan for flash-resident Slug source and UART output. |
| [embedded-static-build-toolchain.md](planning/completed/embedded-static-build-toolchain.md) | Completed implementation plan for descriptor-driven `.slug` to firmware builds with static providers. |
| [body-derived-inference-follow-up.md](planning/completed/body-derived-inference-follow-up.md) | Completed known-call and nil-partitioned body-derived inference. |
| [expression-foundation-inventory.md](planning/completed/expression-foundation-inventory.md) | Historical expression-support boundary and implementation slices. |
| [type-checking-phase-3-complete-inference-across-ordinary-expressions.md](planning/completed/type-checking-phase-3-complete-inference-across-ordinary-expressions.md) | Acceptance criteria and execution packages for complete ordinary-expression inference. |
| [nominal-types-implementation.md](planning/completed/nominal-types-implementation.md) | Checklist for nominal resources, enums, and aliases. |
| [agentic-development-refactoring.md](planning/completed/agentic-development-refactoring.md) | Organizational refactoring plan for discoverable subsystem boundaries and focused validation. |
| [micro-runtime-followups.md](planning/completed/micro-runtime-followups.md) | Completed correctness, lifecycle, and Cargo feature-matrix follow-ups for the slim VM runtime. |
| [FFI channel producer prototype ABI 0.13 plan](planning/completed/ffi-channel-producer-v0.13.md) | Completed prototype ABI 0.13 native-producer work and `slug.io.stdin` Clutch extraction; the prototype has since been removed. |
| [ai-assisted-development.md](planning/completed/ai-assisted-development.md) | Plan for trustworthy agent guidance, bounded working sets, and reproducible validation. |

Hand-authored documents describe decisions and requirements. Files in
`generated/` are derived artifacts: edit their source manifest and regenerate
them instead of editing the rendered file directly.
