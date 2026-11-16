# Persistent maps

## Status

Proposed (2026-11-13). Stage 7 of
[the collection redesign](../planning/immutable-collection-value-redesign.md).

## Context

Slug maps are immutable values. A map has a contiguous vector of entries and,
above 32 entries, an open-addressing index of their positions
(`collections.rs`, `INDEX_THRESHOLD`). An update by the map's only owner
changes it in place; an update of a map something else still holds copies all
of it. So keeping an old version of a map costs a full copy at the next update:
the cost of an update depends on what else holds the map, not on how much
changes.

That makes performance fall off a cliff that the language's value semantics
invite you to approach. Keeping versions is what immutable values are for:
undo and history, snapshots (Crawl's `snapshot()` and `load`), the values a
fork is handed, a previous state kept for comparison. Each turns every later
update of a large map into O(n). An accidental case is worse:
`map(xs, fn(x) { seen = seen + {[x]: true} })` keeps every version in its
result (an assignment evaluates to the value assigned), so 40,000 keys took
73 s and memory grew with the square of the length. `each` now serves that
loop, but the cliff remains wherever versions are kept on purpose.

Measured on 2026-11-13, the corpus and Crawl copy little map data, because the
VM avoids sharing where it can: a sole owner updates in place, and module and
captured variables release their reference in the fused `UpdateGlobal` and
`UpdateCapture`. The most is 2.7M map elements copied over 1000 Crawl joins
(`crawl-ticks` 151k, `crawl-history` 390k). The large shared copies the corpus
did have were bytes, now fixed. So the case for persistent maps is the
guarantee, that an update costs about what it changes, not a measured
slowdown in today's programs. The redesign's stage 7 gate asks for the
latter; this record asks to accept the former as the reason, provided the
corpus does not regress.

### Constraints

- Map enumeration order is unspecified by the language, but it is
  deterministic today, and programs rely on that. Crawl's empire layer picks
  from `filter(keys(empire.holds), ...)` with its seeded generator
  (`empire.slug`), so a world seed reproduces a world only while key order
  repeats from run to run. Map printing and `json.stringify` output (and tests
  that compare it) follow the same order. Hosted builds hash keys with a
  randomly keyed `SipHash` against hash flooding, so an order taken from hash
  values would change on every run.
- The redesign's invariants hold: numeric key equivalence (`1` and `1.0` are
  one key), a map literal that repeats a key is an error, unique-owner updates
  stay in place, shared values never change, invalid keys are checked errors.
- `no_std` builds (the ESP32-S3) need `alloc` only; forks share immortal data
  through the `crate::sync` wrappers; `.cslug` artifacts and the native API
  see maps only as entries.
- Small, record-like maps are the common case and must not pay for any of it.

## Decision (proposed)

Keep today's architecture, an entry sequence plus an index of positions, and
make both halves persistent above the threshold:

- Up to `INDEX_THRESHOLD` (32) entries, nothing changes: a flat vector with
  no index, copied whole when shared (at most 32 entries).
- Above it, the entries live in a persistent vector (a 32-way trie of
  `(key, value)` with a tail, as Clojure's and `im`'s vectors) and the index
  is a hash array mapped trie from the key's hash to the entry's position.
  Both share structure between versions; an update copies one path in each
  (about log32 n nodes) instead of the whole map.
- Removal keeps today's swap-remove: the last entry moves into the removed
  one's position (one vector set and one pop, plus one index update for the
  moved key). Enumeration order is therefore exactly today's, on every build,
  whatever the hash key: nothing a program prints or iterates changes.
- Nodes are reference counted through the same `crate::sync` types as today's
  storage, and an update takes each node it may change with `make_mut`: a
  sole owner still updates in place, node by node, and only nodes another
  version holds are copied.
- The index keeps the per-map randomly keyed hasher (FNV-1a freestanding),
  shared by the versions derived from a map. A full 64-bit hash collision
  goes to a collision node searched linearly; partial collisions only deepen
  the trie, at most 13 levels.

## Consequences and how it is judged

- A kept version costs O(log n) per later update instead of O(n), in time
  and in memory; the `map` case above becomes about n log n.
- Lookups in large maps walk two to three trie levels where the open-addressing
  index probes once; iteration walks trie leaves instead of one vector. Sole
  owner updates touch a path instead of one slot. These are the risks.
- Accepted only on evidence: an interleaved PGO `make bench-ab` over the
  corpus with hold-outs within their spread (in particular `word-frequency`
  and `graph-search`, which gained most from the index); new workloads that
  keep versions, timed and with peak memory, including
  `crates/slug-vm/benches/memory/large-maps-retained-64.slug` (7.8 MiB today);
  Crawl's per-tick and history timings; unit tests against a reference model
  (20,000 random inserts and removals, as the index has), unchanged
  enumeration order checked against today's implementation on the same
  operation sequences, and the existing aliasing and numeric-key tests.
- If the corpus regresses beyond its spread and cannot be recovered, this
  record says so and stays Proposed; the threshold can also rise if trie
  maps only pay above a larger size.

## Plan

1. A persistent vector and a position HAMT as private modules of
   `slug-vm`, with model-based tests, `no_std` builds, and microbenchmarks
   against `Vec` and the open-addressing index.
2. `MapStorage` above the threshold on them, behind the existing `Map` API,
   with enumeration order checked against the current storage on random
   operation sequences.
3. Measure: the corpus A/B, the retained-version workloads and memory,
   Crawl. Record the outcome here and in the redesign plan.
