# Source Front End

This directory owns the private, one-way source pipeline:

```text
source text -> lexer/parser/AST -> semantic analysis -> compiler lowering -> Program
```

`mod.rs` owns the public compile facade and `SourceError`. `interactive.rs`
owns source-readiness classification and interactive compiler state.
`syntax/{lexer,parser,ast}.rs` own syntax only.
`semantics/{typecheck,environment,semantic}.rs` own bindings, type
information, imports, and semantic snapshots.
`lowering/{compiler,state}.rs` lower analyzed syntax to bytecode.

Do not make syntax depend on semantic state, semantic analysis depend on
bytecode encoding or VM execution, or lowering re-decide semantic rules. Keep
invalid source as a checked `SourceError` with a span when available.
Interactive bindings may commit only after their cell settles successfully.

For source syntax, diagnostics, or observable behavior, add or update a CLI
test and run `make test-frontend`. For module/import semantic snapshots, run
`make test-modules`. For interactive compilation or commit behavior, run
`make test-server`. Read `docs/language/` and
`.agents/workflows/language-change.md` before a source-language change.
