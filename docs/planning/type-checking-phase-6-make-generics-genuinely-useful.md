## Phase 6 — Make generics genuinely useful

Slug already has generic call inference, explicit type arguments, substitution, and generic-aware overload resolution.

This phase should make that machinery reliable enough for ordinary library APIs.

The governing principle is:

> **Calling a generic function must not discard type information the compiler already knows.**

Generic variables represent non-nil types. Nullability is expressed explicitly with `T|nil` where it forms part of the API contract.

The goal is not traits, type classes, constraints, higher-kinded types, or sophisticated generic programming. It is predictable parametric typing for normal Slug code.

### Task 1 — Lock down basic argument-to-result inference

Prove the fundamental case:

```slug id="g1z7po"
fn first<T>(values:list<T>):T|nil
```

with:

```slug id="m8pswh"
val names = ["a", "b"]
val name = first(names)
```

inferring:

```text id="02tkcx"
name : str|nil
```

Cover primitive, nominal, enum, struct, and resource element types.

### Task 2 — Prove inference through every generic container

Add focused tests for generic parameters nested within:

```text id="jvkhle"
list<T>
map<K,V>
chan<T>
task<T>
fn<T>
tuple-like internal types
union positions
```

Examples should include:

```slug id="jjct9c"
fn keys<K,V>(values:map<K,V>):list<K>
fn await<T>(task:task<T>):T
fn recv<T>(channel:chan<T>):T|nil
```

The inferred argument must survive substitution into the result.

### Task 3 — Preserve nominal identity through generic inference

Generic substitution must retain the exact nominal identity established in Phase 4.

For example:

```slug id="a5vlv3"
fn identity<T>(value:T):T
```

called with:

```text id="wmswsj"
slug.db.sqlite.Database
```

must return that exact resource type, not an unresolved or reconstructed `Database`.

Cover resources, enums, and structs across module boundaries.

### Task 4 — Support multiple occurrences of the same generic parameter

For:

```slug id="1v6rrs"
fn choose<T>(left:T, right:T):T
```

both arguments must constrain the same `T`.

Compatible calls succeed:

```slug id="nnoyh6"
choose("a", "b")
```

while incompatible calls are rejected:

```slug id="fb0jrh"
choose("a", 42)
```

Generic inference must not synthesize a wider union merely to make incompatible arguments fit.

In particular, the call above must not silently infer:

```text id="cl5v8g"
T = str|num
```

The same rule must apply when occurrences are nested:

```slug id="5vuhc8"
fn append<T>(values:list<T>, value:T):list<T>
```

### Task 5 — Support multiple independent generic parameters

Verify inference remains independent for signatures such as:

```slug id="8n8tyq"
fn lookup<K,V>(values:map<K,V>, key:K):V|nil
```

so:

```text id="fobxzs"
K = str
V = File
```

produces:

```text id="nuhjv4"
File|nil
```

without one substitution contaminating another.

### Task 6 — Infer generics through explicit nullable positions

Retain the rule that a generic type variable does not itself bind to `nil` or to a type containing `nil`.

Nullability must instead be expressed where the generic is used.

For example:

```slug id="aht80d"
fn first<T>(values:list<T>):T|nil
```

accepts lists whose elements are non-nil and returns `nil` for the empty case.

Where nullable elements are intentionally supported:

```slug id="4sr6fv"
fn first<T>(values:list<T|nil>):T|nil
```

accepts lists whose elements may contain `nil`.

Generic inference must understand these nullable type expressions structurally.

For example:

```text id="vz6l04"
expected: T|nil
actual:   str|nil
```

must infer:

```text id="rkk1qj"
T = str
```

Likewise:

```text id="ue0rpl"
expected: list<T|nil>
actual:   list<File|nil>
```

must infer:

```text id="3vdimf"
T = File
```

Do not infer:

```text id="u4c4y7"
T = str|nil
```

or otherwise widen a generic variable to include `nil`.

The language rule is:

> **Generic variables represent non-nil types. Nullability is expressed explicitly with `T|nil` where it is part of the API contract.**

### Task 7 — Make explicit generic arguments equivalent to inferred ones

For a generic function:

```slug id="jxx1wc"
fn first<T>(values:list<T>):T|nil
```

these should produce equivalent instantiated signatures:

```slug id="d7iwwp"
first(["a", "b"])
first<str>(["a", "b"])
```

Explicit type arguments must still validate all supplied arguments against the resulting substituted parameter types.

Explicit generic arguments containing `nil` remain invalid:

```slug id="fvh0nc"
first<str|nil>(...)
```

Nullability must instead be represented by the function's parameter type where required.

Wrong generic arity must remain a static error.

### Task 8 — Preserve generics across module boundaries

Export generic functions from one module and call them from another.

The semantic module snapshot must retain enough generic signature information to infer:

```text id="5a9vjf"
T
K,V
result substitutions
```

at the consuming call site.

Do not specialize or erase exported generic signatures while building module snapshots.

### Task 9 — Preserve generics through imported aliases and destructuring

Verify generic callable information survives ordinary ways of obtaining exported functions:

```slug id="24vn7p"
val lib = import("example")
val first = lib.first
```

and:

```slug id="g51pqj"
val { first } = import("example")
```

