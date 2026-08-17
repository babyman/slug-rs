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

For a source-language change, follow
`.agents/workflows/language-change.md` before editing. It identifies the
language records and implementation tests that must remain synchronized.
