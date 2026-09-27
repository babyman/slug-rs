# Interactive Server

This directory owns the versioned NDJSON protocol, persistent sessions, and
the projection of source and runtime outcomes into protocol events. The server
owns session lifecycle, retained submissions, and event ordering; the VM owns
execution state.

Keep raw bytecode off the protocol. Preserve request/response compatibility,
session isolation, source-error versus runtime-error diagnostic categories,
and the invariant that pending binding-producing forms are not committed until
they settle successfully. Route session output through queued events rather
than process stdout.

Cover protocol, lifecycle, output, and retained-work changes in
`crates/slug-server/tests/interactive_server.rs` and run `make test-server`.
When a change crosses the terminal boundary, also run `make test-repl`.
