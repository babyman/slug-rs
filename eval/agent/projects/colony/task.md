# Colony on many cores

The workspace holds `./colony.slug`, a working module that simulates ants
foraging on a grid. Read it first: its opening comment describes the
simulation, and each export has a doc comment. It runs on one core. Make it
use several: `tick(forks)` must share the work of each tick among `forks`
forks, so that a tick of a large colony takes much less time on four forks
than on one.

The module will be graded by a hidden suite of programs that import it with
`val colony = import("colony")` and call its exports from a function, such as
`main`. Keep the five exports, `start`, `tick`, `run`, `snapshot`, and
`observe`, with the meanings their doc comments give.

## What must not change

The simulation itself. For any world, any sequence of `tick` and `run` calls,
and any number of forks in each, `snapshot()` and `observe(id)` must return
exactly what the module in the workspace returns now for the same calls: the
same rows, the same ants in the same order with the same fields, the same
`store`, `turn`, and every `stats` count, including `stats.looks`. Results
must not depend on how many forks a tick uses, or on how the forks are
scheduled.

The colony is one colony, shared by every task in the program: a tick run in
a task started with `spawn` changes what `snapshot()` returns in every other
task afterwards, as it does now.

## `forks`

`tick(forks)` and `run(ticks, forks)` take `forks`, an integer from 1, with a
default of 1. `tick(1)` may make every decision itself. With more than one,
the work of deciding where the ants go is shared among `forks` forks. `forks`
may be larger than the number of ants.

Any other `forks`, such as `0`, `-2`, or `2.5`, throws an `Error` (from
`slug.builtin`) with `type: "colony.forks"`, any `msg`, and `data:
{forks: f}`, where `f` is the value given, before the tick changes anything.
`run` checks its `forks` the same way before it runs any tick.

## Speed

One group of hidden cases times a colony of about 1200 ants on a 160 by 80
grid over 10 ticks, run in one program with `run(10, 1)` and in another with
`run(10, 4)`, and checks that the second takes at most 0.6 times as long as
the first, start-up included. Grading runs on a machine with at least four
cores. Ticks of small colonies need not be faster with forks.

## Notes

- Read the language specification's sections on tasks and forks
  (`slug-docs/language-specification.md`) before you start.
- Work in `./colony.slug`. You may add other files to the workspace for your
  own testing; only `colony.slug` is graded.
