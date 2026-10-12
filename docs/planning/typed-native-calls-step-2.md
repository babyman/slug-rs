# Typed native calls, step 2: `slug.db.sqlite`

## Goal

Implement step 2 of the replacement plan in
[Typed native calls](../decisions/2026-10-10-typed-native-calls.md): move the
`slug.db.sqlite` clutch to raw SQLite foreign functions plus Slug library code.
Keep every public name, signature, and documented behavior of
`slug.db.sqlite`, `slug.db.sqlite.statement`, and
`slug.db.sqlite.transaction`.

Step 2 ends when all three modules run on typed bindings, their tests pass on
macOS and Linux, and per-query cost is measured against the prototype.

## Out of scope

- `stdin`, the `events` profile, and removing the prototype (steps 3 and 4).
- Windows SQLite support. Its tests stay Unix-only, as today, because the
  Windows runner has no system SQLite.
- Structs, buffers, and manifest constants.

## What step 1's types cannot express

Reading the current adapter, `clutch/slug.db.sqlite.clutch/native/source/sqlite.c`,
against the step 1 bindings shows four gaps:

1. **Text and blob columns need a length.** `sqlite3_column_text` can contain
   NUL bytes, and `sqlite3_column_blob` is binary. A `cstr` result ends at the
   first NUL, and step 1 has no `str` or `bytes` result.
2. **Statements take a handle from another module.**
   `slug.db.sqlite.statement.prepare` takes a `slug.db.sqlite.Database`. Step 1
   resolves `handle<R>` only within one module. The adapter also installs each
   module in its own native scope, and the VM rejects a resource from another
   scope.
3. **Part of the SQLite API cannot be bound directly.** Binding text needs the
   `SQLITE_TRANSIENT` destructor constant, lengths are `int`, text columns are
   `const unsigned char *`, and opening returns its handle through an out
   pointer beside a NULL VFS name.
4. **Closing can fail and leave the handle open.** `sqlite3_close` returns
   `SQLITE_BUSY` while statements are unfinalized and keeps the connection.
   A typed `close` binding must leave its handle unusable whatever it returns.

## Decisions

Decisions 1-6 below were accepted as recommended on 2026-10-11, including the
diagnostic change in decision 4.

1. **Length-delimited results.** Recommendation: allow `out str` and
   `out bytes`. Each is two C parameters, `const char **` or
   `const uint8_t **` plus `size_t *`, and one result cell with a length. Each
   is borrowed or `owned(free_fn)`, like `cstr`. A returned `str` or `bytes`
   stays unsupported, because C returns one value.
2. **Cross-module handles in one clutch.** Recommendation: a signature may
   name `handle<module.R>` for a resource another module of the same clutch
   declares. The adapter installs all modules of one shim table in one native
   scope, as the prototype loader does, so the resource identity
   `slug.db.sqlite.Database` is unchanged.
3. **Thin C helpers in clutch sources.** Recommendation: where the SQLite API
   cannot be bound directly, the clutch adds a small plain-C helper beside the
   raw calls, one helper per SQLite call and with no Slug-aware code. Examples
   are `slug_sqlite_bind_text(statement, index, text, length)` and
   `slug_sqlite_column_text(statement, column, &text, &length)`. Everything
   else binds to SQLite directly. Manifest constants stay deferred.
4. **Errors and the uncaught diagnostic.** Recommendation: the wrappers throw
   `Error` values with the existing types, `sqlite.error` carrying
   `sqlite3_errmsg` and `sqlite.bind` for an unsupported parameter. As with
   `slug.io.fs`, an uncaught failure then reports the thrown struct at the
   wrapper instead of `native ...: message` at the call. This needs your
   acceptance.
5. **Close with open statements.** Recommendation: keep today's behavior.
   `close(database)` first asks a helper whether statements remain open
   (`sqlite3_next_stmt`), and throws `sqlite.error` without closing if so.
   Only then does it call the `close` binding, `sqlite3_close`. Release at
   teardown uses `sqlite3_close_v2`, which is safe in any order.
6. **Per-call cost first.** Building rows in Slug makes several native calls
   per column: type, name, and value. At the 231 ns per call measured in step 1,
   that cost will dominate queries. Recommendation: profile the adapter's
   per-call overhead first and bring you a ranking before changing it, as for
   the VM call path. Add a per-row C helper only if measurements still
   require one after that.

## `keys` becomes a builtin

`slug.core` also runs on the prototype, and the replacement plan does not move
it. `slug.math` and `slug.io.stdin` fit steps 1 and 3. However,
`slug.std.keys` takes a map, and neither Slug code nor a typed C binding can
enumerate map keys.

