# Immutable Collection and Value Redesign

## Status and scope

This is an implementation task list for replacing the VM's direct
`Rc<Vec<Value>>` lists, `Rc<Vec<(Value, Value)>>` maps, and broad `Value` enum
payloads with representations that preserve Slug's immutable-value semantics.
It is not authorization to change collection syntax, numeric semantics, the
native API contract, or the portable `.cslug` format.

The current representation is a sound baseline for small collections: it is
simple, deterministic, and shares complete values cheaply. Its cost is that
every persistent list/map/struct update copies its full vector, while map
lookup and update scan entries linearly. Changes must be staged so that an
internal storage improvement never becomes observable mutation.

## Invariants

- Lists, maps, bytes, structs, schemas, and enum values are immutable Slug
  values. An update returns a new logical value; all aliases retain the old
  value.
- Internal unique-owner reuse is permitted only when it is observationally
  equivalent to allocating a new value. Shared values must never be mutated.
- Channel, task, resource, binding, and closure *handles* may observe external
  or lexical state, but collection values contained in or passed through them
  remain immutable.
- Maps are immutable unordered values. Map enumeration order is unspecified;
  programs requiring an order must order the returned keys explicitly.
- The current numeric equality rule is regression behavior: `1 == 1.0`, and a
  map containing `1` must resolve through `1.0` (and vice versa). A future
  numeric decision may refine boundary cases, but no collection optimization
  may silently choose a different rule.
- Invalid map keys and collection-operation type failures remain checked Slug
  runtime errors, never host panics.

## Sequenced tasks

### 0. Establish collection/value evidence

- [x] Add opt-in metrics for list/map construction, lookup, update, removal,
  merge, slice, and struct-copy operations. Record collection length, whether
  an update had a unique owner, elements copied, and map entries inspected.
- [x] Add representative benchmarks for small (0--8), medium (32--128), and
  large (1,024+) collections. Include repeated persistent updates, read-heavy
  maps, list prepend/append, map merge/remove, struct copies, and retained
  aliases.
- [x] Add peak-RSS fixtures for retained collections and report `Value` layout,
  heap allocations, and reference-count traffic only behind the existing
  metrics feature.

**Gate:** a proposed representation names the workload it improves and the
allocation, copying, or lookup evidence that justifies its extra complexity.

### Stage 0 baseline

The first metrics-enabled run reports a 48-byte `Value`. The 8-element
collection workload performs 8 persistent updates and copies 28 elements per
invocation; the 64-element workload performs 64 updates and copies 2,016
elements; the 1,024-element workload performs 1,024 updates and copies
523,776 elements. All recorded updates are shared under the current
reference-counted vector representation, so unique-owner reuse is the first
candidate optimization to measure in Stage 3. The map workloads deliberately
read an early key repeatedly: a 1,024-element workload inspects 1,048,577 map
entries per invocation, exposing the linear scan cost without claiming an
iteration order guarantee.

On the baseline host, peak RSS was about 2.0 MiB for the minimal fixture, 2.7
MiB for retained 128-map snapshots, and 3.4 MiB for retained 1,024-map
snapshots. These host measurements are directional, not CI thresholds.

### 1. Lock down observable immutability contracts

- [x] Extend VM and CLI coverage for list, bytes, map, and struct updates with
  aliases held in local bindings, closures, spawned tasks, module exports, and
  native round-trips.
- [x] Add map-unorderedness assertions for literals, merges, removals, `map
  copy`, display/debug formatting, and `slug.std.keys`; tests must not rely on
  an enumeration sequence.
- [x] Add map-key regression cases for `1`/`1.0`, strings, bytes, booleans,
  missing keys, duplicate-update behavior, and invalid key categories.
- [ ] Document any discovered observable rules in `docs/language/`; do not
  infer new semantics solely from a Rust data structure.

**Gate:** existing and new tests demonstrate that no alias can observe a
collection update applied through another alias.

### 2. Create private collection seams without changing storage

- [x] Introduce private `List`, `Map`, `Bytes`, and immutable struct-value
  wrappers around the current reference-counted vector/slice storage.
