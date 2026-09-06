## Definite checkable tasks

Phase 3 should be implemented as a sequence of independently verifiable tasks.

Each task must have:

- a clearly defined semantic rule;
- focused compiler tests;
- at least one positive case;
- at least one negative or uncertainty-preservation case where applicable;
- no unrelated language behaviour changes.

A task is complete only when its tests pass and existing test suites remain green.

### Task 1 — Audit every expression kind

Create an explicit inventory of every `ExprKind` handled by semantic analysis.

For each expression kind, record:

```text
result type rule
required child types
environment effects
when Unknown is valid
when a compile error is required
```

Add a test or explicit follow-up task for every expression kind that currently returns `Unknown` without a deliberate semantic reason.

**Check:** no expression kind is left with undocumented inference behaviour.

#### P0 inference inventory

This table is the implementation inventory for Task 1. `Unknown` means the
checker cannot establish a safe static fact; it is not a source-level type.
“Dynamic boundary” is intentional uncertainty. “Follow-up” identifies the
numbered task that must reduce an avoidable fallback.

| `ExprKind` | Result rule | Child requirements and environment effect | `Unknown` is valid when | Compile error is required when |
| --- | --- | --- | --- | --- |
| `Value` | `value_type(value)` | No children; no environment change. | The runtime-only `NativeResource`, `Uninitialized`, or `Binding` value is encountered. | Never from the literal itself. |
| `Interpolate` | `str` | Embedded names are resolved by compilation; no semantic child expression. | Never. | Name resolution rejects an invalid interpolation reference. |
| `Documentation` | `nil` | No children; declaration metadata is recorded by its owning declaration. | Never. | Never. |
| `NotImplemented` | `Unknown` | No children; no environment change. | Always: parser placeholder and future-language boundary. | Never in the checker; syntax support owns the diagnostic. |
| `Name` | Bound value type | No children; reads the current binding. | The name has no semantic binding (dynamic/global lookup). | Only where a surrounding construct requires a known callable, member, or operand. |
| `Declare` | Initializer result | Check initializer, check annotation assignability, and bind the pattern; an annotation controls the exposed binding type. | The initializer is dynamic. | A known initializer violates its annotation or pattern requirements. |
| `Foreign` | Declared function value | Validate defaults and register its callable/resource signature. | Its unannotated result is intentionally dynamic. | Defaults or declared annotations are incompatible. |
| `Resource` | `nil` | Registers a nominal type in the type environment. | Never. | Duplicate or invalid resource declarations. |
| `Enum` | `nil` | Registers the enum and its case values. | Never. | Duplicate/invalid enum or case declarations. |
| `TypeAlias` | `nil` | Registers a transparent resolved alias in the type environment. | Never. | The alias is unresolved, cyclic, or invalid. |
| `Assign` | Assigned value type | Check value; update the existing mutable binding according to Task 5. | The assigned expression is dynamic. | A known value violates the binding contract. |
| `Return` | Returned value type | Check child; enclosing function consumes the result. | The returned expression is dynamic. | A known result violates an annotated function return. |
| `Throw` | Internal non-returning result (currently `Unknown`) | Check error child; no normal environment fact follows. | Never after Task 28. | Never from the error value alone. |
| `Defer` | `nil` | Check deferred child; defer bookkeeping is compiler/runtime state. | Never. | The deferred expression has a known semantic error. |
| `Recur` | Internal non-returning result (currently `Unknown`) | Check every argument against the recursive callable contract. | Never after Task 28. | Arguments are known incompatible or recur is invalid in context. |
| `Nursery` | Body result | Optional limit and body are checked; nursery scope is compiler/runtime state. | The body result is dynamic. | Limit/body has a known semantic error. |
| `Spawn` | `task<widen(body)>` | Check body; no ordinary binding change. | Only when the body is dynamic. | Body has a known semantic error. |
| `Select` | Normalized union of handler results | Check each operation and handler; case-local facts do not escape. | A case has no handler or its handler is dynamic; Task 26 must distinguish non-returning/default behavior. | A known send/receive/await/timer operand is invalid. |
| `Match` | Normalized union of case results | Check subject, patterns, guards, and case-local bindings. | A case result is dynamic or no value-producing case exists; Task 27 refines this. | Patterns, guards, coverage, or case reachability are known invalid. |
| `Binary` | `binary_result(operator, left, right)` | Check operands; `and`/`or` apply nil facts to the right; pipeline delegates to call checking. | An operand is dynamic, or an operator rule remains intentionally dynamic. | A known operator/operand pair is incompatible. |
| `Prefix` | Fold `prefix_result` right-to-left | Check operand; no environment change. | Operand/operator is dynamic; unsupported known cases are Task 6 follow-up. | A known invalid operand is rejected by the operator rule. |
| `Call` | Selected callable result | Check callee and arguments; callable/generic resolution may record selected-call metadata. | Callee, function shape, or result is genuinely dynamic. | A known callable has invalid shape, generics, or argument types. |
| `TypeApply` | Callee result/type | Check callee; explicit type arguments are validated during callable resolution. | Callee is dynamic. | Type arguments do not satisfy a known callable. |
| `Function` | `fn(parameters):body-result` | Parameters enter a child scope; defaults and body are checked; function metadata is recorded. | An unannotated body cannot be inferred. | Defaults/body violate known parameter or return contracts. |
| `Block` | Final expression, or `nil` when empty | Child scope; compatible outward facts merge after checking. | Final expression is dynamic. | A contained expression has a known semantic error. |
| `If` | Normalized union of then/else (`nil` if absent) | Check condition; apply nil facts independently to branch environments and merge compatible facts. | A branch is dynamic; Task 28 removes non-returning contamination. | Condition/branch contains a known semantic error. |
| `List` | `list<union(elements)>`, or unparameterized list | Check elements and unpack known list spreads. | A spread is dynamic/non-list, requiring unparameterized list; Task 11 limits precision loss. | A known invalid spread is rejected once the spread contract is made static. |
| `Map` | `map<union(keys), union(values)>`, or unparameterized map | Check keys and values independently. | Only the empty map lacks key/value facts. | A contained key/value has a known semantic error. |
| `StructSchema` | `schema` | Check field defaults; schema members/required fields are captured in the binding. | A field without annotation/default has an unknown member type. | A default violates its known annotation. |
| `StructInit` | Exact nominal struct when schema identity is known; otherwise `struct` | Check schema and field values. | Schema identity is dynamic. | Schema is invalid, fields are duplicate/missing/unknown, or values mismatch. |
| `StructCopy` | Source struct type; widened map type for a map copy | Check source and replacements. | Source is dynamic. | Known struct fields are duplicate/unknown or replacement values mismatch. |
| `Index` | Known element/value/field result, including `map<K,V>[K] -> V|nil` | Check collection and index; static string fields can use member bindings. | Collection/index is dynamic, collection is unparameterized, or a struct field is not statically resolved; Tasks 13 and 15 refine these cases. | A known collection is not indexable, index type is wrong, or known struct field is absent. |
| `Slice` | Preserve list element type, `str`, or `bytes` | Check collection and numeric bounds. | Collection is dynamic; unknown collection families remain a Task 14 follow-up. | Bounds are known non-numeric or known collection cannot be sliced. |