Decision (2026-10-11): `keys` becomes a defined builtin, because listing a
map's keys is essential beyond this case. Recommendation for the mechanism: a
VM builtin like `int`, implemented once and available on every host, including
embedded targets, with no per-host registration. The alternative is a host
function declared in `slug.builtin` beside `len`, which every host would have
to register. `slug.std.keys` keeps working by re-exporting the builtin. The
decision record for phase 6 settles the mechanism, the key order, and the
`slug.std` compatibility before the work starts.

## Phases

### 1. Adapter per-call cost

- [x] Profile the production typed scalar call and rank the per-call costs by
  saving and risk.
- [x] Apply the fixes you approve, one per commit, and re-measure.

Instruments' Time Profiler on 30 million typed `add` calls put about 86 ns of
the typed path's extra cost over a Rust native inside the native callback.
About 35 ns of that was two heap allocations per call, one for decoded cells
and one for result values. The ranked fixes were:

1. Decode results inside the call's decode step, straight into the Slug
   value, with no per-call collection. Approved and applied.
2. Precompute the binding index instead of parsing the member key, and avoid
   two `Rc` clones, an estimated 3-6 ns. Not applied.

The C call through the shim (about 8 ns) is the uniform entry's cost, and the
VM's per-call work is shared with every native call and belongs to
[VM optimization item 10](vm-optimization.md#10-reduce-per-instruction-overhead-and-indexed-member-lookup-deferred).

Fix 1 measured on the same Apple M3 host:

| Measurement | Before | After |
|---|---:|---:|
| Profiling harness, typed `add` (median of 5 alternating runs) | 394 ns | 365 ns |
| `make bench-ffi-prototype-scalar`, production typed path | 371 ns | 336 ns |
| Same run, 0.13 host-table callback | 339 ns | 336 ns |
| Same run, typed `slug.io.fs` per operation | 1,900 ns | 1,656 ns |
| Same run, prototype `slug.io.fs` per operation | 1,577 ns | 1,449 ns |

A scalar typed call now costs the same as the 0.13 callback. The filesystem
figures are dominated by a flushed write per operation and vary more between
runs; the typed clutch's gap narrowed from about 20% to about 14%.

### 2. Decisions

- [x] Record decisions 1-3 in a decision record that extends
  [Typed native manifest bindings](../decisions/2026-10-11-typed-native-manifest-bindings.md):
  [Typed native length results and shared handles](../decisions/2026-10-11-typed-native-length-results-and-shared-handles.md).
- [x] Record decisions 4 and 5 in this plan.

### 3. Length results and cross-module handles

- [x] Parse `out str`, `out bytes`, and `handle<module.R>` in the manifest
  model, generate them in shims, and encode and decode them in the adapter.
- [x] Install one shim table's modules in one native scope.
- [x] Extend the generator compile tests, the adapter tests, and the desktop
  compatibility check.

The shared scope comes from `NativeModule::new_shared_scope`, a hidden
`slug-vm` constructor that draws from the same counter as `NativeModule::new`.
A resource's type is registered on its declaring module, even when that module
binds no functions. A mutation test shows the cross-module call fails when each
module gets its own scope.

### 4. Move `slug.db.sqlite`

- [ ] Rewrite `sqlite.c` as the thin helpers with a header, and bind the rest
  of SQLite directly in a typed manifest that links `sqlite3`.
- [ ] Rewrite `sqlite.slug` and `statement.slug` as private raw declarations
  plus exported wrappers. They bind variadic parameters by type, build rows as
  maps, and reset and clear a statement after each use, including on failure.
  `transaction.slug` is unchanged.
- [ ] Stage the clutch through `slug-native-shim stage`.

### 5. Tests and measurement

- [ ] Point the five SQLite integration tests at the typed clutch, keeping a
  prototype copy for the 0.13 loader's list and map builder coverage until
  step 4. Add tests for thrown error types, NUL text and blob columns, and
  close with open statements.
- [ ] Measure exec and query end to end against the prototype and record the
  results.

### 6. `keys` builtin

- [ ] Record the `keys` builtin in a decision record: its mechanism, its key
  order (today's documentation says both "insertion order" and "unspecified
  order"), and how `slug.std.keys` keeps working.
- [ ] Implement it, document it in `lib/slug/builtin.slug` and the language
  specification, and remove `keys` from the core clutch's native library.

## Exit gate

- `slug.db.sqlite` and its statement and transaction modules run on typed
  calls, and their tests pass on macOS and Linux.
- The prototype loader keeps its list and map builder coverage.
- Measurements against the prototype are recorded.
- `keys` is a builtin, and `slug.std.keys` no longer needs the prototype.
