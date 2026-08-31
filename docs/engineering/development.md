# Development

Use the narrowest validation that proves a change while iterating, then run the
appropriate handoff check.

| Change surface | Iteration check | Handoff check |
|---|---|---|
| VM behavior | `make test-vm` | `make check` |
| CLI/source behavior | `make test-cli` | `make check` |
| Rust implementation | Focused test | `make check` |
| Documentation only | `make docs-check` | `git diff --check` |

`make check` runs Rust formatting validation, Clippy with warnings denied, all
tests, and documentation checks. CI runs the same target.

## Toolchain policy

[`../../rust-toolchain.toml`](../../rust-toolchain.toml) pins the Rust compiler
and required `rustfmt` and Clippy components. `Cargo.toml`'s `rust-version`
matches that pin, and CI installs the same version before it runs `make ci`.

Update the toolchain only in a deliberate maintenance change. Update the pin,
`rust-version`, and CI configuration together; then run `make check` with the
candidate toolchain and review any resulting formatter, lint, dependency, or
lockfile changes as part of that change. Do not make incidental toolchain or
lockfile updates while implementing unrelated work.

For a source-language change, follow
`.agents/workflows/language-change.md` before editing. It identifies the
language records and implementation tests that must remain synchronized.

## Documenting decisions

A Slug-visible rule, diagnostic class, runtime guarantee, or compatibility
promise must have an owner in the normative language or runtime documents
before code or tests rely on it. When an applicable document does not decide
the behavior, record the selected rule there first; do not allow implementation
behavior to become the de facto specification.

Create a decision record when the choice meets the criteria in
[`docs/decisions/README.md`](../decisions/README.md). A decision record explains
why a non-trivial rule was selected, but does not replace the normative
requirement. No new record is needed when the documentation already expressly
determines the behavior; for example, exact map patterns already disallow
`...rest`.
