# Separate interactive client and server crates

## Context

`slug-repl` is a terminal transport client, while the VM and source compiler
own language execution. Keeping all three binaries in one crate made the
terminal dependency and the server protocol part of the VM crate's dependency
and build surface.

## Decision

Use a virtual Cargo workspace with three members: `slug-vm`, `slug-server`, and
`slug-repl`. The VM package lives at `crates/slug-vm`; `slug-server` owns the
interactive protocol and session server and depends on `slug-vm`. `slug-repl`
depends only on `slug-server` and its terminal client dependencies; it must not
import VM or compiler implementation APIs.

The VM provides the server's internal interactive execution bridge as hidden
Rust API. It is not a stable embedding or protocol contract.

## Consequences

The REPL can have terminal-specific dependencies without making them direct VM
dependencies. Interactive protocol and server tests live with `slug-server`;
terminal process tests live with `slug-repl`. Workspace-wide validation must
select all members so that the paired server executable is built for REPL tests.

## Migration

Existing commands targeting `slug-repl` use `cargo run -p slug-repl`; commands
targeting the language CLI use `cargo run -p slug-vm --bin slug`.