The direct `Type::Unknown` sites in `check_expression` are therefore
classified as follows: `Name`, dynamic `Call`, and dynamic operator/index/slice
operands are intentional dynamic boundaries; `NotImplemented` is a
future-language boundary; untyped foreign/function/schema members are
genuinely unknowable until a contract is supplied; and `Throw`, `Recur`,
unhandled `Select` cases, imprecise spreads, static struct/index fallbacks, and
remaining incomplete operator rules are Phase 3 follow-ups (Tasks 6, 7,
11–15, 20, 26, and 28).

---

### Task 2 — Lock down scalar literal inference

Verify and test:

```text
nil        → nil
bool       → bool
number     → num
string     → str
bytes      → bytes
```

**Check:** bindings initialized from each literal retain that inferred type through later references.

**Status:** complete — `tests/cli/types_and_metadata.rs` proves both compatible
calls and incompatible annotated uses for every scalar literal family.

---

### Task 3 — Preserve inferred binding types

Ensure:

```slug
val x = expression
```

stores the inferred type of `expression` in the semantic environment.

Verify transitive propagation:

```slug
val a = "slug"
val b = a
val c = b
```

All three must be known as `str`.

**Check:** a later incompatible operation on `c` fails at compile time.

**Status:** complete — `tests/cli/types_and_metadata.rs` proves `a -> b -> c`
retains `str` and reports the resulting incompatible numeric operation.