Calling `first(...)` must behave the same as calling the module member directly.

### Task 10 — Preserve generic callable information through local bindings

A generic function must not lose its generic signature merely because it is assigned or aliased:

```slug id="2tl7nn"
val head = first
val name = head(names)
```

If the current structural `fn<...>` representation cannot retain generic parameters, keep the richer callable metadata attached to the binding rather than collapsing prematurely to an erased function type.

### Task 11 — Make higher-order generic APIs useful

Support generic parameters nested inside callable arguments:

```slug id="rd9q90"
fn apply<T,R>(value:T, transform:fn<T,R>):R
```

A call such as:

```slug id="x6i35w"
val size = apply("hello", len)
```

should preserve the relationship among argument, callback, and result types wherever those types are statically known.

Keep this limited to direct parametric substitution; do not introduce higher-kinded types or trait-style constraints.

### Task 12 — Fix generic inference with variadic parameters

For signatures such as:

```slug id="j7pwsa"
fn collect<T>(...values:T):list<T>
```

all supplied variadic arguments should participate in inference.

Repeated arguments must consistently constrain `T`.

For example:

```slug id="r3od8z"
collect("a", "b", "c")
```

infers:

```text id="fs7j9k"
T = str
```

while:

```slug id="a6w0qf"
collect("a", 42)
```

must not invent `str|num` merely to satisfy the call.

Zero supplied variadic arguments may remain unresolved unless explicit type arguments or another parameter establish `T`.

### Task 13 — Address spread arguments deliberately

The current call binder bypasses normal parameter inference whenever a call contains a spread.

Improve this where the spread type contains enough static information.

For example:

```slug id="50dhk3"
val values:list<str> = ...
collect(...values)
```

should be capable of inferring:

```text id="by76vz"
T = str
```

when the signature makes that relationship unambiguous.

Likewise, if the signature explicitly permits nullable elements:

```slug id="5ejnpk"
fn collectNullable<T>(...values:T|nil):list<T|nil>
```

then a known:

```text id="8v4bpm"
list<str|nil>
```

spread should provide evidence for:

```text id="2jn5du"
T = str
```

Do not invent element types for unparameterized or dynamic spreads.

### Task 14 — Preserve generic information through defaults and named arguments

Generic inference must be based on whichever arguments are actually supplied, independent of positional or named syntax.

Defaulted parameters should participate only when their statically known type provides useful constraints consistent with the language's existing default-argument semantics.

### Task 15 — Make generic overload resolution deterministic

Retain the current generic-aware overload machinery and add regression coverage for combinations such as:

```text id="p0oyfb"
concrete vs generic
generic vs generic
generic vs variadic generic
```

A more specific concrete candidate should win where applicable.

Equivalent or incomparable generic instantiations must produce an ambiguity diagnostic rather than depending on declaration order.

### Task 16 — Do not allow unresolved generics to masquerade as proof

After candidate inference, distinguish genuinely inferred generic parameters from unresolved ones.

If an unresolved `T` affects only information that cannot be known statically, degradation to `unknown` may be appropriate.

If resolving `T` is required to prove that a call is valid, the checker must not silently treat `unknown` as successful generic inference.

### Task 17 — Exercise concurrency as the integration test

Use the real channel/task APIs as a principal end-to-end proof.

Code conceptually equivalent to:

```slug id="i96c9f"
val inbox = chan<File>()

send(inbox, file)

val received = recv(inbox)
```

should retain:

```text id="dhzgwc"
inbox    : chan<File>
received : File|nil
```

and invalid cross-type sends must fail statically.

The standard receive signature:

```slug id="dt8kpf"
fn recv<T>(channel:chan<T>):T|nil
```

also validates the non-nil generic rule.

Because `T` cannot itself contain `nil`, the returned `nil` remains unambiguously the channel-closed sentinel rather than a value transmitted through the channel.

Likewise:

```text id="tjqyvn"
task<num> -> await(...) -> num
```

must preserve the task result type.

### Task 18 — Exercise generic collections containing nominal resources

Add integration coverage such as:

```text id="9d0nmn"
list<File>
map<str,File>
chan<File>
task<File>
```

and prove the nominal resource identity survives every generic layer and module boundary.

This should build directly on the nominal identity guarantees established in Phase 4.

### Task 19 — Improve generic mismatch diagnostics

Generic failures should explain the relationship that failed rather than collapsing into a vague overload failure where possible.

For example:

```text id="r36j6s"
T inferred as str from argument 1,
but argument 2 has type num
```

or equivalent structured information.

Failures involving explicit nullability should also make the distinction clear where useful. For example, attempting to pass `list<str|nil>` to `list<T>` should identify the nullable element type rather than presenting it as an unresolved generic.

`--diagnostic-format=json` should expose the same underlying diagnostic cleanly.

### Task 20 — Keep the scope intentionally small

Explicitly exclude from this phase:

* trait or interface bounds;
* type classes;
* generic constraints;
* variance annotations;
* higher-kinded types;
* specialization;
* generic structs or enums unless independently required;
* compile-time generic metaprogramming;
* monomorphization as a performance feature.

This phase is complete when ordinary generic library functions preserve information predictably through calls, modules, collections, channels, tasks, higher-order functions, variadics, and spreads without inventing types merely to make calls succeed.
