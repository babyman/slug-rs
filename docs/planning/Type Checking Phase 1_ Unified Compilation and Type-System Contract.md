# Type Checking Phase 1: Unified Compilation and Type-System Contract

## Status and purpose

This document defines the first phase of Slug's type-system development.

Phase 1 removes optional type checking and establishes a single semantic-analysis path for every Slug program. It also
defines the contract that future type-system work must preserve.

This phase is primarily architectural. It does **not** attempt to make Slug fully statically typed, substantially
increase inference, introduce machine-level numeric types, specialize bytecode, or change runtime value representation.

The immediate goal is simpler:

> **Type checking is part of compiling Slug. It is not an optional compiler mode.**

Once this is true, later improvements can strengthen what the compiler knows without repeatedly changing the language's
fundamental execution model.

---

## Motivation

Slug increasingly depends on semantic type information for more than diagnostics.

The compiler now reasons about concepts including:

- function signatures and callable selection;
- structs and nominal identities;
- enums;
- transparent type aliases;
- nominal resource types;
- unions and nil;
- generic collections;
- channels and tasks;
- imported type information;
- foreign-function contracts.

This semantic information will also become important to two major areas of future development:

1. **FFI lowering**, where concrete native representations and nominal resource identities matter;
2. **VM optimization**, where proven type information can allow specialized bytecode and cheaper execution.

Maintaining separate checked and unchecked compilation paths therefore works against the direction of the
implementation.

Slug should have one compiler and one semantic model.

---

# Decision

All source compilation performs semantic type checking.

The `-type-check` command-line option is removed.

Any public or internal compiler APIs whose purpose is to select between checked and unchecked compilation are removed or
consolidated.

The compilation pipeline becomes conceptually:

```text
source
  ↓
lex
  ↓
parse
  ↓
semantic analysis
  ├─ name resolution
  ├─ type resolution
  ├─ type inference
  ├─ callable resolution
  ├─ narrowing
  └─ provable type validation
  ↓
compile
  ↓
bytecode
```

There is no alternate unchecked source-compilation path.

The VM remains responsible for validating conditions that cannot be proven statically.

---

# Type-system contract

Slug is not required to determine a concrete static type for every expression.

Instead, the compiler follows this rule:

> **A type error is a compile error when the compiler has sufficient information to prove that the program is invalid.
Genuine uncertainty remains valid and is handled safely at runtime.**

This distinction is fundamental to Slug's type system.

It preserves Slug's lightweight programming model while allowing semantic analysis to become increasingly powerful.

---

## Known incompatibility

When both sides of an operation or assignment are sufficiently known and incompatible, compilation fails.

For example:

```slug
val value = "hello"
value + 10
```

If the compiler knows `value` is a string and numeric addition cannot accept it, this is a compile-time error.

Likewise:

```slug
resource File
resource Socket

fn read(file:File) {
    ...
}

val socket:Socket = ...
read(socket)
```

is invalid because `Socket` and `File` are distinct nominal types.

The runtime should not be asked to rediscover an incompatibility already proven by semantic analysis.

---

## Genuine uncertainty

Lack of sufficient static information is not itself an error.

For example:

```slug
fn increment(value:any) {
    value + 1
}
```

The compiler may be unable to prove that every possible `value` supports this operation.

That uncertainty does not automatically make the function invalid.

If execution reaches the operation with an incompatible value, the VM reports a normal Slug runtime error.

Therefore:

```text
known invalid
    → compile error

known valid
    → compile normally

genuinely unknown
    → compile with runtime enforcement where required
```

Future type-system work should reduce the third category through better inference and narrowing, not eliminate it by
requiring annotations everywhere.

---

# Explicit types are contracts

A programmer-provided type annotation is an explicit contract and must always be enforced.

For example:

```slug
fn double(value:num):num {
    value * 2
}
```

The compiler may rely on `value:num` when checking the function body.

Callers must provide something assignable to `num`, either provably at compile time or through whatever checked dynamic
path Slug permits.

Likewise:

```slug
val name:str = value
```

must reject `value` if semantic analysis proves it cannot be a string.

Annotations are not hints.

---

# Inferred types are equally authoritative

A type inferred by the compiler is not weaker than an explicitly written type.

For example:

```slug
val name = "Slug"
```

allows the compiler to treat `name` as `str` wherever that fact remains valid.

The programmer should not need to write:

