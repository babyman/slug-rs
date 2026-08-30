# AI-Assisted Development Plan

This plan keeps Slug easy to change safely when contributors use AI coding
agents. It is a repository-maintenance plan, not a language specification or
an implementation-architecture decision. Existing language, compatibility,
and runtime documents remain authoritative.

## Goal

An agent beginning a bounded change should be able to determine the owning
documents, implementation area, focused tests, and required validation without
loading unrelated large files or inferring a contract from stale guidance.

## Current foundation

The repository already has useful guardrails:

- Root `AGENTS.md` defines repository-wide invariants and the standard
  validation ladder.
- `.agents/workflows/language-change.md` gives source-language work a concrete
  documentation, implementation, testing, and handoff sequence.
- `docs/README.md` establishes an authority order rather than allowing tests or
  implementation details to silently redefine language behavior.
- `make check` is both the local handoff command and the CI gate.

The work below addresses the remaining places where repository guidance can
misdirect an agent, where a large file makes the working set unnecessarily
wide, or where reproducibility relies on moving tooling.

## Scope and invariants

- Preserve the current documentation authority order and keep language rules in
  `docs/language/`.
- Treat file-size targets as navigation guidance, not a reason for mechanical
  refactors. Split only at an actual ownership boundary.
- Preserve the public behavior and checked-error guarantees while reorganizing
  tests or Rust modules.
- Keep private bytecode separate from the portable `.cslug` contract. Any
  change to the Rust crate's public API or compatibility policy requires a
  decision record.
- Keep CI and the documented local handoff command equivalent.

## 1. Make repository guidance self-consistent

- [x] Correct `docs/language/README.md` to name the existing
  `tests/conformance/` fixture suite rather than `tests/vm-conformance/`.
- [x] Update that document's fixture description to match the current
  versioned `.fixture.toml` metadata contract.
- [x] Expand `docs/engineering/testing.md` and the test routing in `AGENTS.md`
  to cover VM, CLI, module-loader, configuration, conformance-runner, and
  conformance-metadata tests.
- [x] Add focused Make targets only where they materially shorten a common
  iteration loop; otherwise document the direct `cargo test --test …` command.
- [x] Extend `scripts/docs-check.sh` with inexpensive checks for documented
  repository paths that are intended as mandatory handoff contents.

Completion evidence: links and paths named by contributor documentation exist,
the test-routing table covers every integration-test boundary, and
`make docs-check` rejects the documented-path regressions it can verify.

## 2. Keep the working set small and discoverable

Prefer a small entry file and feature-focused modules over a monolithic file.
The current review identifies these priority candidates:

1. Split `tests/cli.rs` by observable language feature, retaining a thin test
   harness if Rust integration-test discovery requires it.
2. Split `tests/vm.rs` by VM/runtime subsystem or bytecode family.
3. Extract coherent ownership areas from `src/vm/mod.rs`, such as dispatch,
   call/frame handling, and asynchronous/select behavior, only after confirming
   their dependency direction.
4. Extract type representation, inference, overload resolution, or diagnostics
   from `src/source/typecheck.rs` when their APIs can be explicit and
   independently tested.

For new work, aim to keep production modules below roughly 1,500 lines and
test modules below roughly 1,200 lines. Crossing a target triggers a review of
module ownership; it does not mandate a split.

- [ ] Add a brief module map at the entry point whenever a large file is split.
- [ ] Keep tests colocated by observable behavior, with names that make focused
  search straightforward.
- [ ] Avoid extracting a shared abstraction solely to reduce line count.
- [ ] Run the focused affected suite after each slice and `make check` after a
  completed refactor.

Completion evidence: the priority files have feature-oriented navigation
boundaries, and an agent can identify the owning file and focused test without
opening an unrelated multi-thousand-line file.

## 3. Clarify the crate and bytecode boundary

The documentation calls `Program`, `Chunk`, and `Op` private implementation
details, while `src/lib.rs` publicly re-exports them. Before changing code,
make the compatibility choice explicit:

- [ ] Decide whether the public Rust exports are an intentionally unstable
  embedding/testing surface or an accidental exposure.
- [ ] Record the choice in a decision record because it changes runtime
  architecture and compatibility policy.
- [ ] If retained, document the stability and supported-use limits next to the
  public API and in the compatibility reference.
- [ ] If removed, first migrate integration tests to an appropriate internal
  test boundary without weakening bytecode verification coverage.

Completion evidence: the README, `AGENTS.md`, architecture documentation, and
Rust exports describe one consistent boundary.

## 4. Make toolchain results reproducible

- [ ] Select and pin a Rust toolchain in `rust-toolchain.toml`.
- [ ] Set the corresponding minimum supported Rust version in `Cargo.toml`.
- [ ] Update CI to use that pinned toolchain while retaining `rustfmt` and
  Clippy components.
- [ ] Document the toolchain update policy so dependency and lint changes are
  deliberate maintenance work rather than incidental agent output.

Completion evidence: a fresh local checkout and CI use the same Rust version,
and `make check` remains the single verified handoff gate.

## Sequencing

Complete section 1 first: agents should not be asked to follow paths or test
routing that no longer match the repository. Section 4 can proceed
independently. Split tests before production modules in section 2, because test
splits reduce context size with the least architectural risk. Complete section
3 before any change that removes or formalizes a public bytecode export.

## Verification and handoff

Every documentation-only slice runs `make docs-check` and `git diff --check`.
Every Rust, build, or CI slice runs the focused test while iterating and
`make check` before handoff. Record the commands actually run, any deliberately
unrun validation, and any remaining compatibility decision separately from the
implementation work.