---

### Task 4 — Enforce annotated bindings without losing initializer facts

For:

```slug
val x:T = expression
```

verify that:

1. `expression` is inferred normally;
2. assignability to `T` is checked;
3. the binding is exposed as `T`.

Test widening cases such as:

```slug
val x:num|nil = 10
```

**Check:** initializer is accepted as `num`; binding is subsequently treated as `num|nil`.

**Status:** complete — `tests/cli/types_and_metadata.rs` proves an inferred
`num` initializer is accepted by `num|nil` and that later reads retain the
declared nullable type.

---

### Task 5 — Define mutable binding inference behaviour

Document and implement the rule for reassignment of inferred mutable bindings.

Test at least:

```slug
var x = 1
x = 2
```

and the heterogeneous case:

```slug
var x = 1
x = "slug"
```

The latter must either:

- widen according to a documented rule; or
- fail according to a documented fixed-binding rule.

It must not depend accidentally on analysis order.

**Check:** behaviour is explicit and covered by tests.

**Status:** complete — inferred `var` bindings use the fixed-initializer rule
in `docs/language/language-specification.md`; the CLI regression covers both
compatible reassignment and a rejected heterogeneous reassignment.

---

### Task 6 — Centralize unary operator inference

Create or consolidate one semantic rule for prefix operators.

Verify known cases such as:

```text
-num → num
```

and boolean/logical forms according to Slug semantics.

**Check:** known-invalid operands fail during compilation; unknown operands remain dynamic where permitted.

**Status:** complete — the existing centralized `prefix_result` rule is covered
by CLI cases for numeric, logical, and bytewise prefix results, known-invalid
operands, and an unannotated dynamic operand.

---

### Task 7 — Centralize binary operator inference

Create a single semantic operation equivalent to:

```text
infer_binary(operator, left_type, right_type)
    → result type
    | incompatibility
    | unknown
```

Cover arithmetic, comparison, equality, concatenation, and logical operators according to Slug semantics.

**Check:** operator inference is not duplicated inconsistently across checker code paths.

**Status:** complete — the existing `binary_result` dispatch is covered by CLI
cases for arithmetic, comparison, equality, concatenation, logic, dynamic
operands, and known-incompatible operands.

---

### Task 8 — Complete block result inference

Verify that a block returns the type of its final value-producing expression.

Test:

```slug
{
    val x = 10
    x * 2
}
```

as `num`.

Also test empty and non-value-producing blocks according to Slug semantics.

**Check:** local scope ends correctly while the block result type survives.

**Status:** complete — CLI coverage proves a final numeric block result, an
empty branch block as `nil`, and that the block-local binding is unavailable
outside the block.

---

### Task 9 — Complete `if` result inference

Infer the normalized union of all possible result branches.

Test:

```slug
if flag { 1 } else { 2 }
```

as:

```text
num
```

and:

```slug
if flag { 1 } else { "one" }
```

as:

```text
num|str
```

Also test `if` without `else`.

**Check:** branch results normalize rather than producing duplicate or unnecessarily broad unions.

**Status:** complete — CLI coverage proves same-type normalization,
heterogeneous branch unions, and the implicit `nil` result without `else`.

---

### Task 10 — Complete list literal inference

Infer list element types from all elements.

Test:

```slug
[1, 2, 3]
```

as:

```text
list<num>
```

and:

```slug
[1, "two"]
```

as:

```text
list<num|str>
```

Define and test the empty-list representation.

**Check:** nested lists preserve their contained types.

**Status:** complete — CLI coverage proves homogeneous, heterogeneous, nested,
and empty list representations, plus a rejected incompatible element union.

---

### Task 11 — Complete list spread inference

Verify spread propagation:

```slug
val a = [1, 2]
val b = [3, ...a]
```

must infer:

```text
list<num>
```

Test known mixed and genuinely unknown spread operands.

**Check:** a known spread element type contributes to the list union; a genuinely
unknown spread produces an unparameterized list without converting `Unknown`
into `any`.

**Status:** complete — `docs/language/language-specification.md` now defines
the distinction, and CLI coverage proves known typed spreads, `list<any>`, and
an unparameterized list from a genuinely unknown spread.

