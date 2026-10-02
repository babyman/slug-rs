# Slug executable crate boundary

This crate will compose the VM, frontend, and selected loader. It will own
desktop CLI policy, configuration assembly, entry lookup, builtin
registration, and server mode; loaders must not absorb those responsibilities.

Use `make test-slug` for focused executable-host work. Preserve `slug --server`
as the sole interactive-server entry point; `slug-repl` remains a protocol-only
terminal client.