```slug
val name:str = "Slug"
```

merely to enable checking or optimization.

This establishes an important design principle:

> **Slug's type system describes what the compiler knows, not what the programmer is required to write.**

Explicit annotations describe useful contracts. Inference supplies the remainder whenever possible.

---

# `any`, `unknown`, and dynamic values

The implementation must maintain a clear distinction between internal lack of knowledge and the source-level meaning of
`any`.

These concepts serve different purposes.

## `any`

`any` is an intentional source-level type.

A programmer declaring:

```slug
fn consume(value:any) {
    ...
}
```

is explicitly allowing values whose more specific type is not part of that function's contract.

`any` therefore represents deliberate dynamism.

It must retain the language's existing nil semantics; if `any` excludes nil, then `any|nil` remains the form that
accepts nil.

## Unknown compiler knowledge

An internal unknown type or incomplete type fact means that semantic analysis has not yet established enough information
about an expression.

It is a property of compiler knowledge, not a type programmers should generally reason about.

Unknown information must not be treated as proof of compatibility.

It must also not automatically be treated as proof of incompatibility.

Future inference improvements should progressively replace unknown information with stronger facts.

---

# Nil and unions

Union types represent genuine alternatives.

For example:

```slug
str|nil
```

means that both cases are valid possibilities.

Control-flow analysis may narrow that union when sufficient evidence exists:

```slug
fn printName(name:str|nil) {
    if name != nil {
        println(name)
    }
}
```

Within the branch, the compiler may treat `name` as `str`.

This principle should eventually extend to guards, `match`, enums, and other control-flow constructs, but Phase 1 does
not require new narrowing behaviour.

Existing narrowing behaviour should continue to work unchanged.

---

# Nominal and structural typing

Slug contains both structural and nominal type concepts.

The checker must preserve that distinction.

## Nominal types

Types whose identity is part of their meaning are compatible according to identity rather than representation.

Current examples include:

```text
struct/schema identities
enum types
resource types
```

Two resources with identical runtime representation are not interchangeable if their declared resource types differ.

Two enum cases with identical names are not interchangeable if they belong to different enum types.

## Transparent aliases

A type alias does not introduce a new nominal identity.

For example:

```slug
type Path = str
```

means that `Path` and `str` are assignable as the same underlying type.

Aliases improve naming and API expression without introducing conversion ceremony.

Future strong typedefs, if Slug ever needs them, should be introduced as a separate language concept rather than
changing alias semantics.

---

# Compile-time and runtime enforcement

Static checking and runtime checking are complementary.

Moving checking into normal compilation does **not** mean removing runtime validation.

Some program paths will remain dynamic.

This is especially important at boundaries such as:

- `any`;
- dynamically selected callables;
- imported or dynamically obtained values;
- foreign functions;
- native resources;
- future machine-level numeric conversion;
- data originating outside the Slug program.

The rule is:

> **Compile-time proof may remove the need for a runtime decision, but lack of proof must not remove runtime safety.**

This creates a path toward future optimization.

A generic operation may initially retain runtime checks:

```text
Add
```

while a future compiler may emit:

```text
AddInt
```

when semantic analysis proves the required invariant.

The specialized operation is valid because of a compiler proof, not because the VM guesses.

---

# Phase 1 implementation

Phase 1 consolidates existing compilation behaviour without intentionally adding new type-system rules.

## Remove the command-line mode

Remove the `-type-check` option.

Commands that compile Slug source always perform semantic type checking.

Documentation, usage output, tests, and examples referring to the option must be updated.

---

## Remove checked-versus-unchecked compiler APIs

Remove APIs whose only purpose is choosing whether semantic type checking occurs.

Where the source/compiler layer currently exposes separate operations equivalent to:

```text
compile(...)
compile_type_checked(...)
```

replace them with one normal compilation entry point.

Likewise, remove `type_check` booleans passed through module-loading or compilation APIs.

Callers should not decide whether Slug semantics are enforced.

---

## Consolidate semantic analysis

The source front end should have one conceptual semantic-analysis operation.

Its responsibility includes all semantic knowledge required before bytecode generation.

The long-term shape should be approximately:

```text
parse(source)
    ↓
analyze(ast, imports)
    ↓
SemanticAnalysis
    ↓
compile(ast, SemanticAnalysis)
```

Phase 1 does not require a large refactor solely to achieve this exact API.

