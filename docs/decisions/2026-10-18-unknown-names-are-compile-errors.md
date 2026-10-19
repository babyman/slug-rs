# Unknown names are compile errors

## Status

Accepted

## Context

A name that was neither local nor declared at a module's top level compiled to
a global lookup, because the runtime installs names the compiler did not
track: the host's builtins and the VM's own functions. A misspelled name, or
one an agent guessed, such as `type(a)` for a type test, compiled and failed
only when it ran, with `unknown name`, and only on a path that reached it.

The compiler already resolves the implicit `slug.builtin` module for type
checking, and that module declares every builtin the host provides. Across 209
Slug files, the only name the compiler did not resolve was `Error`, which
`slug.builtin` exports.

## Decision

When the compiler has the implicit `slug.builtin` module, a name that is not
declared, not exported by that module, and not one of the VM functions `cfg`,
`int`, and `stacktrace` is a static `unknown_name` error. Names a `{*}`
declaration selects count as declared when the checker knows the selected
map's entries, as for an import. The error suggests up to three names in
scope within a small edit distance, using the module-name matcher, and offers
each as a replacement edit:

```text
unknown name `pirntln`; did you mean `println`?
```

The name is still looked up when it runs when the compiler cannot know every
name the runtime provides: without `slug.builtin`, as when an embedding
compiles source directly; after a `{*}` over a map whose entries are known
only when it runs; and in interactive sessions, whose host may register names
at any time.

## Consequences

### Positive

- A misspelled or invented name is reported before anything runs, at its use,
  with the likely correction.

### Negative

- The VM's function names are listed in the compiler as well as the VM.

### Neutral

- Every file in the corpus that compiled before still compiles.

## Migration

None for programs that ran: a name now reported was one that failed whenever
its use was reached.
