# Validate C prototype members by opaque key

## Context

The first C math module dispatched callbacks by their arity. That was enough
for `add` and `sqrt`, but it could not represent two distinct C functions with
the same call shape. Its C-facing status was also a Rust enum, and strings were
NUL-terminated pointers despite the native ABI direction requiring fixed-width
layouts and explicit byte lengths.

## Decision

The feature-gated C prototype uses a fixed-width, length-delimited descriptor
layout. Every function declares an opaque UTF-8 member key. The Rust facade
retains that key on the native function and exposes it only during the callback;
the C bridge selects its descriptor by that key rather than by arity.

C callbacks return an `int32_t` status. The bridge accepts only `0` and `1`;
any other value is a checked native-contract violation. Module registration is
batched and preflighted, so a conflicting descriptor cannot partially register
a module.

## Consequences

- Same-arity C functions now dispatch independently without using Slug source
  annotations as ABI metadata.
- Descriptor and error text are copied into owned Rust strings at the unsafe
  boundary.
- The prototype remains exact-arity-only and does not yet bind multiple source
  `foreign` overloads with the same local name.
- ABI v1 still needs a durable source-to-member-key binding rule, module state,
  lifecycle callbacks, and a cross-platform loader.

## Migration

The prototype header is unstable. Its C fixtures now require descriptor sizes,
member keys, length-delimited text, and integer callback statuses.