- [x] Move indexing, iteration, equality, display, construction, update,
  merge, removal, slicing, pattern extraction, and native conversion behind
  those wrappers.
- [x] Remove direct collection-vector access from VM operations, source-facing
  native helpers, FFI prototypes, and tests that do not intentionally inspect
  private bytecode builders.
- [ ] Keep constructors and views borrowing where possible; do not expose a
  mutable collection view.

**Gate:** `make check` passes with no source-language behavior change, and the
collection backing type can be changed in one module rather than across the
VM.

### 3. Optimize unique-owner persistent updates

- [x] Implement internal consume-or-copy update paths for lists, maps, bytes,
  and structs using `Rc::try_unwrap`/`Rc::make_mut` only behind immutable
  wrapper operations.
- [x] Preserve exact old-value behavior for shared aliases and retain existing
  insertion-order rules for maps.
- [x] Measure copied elements and allocations for unique versus shared updates;
  retain the optimization only if ordinary construction/update workloads
  improve without regressing retained-alias workloads materially.

**Gate:** the optimization is invisible to Slug code, closures, tasks, module
instances, and native clients.

### 4. Resolve map-key equivalence before adding an index

- [ ] Coordinate with the [numeric representation decision](numeric-representation-decision.md)
  to specify cross-representation numeric equality, signed zero, non-finite
  values if supported, and values outside exact binary64 integer range.
- [ ] Specify an internal `MapKey` canonicalization/equality/hash contract such
  that equal Slug keys always hash equally. It must cover boolean, numeric,
  string, and bytes keys and reject every other current invalid key class.
- [ ] Add conformance tests covering key equality, lookup, replacement, merge,
  removal, patterns, and native map conversion for every key family.
- [ ] Record the adopted key contract in `docs/language/` and a decision record
  before replacing linear lookup with any hashed or indexed representation.

**Gate:** no indexed map lands until the key-equivalence contract is complete.
The present vector representation remains the safe implementation because it
uses language equality directly.

### 5. Evaluate a measured persistent map representation

- [ ] Prototype a persistent hash map, HAMT, or a measured hybrid representation
  once Stage 4 provides a valid `MapKey` contract.
- [ ] Prove that lookup, update, merge, removal, equality, formatting, and
  `keys(map)` preserve the Stage 1 contracts without exposing iteration order.
- [ ] Compare candidates with vector-backed maps for small, medium, and large
  workloads, including allocation and retained-memory cost.

**Gate:** key behavior remains source-compatible, enumeration remains
unspecified, and the chosen representation is justified by benchmark evidence.

### 6. Narrow the dynamic `Value` representation

- [ ] Measure each `Value` variant's size and frequency on representative
  workloads before changing the enum.
- [ ] Prototype moving only heavyweight composite/runtime payloads behind a
  heap-backed internal variant while retaining nil, booleans, integers, floats,
  and compact string/byte handles inline.
- [ ] Audit equality, debug/display, error values, pattern matching, native
  conversion, closure/task retention, and resource teardown for the new
  indirection boundary.

**Gate:** no unsafe tagged-pointer scheme, custom allocator, or universal
per-value heap allocation is introduced without a separate measured decision.

### 7. Consider persistent trees only when needed

- [ ] Prototype a persistent vector trie, rope, HAMT, or equivalent only if
  Stage 0/3/5 evidence identifies repeated large shared updates as material.
- [ ] Compare it with the wrapper plus unique-owner implementation using the
  same aliasing and ordering conformance matrix.
- [ ] Record a decision explaining why the added pointer depth, allocations,
  implementation complexity, and FFI implications outweigh the flat-vector
  baseline.

**Gate:** this stage is optional. It is not justified solely by asymptotic
complexity or by the existence of immutable collections in other runtimes.

## Delivery rules

Land one stage or one independently measurable slice per commit. Run the
narrowest relevant VM/native/CLI tests while iterating, then `make check`
before handoff. Update the language documents and create a decision record only
when an implementation adopts a new observable collection, map-key, or numeric
contract.