However, new work should move toward one semantic-analysis result rather than maintaining parallel validation and
checking pipelines.

If existing validation and type-checking passes currently contain different behaviour, Phase 1 should merge them
conservatively.

---

# Compatibility requirement

Phase 1 should not deliberately increase the set of rejected programs beyond what the existing type-checked compilation
mode already rejects.

In other words:

> **The previous `-type-check` behaviour becomes normal Slug behaviour.**

This gives the project a stable baseline before strengthening the checker.

Programs that previously relied on the unchecked mode and contain errors already diagnosed by `-type-check` will now
fail compilation. This is the intentional compatibility change introduced by this phase.

No additional incompatibility should be introduced accidentally while removing the mode split.

---

# Diagnostics

Type errors must remain source-level Slug diagnostics.

Removing optional checking must not make errors:

- Rust panics;
- VM validation failures;
- misleading bytecode errors;
- internal compiler failures.

Diagnostics should identify the relevant source location and describe the incompatible types or contract where possible.

The compiler should prefer language concepts over implementation terminology.

For example:

```text
expected File, found Socket
```

is preferable to an error describing internal resource identifiers.

---

# Tests

Phase 1 should establish tests for the unified compiler behaviour.

At minimum, cover:

### Existing valid programs

Programs that currently compile successfully with type checking enabled continue to compile and execute normally.

### Existing type errors

Programs rejected by the current checked mode are rejected during ordinary compilation without any command-line option.

### Dynamic programs

Programs containing genuinely dynamic values continue to compile where the compiler cannot prove an error.

Runtime checking remains responsible for invalid dynamic execution.

### Explicit annotations

Known violations of parameter, return, binding, resource, enum, collection, and other existing annotations fail
compilation.

### Imports

Imported semantic type information behaves identically under the unified compiler path.

### CLI behaviour

`-type-check` is no longer accepted or documented.

Normal invocation performs checking automatically.

---

# Documentation changes

Update public and engineering documentation so that type checking is described as part of compilation rather than an
optional feature.

Avoid terminology such as:

```text
type-checked mode
strict mode
checked compilation
```

unless Slug later introduces an intentionally distinct language mode.

Documentation should instead describe:

- what the compiler can currently prove;
- what remains dynamically checked;
- known limitations of inference;
- supported type-system features.

The implementation-support ledger should track those capabilities individually rather than treating type checking as an
all-or-nothing feature.

---

# Non-goals

Phase 1 does not attempt to:

- make every expression statically typed;
- reject every use of `any`;
- reject programs merely because some type information is unknown;
- require new annotations;
- introduce `i32`, `u32`, `f32`, or other machine numeric types;
- change `num`;
- introduce numeric range analysis;
- redesign generic inference;
- substantially extend control-flow narrowing;
- specialize bytecode;
- remove runtime type checks;
- change `Value` representation;
- alter the native ABI solely for type-system purposes.

Those are later phases built on this foundation.

---

# Acceptance criteria

Phase 1 is complete when:

1. `-type-check` no longer exists.
2. There is no public compiler or module-loader switch controlling whether type checking occurs.
3. Every source compilation performs the existing strongest semantic checking.
4. Existing programs accepted by the previous checked mode remain accepted.
5. Errors previously detected only with `-type-check` are now ordinary compile errors.
6. Genuinely dynamic programs remain supported.
7. Runtime validation remains intact for cases not statically proven.
8. Imports and exported type information use the same unified checking path.
9. CLI and language documentation describe type checking as normal compiler behaviour.
10. The full test and conformance suites pass.

---

# Direction after Phase 1

Phase 1 deliberately establishes policy before sophistication.

Subsequent work can strengthen semantic analysis incrementally:

```text
unified compiler
      ↓
better expression inference
      ↓
stronger nominal typing
      ↓
flow-sensitive narrowing
      ↓
generic propagation
      ↓
numeric facts beneath num
      ↓
precise FFI lowering
      ↓
typed bytecode specialization
      ↓
representation-aware VM optimization
```

Each stage should follow the same contract:

> **Use static knowledge when it exists. Preserve safe dynamic behaviour when it does not.**

The long-term goal is not to maximize the amount of type syntax in Slug.

The goal is to maximize useful compiler knowledge while keeping Slug simple to write.

A successful type system should therefore improve correctness, FFI clarity, and VM performance without requiring
programmers to constantly explain information the compiler can determine for itself.