---

### Task 12 — Complete map literal inference

Infer key and value types independently.

Test:

```slug
{
    "one": 1,
    "two": 2
}
```

as:

```text
map<str,num>
```

Test heterogeneous keys and values and define the empty-map representation.

**Check:** uncertainty in values does not unnecessarily erase known key types, and vice versa.

**Status:** complete — CLI coverage proves independently inferred key/value
unions, the empty-map representation, and a rejected value-only mismatch.

---

### Task 13 — Complete index inference

Define result inference for every statically known indexable type.

At minimum test existing supported forms such as:

```text
list<T>[...] → T
map<K,V>[...] → defined map lookup result
str[...] → defined string index result
bytes[...] → defined byte index result
```

**Check:** known-invalid index operations fail statically.

**Status:** complete — CLI coverage proves list, map, string, and bytes index
results, including the nilable map lookup contract and a static index mismatch.

---

### Task 14 — Complete slice inference

Verify that slicing preserves collection family and contained type.

Examples:

```text
list<File>[slice] → list<File>
str[slice]        → str
bytes[slice]      → bytes
```

**Check:** slicing does not collapse a known collection to `Unknown`.

**Status:** complete — CLI coverage proves list element precision and string/
bytes family preservation through slices, plus a static non-sliceable error.

---

### Task 15 — Complete member-access inference

Known members must return their declared or inferred semantic type.

Test:

- struct fields;
- module members;
- imported functions;
- exported values.

**Check:** crossing a member-access boundary does not discard semantic type information.

**Status:** complete — `tests/cli/types_and_values.rs` covers typed struct
fields and chained map members; `tests/module_loader.rs` covers imported
callable/export snapshots and their retained signatures.

---

### Task 16 — Preserve nominal struct inference

Struct construction must infer the exact nominal struct type.

Test:

```slug
val x = Person { ... }
```

as:

```text
Person
```

Verify that field access and passing the value through bindings retain `Person`.

**Check:** two structurally similar but nominally distinct structs remain distinct where Slug requires nominal identity.

**Status:** complete — `tests/cli/types_and_values.rs` proves schema aliases
retain nominal construction identity and rejects assignment between distinct
otherwise empty schemas.

---

### Task 17 — Preserve enum inference

Every enum case must infer its owning enum type.

Test:

```slug
val direction = Direction.North
```

as:

```text
Direction
```

and:

```slug
[Direction.North, Direction.South]
```

as:

```text
list<Direction>
```

**Check:** cases from distinct enum types are not conflated.

**Status:** complete — `tests/cli/types_and_values.rs` proves qualified enum
values satisfy their owning nominal type and enum coverage rejects a distinct
enum's identically named case.

---

### Task 18 — Preserve nominal resource inference

Foreign functions returning resources must propagate their declared resource type.

Test:

```slug
val file = open("a")
```

as:

```text
File
```

and:

```slug
[open("a"), open("b")]
```

as:

```text
list<File>
```

**Check:** passing a known wrong resource type to a foreign or Slug function fails statically.

**Status:** complete — CLI coverage proves `fs.openRead` retains `fs.File`
through a binding and `list<fs.File>`; existing resource call tests reject
known wrong resource arguments.

---

### Task 19 — Verify transparent alias behaviour

Test that aliases preserve compatibility without creating nominal identity.

For:

```slug
type Path = str
```

verify that inferred `str` values satisfy `Path` parameters and vice versa.

**Check:** aliases improve naming without changing assignability.

**Status:** complete — `tests/cli/types_and_values.rs` proves transparent
aliases work in bindings, collection elements, function parameters, unions,
and runtime-checkable constraints without introducing nominal identity.

---

### Task 20 — Complete known-call result inference

When callable resolution selects a known callable, the call expression must return that callable's known result type.

Cover:

- ordinary functions;
- imported functions;
- overload-selected functions;
- foreign functions;
- known callable values.

**Check:** selected result types survive through subsequent expressions.

**Status:** complete — `tests/cli/types_and_metadata.rs` covers ordinary and
structural callable values; `tests/module_loader.rs` covers imported and
overload-selected callables; `tests/cli/filesystem.rs` covers typed foreign
results.

---

### Task 21 — Make pipeline inference identical to call inference

For every pipeline form, verify that its result type matches the equivalent explicit call.

