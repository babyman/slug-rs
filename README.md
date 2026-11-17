# Slug

Slug is a small, expressive language for writing programs with values,
functions, pattern matching, modules, checked errors, and structured
concurrency. This repository, **slug-rs**, is the reference implementation of
that language, in Rust; it replaces the earlier Go implementation, which is now
legacy. It is deliberately building the language foundation before adding
compatibility layers or speculative abstractions.

The project is useful in two related ways:

- **As a language implementation:** it runs a growing, well-specified subset
  of Slug source code from the command line and an interactive REPL.
- **As an embeddable runtime:** it provides a checked in-process bytecode VM,
  a source frontend, and host boundaries for desktop, restricted, and static
  embedded environments.

Slug source programs are the contract. The Rust VM and bytecode are internal
implementation surfaces: they are available for embedding and testing, but
remain unstable during pre-release development.

## A first Slug program

```slug
val greeting = fn(name) { "Hello, " + name + "!" }

val main = fn() {
    println(greeting("Slug"))
}
```

Save it as `hello.slug`, then run it from this repository:

```sh
cargo run -p slug -- hello.slug
```

The CLI evaluates the program and invokes a local `main()` entrypoint. Pass
program arguments after the source path; run `cargo run -p slug -- --help` for
the full interface.

## What Slug supports today

The implemented subset already covers the core of day-to-day source programs:

- immutable `val` and mutable `var` bindings; functions, closures, captures,
  returns, and positional/named/defaulted/variadic calls;
- numbers, strings, bytes, booleans, `nil`, lists, maps, structs, fieldless
  enums, type aliases, indexing, slicing, and persistent collection updates;
- `if`, `match`, destructuring, pinned patterns, checked match coverage, and
  source annotations with progressively stronger static checking;
- imports, isolated cached modules, projects marked by `slug.toml`,
  configuration through `cfg`, compile-time `asset` embedding, module
  introspection with `describe`, and the built-in `print`, `println`, `len`,
  `keys`, `chan`, and `close` functions;
- channels, tasks, nurseries, `select`, timers, `fork` tasks that see a
  snapshot and may run on worker threads, per-task `task var`s, `defer`,
  `throw`, error recovery, and contextual stack traces;
- a standard library shipped as clutches: collections, strings, bytes, math,
  JSON and TOML, time, randomness, standard input, signals, testing, and,
  from separate clutches, files, TCP, HTTP, SQLite, and cryptography; and
- compilation of closed whole-program `.cslug` artifacts for portable
  execution on compatible Slug VMs.

This is not yet complete Slug conformance. The [language support
matrix](docs/generated/language-support.md) is the authoritative list of what
slug-rs implements; the [language documents](docs/language/README.md) define
the target language semantics.

## Using slug-rs

### Compile and run a closed artifact

`.cslug` stores a complete, validated compiled program. It runs without
consulting source resolvers:

```sh
cargo run -p slug -- compile hello.slug hello.cslug
cargo run -p slug -- run hello.cslug
```

See [compiled artifacts](docs/reference/compiled-artifacts.md) for its format
and compatibility contract.

### Report what a program can touch

`slug report` lists a program's modules, imports, host-supplied builtins, and
`foreign` and `resource` declarations as JSON, without running it:

```sh
cargo run -p slug --bin slug -- report hello.slug
```

See [capability report](docs/reference/capability-report.md) for the format.

### Work interactively

Build the workspace binaries once, then start the terminal REPL:

```sh
cargo build --workspace --bins
cargo run -p slug-repl
```

Pass a source path to seed a session: `cargo run -p slug-repl -- session.slug`.
The REPL runs alongside `slug --server`, supports multiline input and in-session
history, and retains successful bindings between submissions. Use `:quit`,
`:exit`, or end-of-file to close it. Set `SLUG_SERVER` to select a different
local server executable.

### Use the library clutches

Slug's library ships as *clutches*: directories that provide modules, with
optional native code. `clutch/` holds them, indexed by `clutch/manifest.toml`:
`slug.core.clutch` provides `slug.std`, `slug.string`, `slug.fmt.json`,
`slug.test`, `slug.time`, `slug.io.stdin`, and more; others provide
`slug.io.fs`, `slug.net.tcp`, `slug.net.http`, `slug.db.sqlite`,
`slug.crypto.hash`, and `slug.crypto.schnorr`. The runner finds installed
clutches through `$SLUG_HOME/clutch/manifest.toml`, and a project may index its
own in `clutch/manifest.toml` at its root. Native calls are generated from
typed manifests. This checkout is also the clutches' development repository,
so stage their native libraries before a program imports one:

```sh
make stage-native-clutches
```

Staged platform libraries are local build outputs, not release artifacts. The
clutch format is still version 0 and may change. See
[clutches](docs/reference/clutches.md) for the provider model and [native ABI](docs/reference/native-abi.md) for the host
boundary.

## Inside the project

slug-rs is a Rust workspace with deliberately narrow boundaries:

| Area | Responsibility |
| --- | --- |
| `crates/slug-vm` | Dynamic values, bytecode, runtime, and checked VM failures. |
| `crates/slug-frontend` | Parsing, semantics, lowering, and module graph construction. |
| `crates/slug` | Command-line interface, configuration, fixtures, and server mode. |
| `crates/slug-repl` | Terminal client for `slug --server`. |
| `crates/slug-*-loader` | Desktop, nil, and other host-specific import resolution. |
| `crates/slug-native-*` | Typed native manifests and the adapter that is exclusively responsible for shim calls. |
| `crates/slug-static-host`, `crates/slug-embedded-build`, `firmware/` | Static module contracts, firmware build descriptors, and the ESP32-S3 firmware. |
| `clutch/` | The standard library and other modules, as clutches. |
| `lib/` | The `slug.builtin` contract, `slug.channel`, and installed tools such as the `slug test` runner. |
| `tests/conformance` | Portable source fixtures with versioned sidecars. |

The VM executes `Program`s made of chunks, constants, and typed instruction
variants—not serialized opcode bytes. Malformed bytecode, invalid source, and
runtime faults must produce checked Slug errors with source spans and call
frames, never host panics. The [architecture guide](docs/engineering/architecture.md)
explains these ownership and compatibility boundaries in detail.

## Development

Rust 1.96.1 (including `rustfmt` and Clippy) is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml); Rustup selects it automatically.

```sh
make check
```

`make check` validates formatting, strict Clippy, the full test suite, the
slim and cooperative feature configurations, the documentation, and the
clutches' Slug tests. While iterating, use focused commands
such as `make test-vm`, `make test-cli`, or `make slim`. See the
[testing guide](docs/engineering/testing.md) for test routing and
[AGENTS.md](AGENTS.md) for repository working rules.

## Learn more

- [Documentation index](docs/README.md) — authority, reference contracts, engineering, and plans.
- [Language documentation](docs/language/README.md) — normative grammar and semantics.
- [Language support matrix](docs/generated/language-support.md) — implemented subset versus target surface.
- [Compatibility policy](docs/reference/compatibility.md) — what slug-rs promises today.
