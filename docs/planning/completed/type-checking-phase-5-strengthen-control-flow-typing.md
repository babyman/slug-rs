### Phase 5 — Strengthen control-flow typing

Slug already performs direct branch-local nil narrowing and uses pattern constraints while checking `match`.

This phase should turn that existing machinery into modest flow-sensitive analysis.

The goal is deliberately limited:

> Track simple type facts through direct branches, guards, match alternatives, and terminating control flow.

Do not build a general theorem prover or full control-flow graph.

#### Task 1 — Represent whether an expression can continue

Extend expression checking so it can distinguish:

```text
falls through
terminates current control flow
```

Terminating expressions include at least:

```slug
return ...
throw ...
recur(...)
```

Avoid inferring reachability solely from the expression's ordinary value type. Flow/reachability should be represented explicitly enough that branch merging can use it safely.

#### Task 2 — Make `return` terminating for flow analysis

`throw` and `recur` already produce `Never`; `return` currently does not carry equivalent reachability information.

Ensure:

```slug
if value == nil {
    return
}
```

marks that branch as unable to continue.

Preserve existing function return-type inference and validation while adding this control-flow information.

#### Task 3 — Preserve facts from a surviving `if` branch

Given:

```slug
fn foo(value:str|nil) {
    if value == nil {
        return
    }

    println(value)
}
```

infer after the `if`:

```text
value : str
```

Likewise:

```slug
if value != nil {
    use(value)
} else {
    return
}

use(value)
```

must retain the non-nil fact after the branch.

When both branches continue, continue using the normal merged environment.

#### Task 4 — Handle both terminating branches correctly

For:

```slug
if condition {
    return
} else {
    throw "failed"
}

unreachable()
```

the `if` itself must be considered terminating.

Do not merge bindings from unreachable continuations into the enclosing environment.

#### Task 5 — Preserve existing direct nil narrowing

Lock down the behavior that already exists:

```slug
if value != nil {
    // value : T
} else {
    // value : nil
}
```

and the inverse for `== nil`.

Cover both operand orders:

```slug
value != nil
nil != value
value == nil
nil == value
```

#### Task 6 — Preserve short-circuit narrowing

Add regression coverage for the existing `&&` and `||` behavior:

```slug
value != nil && use(value)
value == nil || use(value)
```

The right-hand expression should receive the facts implied by evaluating it.

Nested simple conditions should compose where the existing fact model can do so without introducing speculative reasoning.

#### Task 7 — Generalize flow facts beyond the name “nil facts”

Refactor the existing `nil_condition_facts` / fact application machinery into a small general flow-fact abstraction.

Nil narrowing may remain the only equality-derived fact initially, but the representation should be capable of carrying a narrowed `Type` rather than baking `nil` into the control-flow architecture.

This prepares the same mechanism for `match` without introducing separate narrowing systems.

#### Task 8 — Narrow named match subjects within cases

For a named subject:

```slug
match value {
    str s {
        // s : str
        // value : str
    }
    num n {
        // n : num
        // value : num
    }
    nil {
        // value : nil
    }
}
```

continue typing pattern bindings as today, but also narrow the original subject binding within the corresponding case when the subject is a simple name.

Do not attempt arbitrary expression alias tracking.

#### Task 9 — Carry surviving `match` facts forward

For a closed union:

```slug
match value {
    nil {
        return
    }
    str s {
        use(s)
    }
}
```

if only one possible subject type can reach the continuation after the `match`, preserve that fact.

More generally, post-match subject type should be the union of alternatives from cases that can fall through.

If no case can fall through, the whole `match` is terminating.

#### Task 10 — Respect guards conservatively

For guarded cases:

```slug
match value {
    str s if predicate(s) {
        ...
    }
}
```

narrow within the case according to the pattern, but do not treat a guarded pattern as having eliminated that type from later cases or from post-match flow unless that is provably true.

Existing coverage behavior should remain conservative around guards.

#### Task 11 — Make branch merging reachability-aware

Update environment merging so:

- two continuing branches merge normally;
- one continuing branch and one terminating branch use the continuing environment;
- two terminating branches produce no continuation;
- branch-local declarations do not escape their scope;
- mutation of existing bindings continues to merge according to existing type rules.

This should become the central rule rather than adding special cases for guard clauses.

#### Task 12 — Add nested-flow regression tests

Cover modest combinations such as:

```slug
if value != nil {
    if ready {
        use(value)
    }
}
```

and:

```slug
if value == nil {
    return
}

match value {
    str s { ... }
    num n { ... }
}
```

Facts should survive nesting when logically justified and disappear when branches merge ambiguously.

#### Task 13 — Keep nominal identity intact during narrowing

Flow narrowing must operate on actual `Type` values and preserve the nominal identity work completed in Phase 4.

For example:

```text
File|Socket|nil
```

narrowed by control flow must retain the original `File` and `Socket` identities rather than reconstructing types from display names.

#### Task 14 — Add structured diagnostic regression coverage

Ensure errors discovered after narrowing report the narrowed actual type.

For example, inside:

```slug
if value != nil {
    needsNum(value)
}
```

a `str|nil` input should report the actual type as `str`, not `str|nil`.

The same result should appear through `--diagnostic-format=json`.

### Explicitly out of scope

Do not add:

- arbitrary predicate-based narrowing;
- relational reasoning;
- alias analysis;
- mutation-sensitive SSA;
- loop fixed-point analysis;
- sophisticated boolean algebra;
- user-defined type guards.

Direct branches, short-circuit conditions, guards, match alternatives, and terminating control flow are sufficient for this phase.