**Check:** pipelines contain no separate degraded inference path.

**Status:** complete — `tests/cli/types_and_values.rs` covers chained pipeline
execution, and `tests/module_loader.rs` covers a pipeline through a selected
typed imported overload.

---

### Task 22 — Complete obvious function-result inference

For functions without explicit return annotations, infer obvious result types from their bodies.

Test:

```slug
fn answer() {
    42
}
```

as:

```text
fn():num
```

and branching result unions where already supported.

**Check:** simple functions no longer default to `Unknown` when the result is obvious.

**Status:** complete — `tests/cli/types_and_metadata.rs` proves an
unannotated numeric function is retained as a precise function value and its
inferred result rejects an incompatible later annotation.

---

### Task 23 — Preserve explicit function return contracts

For annotated functions:

```slug
fn answer():num {
    ...
}
```

verify that all known value-producing returns are assignable to `num`.

**Check:** inferred body information is used to validate the declared contract.

---

### Task 24 — Preserve `Task<T>` result inference

Verify:

```slug
val task = spawn {
    compute()
}
```

where `compute():num` produces:

```text
task<num>
```

**Check:** awaiting or otherwise consuming the task preserves `num` according to existing task semantics.

---

### Task 25 — Preserve channel element types

Where a channel is known as:

```text
channel<T>
```

verify that existing send/receive operations preserve `T`.

**Check:** known-invalid sends fail statically and receives do not collapse to `Unknown`.

---

### Task 26 — Complete `select` result inference

Infer the normalized union of value-producing case branches.

**Check:** case result types compose correctly, including same-type normalization.

---

### Task 27 — Complete `match` result inference

Infer the normalized union of case result expressions.

Test same-type and heterogeneous cases.

**Check:** known pattern-bound types are preserved inside case bodies wherever existing pattern semantics provide them.

---

### Task 28 — Introduce an internal non-returning type

Stop using ordinary `Unknown` for expressions that do not produce a value because control flow terminates or transfers.

At minimum review:

```text
throw
recur
```

Introduce an internal representation equivalent to `Never` or bottom if needed.

This need not become Slug source syntax.

**Check:** this:

```slug
if condition {
    10
} else {
    throw "failed"
}
```

infers:

```text
num
```

rather than `num|unknown`.

---

### Task 29 — Normalize unions consistently

Create focused tests for union normalization produced through inference.

At minimum:

```text
str|str        → str
str|(num|str)  → str|num
File|File      → File
```

Also lock down the interaction between unions, nil, `any`, `Unknown`, and the internal non-returning type.

**Check:** repeatedly composing expressions does not continuously inflate equivalent union types.

---

### Task 30 — Preserve inferred types across imports

Module A:

```slug
export val name = "Slug"

export fn count() {
    10
}
```

Module B must observe equivalent semantic information to:

```text
name  : str
count : fn():num
```

without requiring explicit annotations solely for export.

**Check:** module semantic snapshots retain inferred exported types.

---

### Task 31 — Add nested composition tests

Add tests deliberately combining multiple inference boundaries.

Examples should include forms equivalent to:

```slug
val result = [
    if condition {
        open("a")
    } else {
        open("b")
    }
]
```

Expected:

```text
list<File>
```

and:

```slug
val result = transform(
    source()
)
```

where known call results flow through multiple bindings/calls.

**Check:** inference works compositionally rather than only in isolated unit cases.

---

### Task 32 — Audit remaining `Unknown` production

After the previous tasks are complete, search the semantic checker for every construction of or fallback to `Type::Unknown`.

For each remaining site, classify it as:

```text
genuinely unknowable
intentional dynamic boundary
future-phase limitation
bug
```

Document future-phase limitations as concrete follow-up items.

**Check:** no unexplained `Unknown` remains in ordinary-expression inference.

---

### Task 33 — Verify diagnostic quality from inferred types

Add negative tests proving inferred facts improve diagnostics.

Examples:

```slug
val value = "hello"
value - 1
```

and:

```slug
val socket = connect(...)
read(socket)
```

should report meaningful semantic types such as:

```text
expected File, found Socket
```

rather than internal runtime representations.

**Check:** new inference failures surface as normal Slug source diagnostics.

---

### Task 34 — Run full regression and conformance suites

Once Phase 3 inference work is complete:

