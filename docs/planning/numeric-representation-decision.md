# Numeric Representation Decision Plan

## Status and scope

The target numeric representation and arithmetic semantics are an outstanding
language and runtime decision. This document defines the questions, candidate
models, experiments, and completion gates needed to make that decision. It is
not a normative source-language specification, an adopted architecture
decision, or authorization to change numeric behavior.

The original Slug implementation used
[DEC64](https://github.com/douglascrockford/DEC64) as one physical number type.
The Rust implementation currently uses separate `Value::Int(i64)` and
`Value::Float(f64)` representations behind the language-level `num` type.
Checked integer operations retain `i64`; mixed arithmetic and non-integral
division promote to binary `f64`. Neither historical behavior nor the current
implementation alone decides the future language contract.

This decision affects literal parsing, arithmetic, equality and ordering, map
keys, matching, type checking, native calls, configuration, portable compiled
modules, VM value layout, and numeric-loop performance. After a model is
selected, record it under `docs/decisions/` and establish its observable rules
in the applicable normative documents before implementation.

## Required language outcomes

The selected model must answer all of the following without relying on a
host-language numeric conversion:

- whether `0.1 + 0.2 == 0.3` is guaranteed;
- the exact integer range and whether integer overflow fails, rounds, or
  promotes;
- the result of non-integral division such as `1 / 3` and its rounding rule;
- whether decimal and binary floating-point values coexist and, if so, whether
  mixing them is implicit or explicit;
- equality and ordering across every numeric representation, especially above
  the exact binary64 integer range;
- treatment of signed zero, not-a-number values, infinities, underflow, and
  overflow;
- canonicalization requirements when one mathematical value has multiple
  encodings;
- the valid operands and results for remainder, negation, bitwise operators,
  shifts, indices, slice bounds, repetition counts, capacities, and timer
  durations;
- source formatting, parsing, interpolation, and round-trip guarantees;
- conversions at native, configuration, module, fixture, and future `.cslug`
  boundaries; and
- whether numeric representation is observable through type annotations,
  overload identity, matching, or diagnostics.

Checked runtime failure remains mandatory. A numeric implementation must not
expose a host panic or host-dependent result. If exceptional IEEE or DEC64
values are retained, their equality, ordering, formatting, and collection-key
semantics must be defined explicitly.

## Candidate models

### Continue with checked `i64` plus binary64

Keep the current two VM representations. Integer-only operations use checked
hardware arithmetic; mixed operations and non-integral division use `f64`.

This gives the strongest execution performance and native-library
interoperability on ordinary CPUs, preserves the full `i64` range, and keeps
numeric values inline. It also retains binary decimal-fraction artifacts and
requires an exact cross-representation comparison algorithm. Converting an
arbitrary `i64` to `f64` for mixed equality, ordering, or arithmetic is not an
acceptable final rule because integers above `2^53` can lose precision.

### Use one DEC64-compatible value

Represent every number as a 64-bit decimal coefficient and exponent. The
published DEC64 format uses a 56-bit signed coefficient and an 8-bit decimal
exponent. It keeps numeric storage compact, represents common decimal
fractions exactly, and has fast integer and equal-exponent paths.

The tradeoffs are a smaller directly representable integer coefficient than
`i64`, software rescaling and rounding for unequal exponents, wide intermediate
arithmetic for multiplication and division, multiple encodings of some
mathematical values, and nonstandard exceptional-value behavior. General
arithmetic and comparisons require more branches and instructions than the
hardware binary64 path on common targets.

DEC64 compatibility is broader than adopting a similar coefficient/exponent
layout. The decision must state whether Slug retains DEC64's exact range,
normalization freedom, rounding, not-a-number behavior, and exceptional
operation results.

### Use IEEE 754 decimal64 or decimal128

Adopt a standardized decimal interchange and arithmetic model. Decimal64 uses
an eight-byte representation with 16 decimal digits of precision. Decimal128
uses 16 bytes with 34 decimal digits and can preserve a much wider combination
of integer magnitude and fractional precision. The
[IEEE 754-2019 standard](https://standards.ieee.org/ieee/315/6210/) defines the
formats, operations, rounding, and exceptional behavior.

Common development targets do not provide the same ubiquitous decimal
hardware path as binary64, so the VM should assume a software implementation.
Decimal128 may enlarge every inline `Value` and reduce stack and local-slot
cache density. Boxing it avoids enlarging unrelated values but adds allocation
and pointer chasing to ordinary numeric execution.

### Use a fixed-scale integer

Store an `i64` coefficient under one language-wide decimal scale. Addition,
subtraction, equality, and ordering remain simple integer operations;
multiplication and division use wider intermediates and rescaling.

This is attractive for a dedicated money type but has insufficient range and
scale flexibility for a universal Slug `num`. Keep it as a candidate only if
the language requirements establish one bounded application domain or a
separate fixed-point type.

### Use a runtime-specialized numeric family

Keep one language-level `num` while selecting specialized VM forms, for
example:

```text
num
|- Int(i64)
|- Decimal(fixed-width decimal)
`- Float(f64), only for explicit scientific or native interoperability
```

Integer operations retain the current checked fast path. Fractional decimal
literals parse directly from source digits into decimal storage, and
non-integral integer division produces a decimal under a specified rounding
context. Decimal and binary floating-point mixing may require an explicit
conversion so a decimal calculation cannot silently acquire binary artifacts.

This model offers the clearest path to exact ordinary decimal arithmetic and
fast integer-heavy execution, but it has the largest semantic surface:
promotion, overload identity, cross-representation comparison, formatting,
native conversion, and overflow behavior must all be coherent. It is the
primary experimental baseline, not an adopted decision.

### Use arbitrary-precision decimal or rational values

Arbitrary decimal can retain source digits and configurable precision;
rational arithmetic can preserve results such as `1 / 3` exactly. Both models
require variable-sized storage, normalization, and commonly heap allocation.
Rationals additionally incur numerator and denominator growth plus greatest-
common-divisor work.

Evaluate these as explicit library or opt-in value types unless measurements
and language requirements justify their cost as the default `num`.

## Semantic hazards to resolve first

The current Rust implementation converts mixed integers and floats through
`i64 as f64`. Before treating current behavior as a candidate baseline, add a
focused characterization for values around `2^53`, the `i64` bounds,
fractional boundaries, negative zero, infinities, and not-a-number values. The
decision must prevent two distinct exact integers from comparing equal merely
because one was rounded during conversion.

Numeric equality must remain consistent everywhere it is observed:

- `==` and `!=`;
- ordering comparisons;
- literal and pinned match patterns;
- list, map, and struct equality;
- map-key lookup and any future hashed map representation; and
- selected overload or static-type behavior if numeric subtypes become
  visible.

Parsing must operate from the original source digits for every decimal
candidate. Parsing through `f64` and then converting to decimal would preserve
the artifact the decimal representation is intended to remove.

## Prototype and measurement plan

Implement candidates as isolated numeric-operation prototypes before changing
`Value`, the parser, bytecode, or normative semantics. Do not add a generic
numeric abstraction to the production VM solely to host the experiment.

### Phase 1: compatibility corpus

- [ ] Recover representative original Slug DEC64 behavior for literals,
  arithmetic, comparison, formatting, exceptional values, and boundaries.
- [ ] Add current Rust characterization cases without declaring accidental
  behavior normative.
- [ ] Define application-shaped cases for money, measurements, counters,
  scientific notation, configuration values, and native round trips.
- [ ] Identify programs that depend on full-width `i64`, binary64,
  DEC64-specific rounding, or exceptional values.

### Phase 2: semantic candidate profiles

- [ ] Write one result table per candidate covering parsing, arithmetic,
  division, remainder, overflow, rounding, equality, ordering, formatting, and
  conversions.
- [ ] Select a rounding mode and precision context for each decimal candidate.
- [ ] Decide whether exceptional numeric values are language values or checked
  runtime failures.
- [ ] Specify whether conversions are exact, rounded, checked, or explicit.

No benchmark result can compensate for an undefined observable result. Remove
a candidate if its semantics cannot be stated coherently.

### Phase 3: representation prototypes

- [ ] Prototype checked `i64`/binary64 with exact mixed comparisons.
- [ ] Prototype a DEC64-compatible or explicitly documented DEC64-derived
  64-bit representation.
- [ ] Prototype IEEE decimal64 and decimal128 using a well-defined software
  implementation.
- [ ] Prototype the runtime-specialized integer/decimal model with explicit
  binary-float conversion.
- [ ] Include arbitrary decimal or rational only if Phase 1 finds a concrete
  default-number requirement for it.

Every prototype must avoid per-operation allocation for its fixed-width common
case and expose counters for slow paths, rescaling, rounding, overflow,
allocation, and conversion.

### Phase 4: VM-relevant benchmarks

Measure each candidate with identical inputs and repeated samples:

- integer arithmetic, comparison, bitwise operations, and shifts;
- equal-scale and mixed-scale decimal addition and comparison;
- decimal multiplication, division, rounding, and overflow;
- mixed-representation operations where the candidate allows them;
- parsing, formatting, interpolation, configuration, and native conversion;
- numeric loops using calls, branches, `recur`, lists, maps, and matching; and
- application-shaped financial and measurement workloads.

Report at least:

- operation and whole-VM elapsed time;
- executed instructions and branch behavior where supported;
- `Value` size and alignment, values per cache line, and total stack/local
  storage;
- allocation count, allocated bytes, peak resident memory, and reference-count
  traffic;
- code and metadata size;
- result accuracy and rounding events; and
- behavior on x86-64 and ARM64 when both are available.

Keep the numeric benchmark separate from the dispatch, shared-program,
scheduler, and compact-instruction changes in
[VM Optimization Plan](vm-optimization.md). Report isolated operation cost
beside whole-VM workloads so interpreter overhead neither masks nor exaggerates
the application impact.

## Decision gates

Select a representation only when all of these gates are satisfied:

1. The source-visible result table is complete and internally consistent.
2. Decimal parsing and formatting round-trip under the proposed rule.
3. Equality, ordering, matching, and collection-key behavior agree across all
   representations.
4. Integer range, decimal precision, exponent range, rounding, and overflow
   meet the compatibility corpus's stated requirements.
5. Bitwise, index, slice, repetition, capacity, and timer operations retain a
   clear exact-integer rule.
6. Native and configuration conversions do not silently lose precision.
7. Fixed-width common values remain inline, or a measured exception justifies
   allocation and pointer chasing.
8. Numeric-loop performance and memory use are acceptable relative to the
   current checked integer fast path and documented application goals.
9. The choice behaves deterministically on every supported host architecture.
10. Migration from original DEC64 behavior and the current Rust subset is
    documented.

The decision should favor predictable language semantics over the fastest
microbenchmark, then choose the least costly representation that implements
those semantics. A single language-level `num` does not require a single
physical VM representation.

## Required follow-through after selection

The adopting change must include:

- a decision record describing the selected semantics and representation;
- normative rules in `docs/language/language-specification.md` and applicable
  guarantees in `docs/language/runtime-requirements.md`;
- `docs/language/slug.ebnf` updates if literal syntax, suffixes, or conversion
  forms change;
- native-boundary and future `.cslug` numeric-encoding updates;
- focused VM and source/CLI conformance tests for every result-table boundary;
- language-support inventory, README, and changelog updates; and
- benchmark evidence attached to the decision or an adjacent report.

Do not silently migrate constants or bytecode tests by changing Rust enum
variants. Private bytecode may change freely, but each source-visible numeric
result must follow the adopted rule and preserve checked failure behavior.
