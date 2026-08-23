# Keep callable signatures in private bytecode metadata

## Context

Slug calls bind positional, named, defaulted, and variadic arguments. The
initial VM exposes only a fixed arity on each chunk, so it cannot validate or
bind those source-level forms while retaining source-located runtime errors.
The project must not turn its mutable internal bytecode into a portable
compatibility contract.

## Decision

Represent a callable's parameter names, default-presence, and final-variadic
flag as private `Chunk` metadata. Compile call sites to private argument-mode
metadata so the VM can expand spreads and perform a single binding operation
before a frame begins. Use the same binder for ordinary calls and `recur`.

Default expressions run in a function prologue only when binding omitted the
parameter. Their bytecode therefore executes in the callee closure's captured
and module environment, never in the caller's lexical environment.

## Consequences

- Direct bytecode tests may construct signature metadata as the VM boundary
  evolves.
- Argument binding remains checked and retains the call instruction span.
- The private metadata is not serialized and does not define `.cslug`.

## Migration

Existing fixed-arity chunks retain positional behavior. No Slug source
migration is required.
