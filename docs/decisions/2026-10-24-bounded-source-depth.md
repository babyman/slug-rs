# Bound source depth and grow compiler stacks

## Status

Accepted

## Context

Every compiler pass recurses once per level of a source tree. The parser
limited bracketed nesting, such as calls, lists, blocks, and functions, to
256 levels, but other source built deeper trees without nesting:

- operator chains, `1 + 1 + ...` and `nil ?? nil ?? ...`, add a level per
  operator, and the parser itself recursed once per `??` or `+:`;
- `else if` chains add a level per branch, also through parser recursion;
- `spawn spawn ... x` nested tasks, and their `task<...>` types, uncounted;
- type annotations, `list<list<...>>`, nested without limit.

A release build used about 8 KiB of stack per level and a debug build about
80 KiB, so `slug report` and `slug run` aborted with a stack overflow on a
1,052-term sum on the 8 MiB main thread, and a debug build overflowed a 2 MiB
thread, the default for spawned and test threads, at 21 nested calls: below
the nesting limit itself.

## Decision

- Every recursive pass over source trees, and cloning an expression, runs
  each recursive step through `stacker::maybe_grow`, which continues on a new
  8 MiB heap-allocated stack segment when less than 512 KiB of the current
  one remains. Compiling no longer depends on the calling thread's stack.
  Recursion over semantic types, such as `task<task<...>>`, stays on the
  thread's stack: type depth follows bracketed nesting, which is bounded.
- Bracketed nesting stays limited to 256 levels. Type annotations and
  `spawn` without braces now count toward it.
- An expression tree may be at most 4,096 levels deep, counting every nested
  expression, pattern level, default, tag argument, and computed pattern key.
  The parser enforces this as it builds each tree, and limits its own
  recursion through expressions and `else if`, `??`, and `+:` chains to the
  same depth, so no deeper tree is built and none is dropped. Dropping a
  tree recurses without growing the stack; a 2 MiB debug-build thread drops
  trees over 10,000 levels deep.
- Both limits report `nesting_limit_exceeded`.

## Consequences

Source within the limits compiles on any thread; tests compile every kind of
chain at 4,080 levels and every kind of nesting at 256 levels on 2 MiB
threads in debug builds. Hostile source fails with a diagnostic, and a
200,000-link chain is rejected after its first 4,096 links rather than
parsed through.

`slug-frontend` depends on `stacker`, which builds a small assembly file
through `cc` for each target. Recursive passes added later must call
`stack::grow`, and the tests above should gain their shapes.

## Migration

None for programs within the limits. `else if` and operator chains longer
than about 4,000 links, and type annotations or brace-less `spawn` nested
past 256 levels, are now compile errors; before, the longer of them crashed
the compiler.