- run formatting;
- run clippy with warnings denied;
- run unit tests;
- run VM tests;
- run CLI tests;
- run module-loader tests;
- run language/conformance tests;
- run documentation consistency checks.

**Check:** stronger inference introduces no unintended runtime or language regressions.

---

## Phase completion gate

Phase 3 should not be considered complete merely because the checker can infer more types.

It is complete when all of the following can be demonstrated:

```text
ordinary expressions produce deliberate result types
        +
known information survives composition
        +
Unknown has only intentional uses
        +
known incompatibilities are rejected
        +
dynamic uncertainty remains valid
        +
module boundaries preserve semantic knowledge
        +
all regression suites pass
```

A useful final audit question is:

> **For any ordinary Slug expression, can we explain exactly why the compiler knows its result type—or exactly why it cannot?**

If the answer is yes throughout the checker, Phase 3 has achieved its purpose.

## Execution roadmap

The 34 tasks above are the acceptance criteria.  Execute them through the
following bounded work packages so that each change has one clear owner,
minimal overlap in `src/source/typecheck.rs`, and a focused test boundary.
Later packages must not begin until their listed dependency gates pass.

| Package | Includes | Deliverable and dependency gate | Primary proof |
| --- | --- | --- | --- |
| P0 — Inference inventory | 1 | Add the `ExprKind` inference matrix, classify every existing `Type::Unknown`, and turn each accidental fallback into a later tracked item. This is documentation and tests only; do not alter semantics. | Focused `tests/cli/types_and_metadata.rs` coverage plus `git diff --check` |
| P1 — Binding and result lattice | 2–5, 8–9, 28–29 | Define the binding update policy and implement the shared result-composition helpers: normalized union and internal non-returning result. Blocks and conditionals must use those helpers. **Gate:** all primitive values, bindings, `if`, and terminating branches retain deliberate types. | `make test-cli` |
| P2 — Operators and collections | 6–7, 10–14 | Make unary/binary rules single-source, then complete list/map, spread, index, and slice inference using the P1 lattice. **Gate:** every supported ordinary collection operation either has a precise result or a checked static error. | `make test-cli` and `make test-vm` |
| P3 — Nominal values and member paths | 15–19 | Preserve member, struct, enum, resource, and alias information across binding and static member access. Keep nominal and transparent-alias semantics distinct. **Gate:** known member paths never degrade a known type. | `make test-cli`; `cargo test --features metrics --test module_loader` |
| P4 — Calls and function bodies | 20–23 | Carry selected callable results through direct calls and pipelines; infer unannotated body results and validate annotated return contracts. **Gate:** a known callable's result is identical at direct, piped, local, and imported call sites. | `make test-cli`; `cargo test --features metrics --test module_loader` |
| P5 — Concurrent and branching forms | 24–27 | Propagate task/channel payloads and compose `select`/`match` results through the P1 union helpers. **Gate:** pattern and handler facts survive into result inference. | `make test-cli` and `make test-vm` |
| P6 — Cross-module composition | 30–31 | Export inferred value and callable snapshots, then add nested end-to-end compositions spanning branches, collections, calls, and imports. **Gate:** no explicit annotation is needed solely to retain a known export type. | `cargo test --features metrics --test module_loader`; `make test-cli` |
| P7 — Unknown and diagnostics audit | 32–33 | Re-audit every `Type::Unknown` site after the preceding work, document intentional dynamic boundaries, and add user-facing diagnostic assertions. **Gate:** every remaining fallback is classified and ordinary known mistakes name source-level types. | `make test-cli` |
| P8 — Release verification | 34 | Run the entire repository quality gate only after P0–P7 are complete; resolve regressions within the package that caused them. | `make check` and `make docs-check` |

### Suggested ticket sequence

Create one ticket per package, not one ticket per numbered acceptance
criterion. Keep the numbered tasks as the ticket checklist. P0 is safe to run
in parallel with implementation planning; P1 through P7 are deliberately
serial because they edit the same inference dispatcher and depend on the type
composition rules introduced before them. P8 is verification only.

Every implementation ticket should state:

1. the numbered tasks it closes;
2. the exact rule being introduced or preserved;
3. the focused test file(s) it will change;
4. its dependency gate from the table; and
5. that it must leave unrelated `Type::Unknown` sites unchanged and documented.
