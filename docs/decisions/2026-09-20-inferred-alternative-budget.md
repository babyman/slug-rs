# Cap inferred overload alternatives

## Context

Finite inferred overload alternatives preserve correlations that ordinary type
unions cannot represent. Nil-partitioned branches may add alternatives to an
operator family's initial set, and future composition must not make compiler
time or memory depend without bound on source control flow.

## Decision

Each callable retains at most 16 private, canonical inferred alternatives.
Construction canonicalizes and deduplicates the entire current set before the
limit is assessed. If any intermediate construction step would produce a
seventeenth alternative, the compiler discards that callable's entire inferred
alternative set; it does not retain a source-order-dependent first 16.

Discarding alternatives widens only that private metadata to ordinary
structural/dynamic callable behavior. Independently proven annotations and
singleton body facts remain available. Widening produces no source diagnostic:
calls that would have depended on discarded alternatives remain valid and use
the VM's existing checked runtime behavior. Future observability, if needed,
belongs in non-semantic metrics or an explicit inference trace.

## Consequences

Nil partitions can extend the five `+` schemes through sixteen canonical
alternatives. Larger compositions remain bounded and deterministic, without
turning an exhausted precision budget into a source error. Tests must cover
the exact limit, overflow widening, guard-order independence, and retention of
unrelated singleton facts.

## Migration

Source calls whose function bodies exceed this private budget may no longer
receive static inferred-alternative rejection. They remain valid and retain
checked runtime failures for incompatible runtime operands.
