# Testing

Tests are evidence for specified behavior. They do not create a language
guarantee when the language documents say otherwise.

| Test | Use for |
|---|---|
| `tests/vm.rs` | Bytecode execution, values, globals, closures, frames, and source spans. |
| `tests/cli.rs` | Accepted source, printed output, exit status, and rendered diagnostics. |

Add a regression test with every behavior change. Error behavior must assert a
Slug error or CLI diagnostic rather than merely proving that execution did not
panic. Source syntax and user-visible behavior require a CLI test even when a
VM test covers the same execution path.
