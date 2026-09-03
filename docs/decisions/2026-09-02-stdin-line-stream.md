# Standard input is one bounded line stream

## Context

Slug channels already provide the explicit, closing asynchronous boundary used
by source programs. A special mutable stdin handle would add a second I/O
model, while terminal prompts still need a small convenient API.

## Decision

`slug.io.stdin.readLines()` returns one shared channel of newline-normalized
strings for process standard input. Empty and final unterminated lines are
preserved; EOF and host read failure close the channel. Multiple calls return
the same single-consumer stream rather than broadcast copies.

`readLine`, `prompt`, and `confirm` are source-library helpers layered on that
stream. `readLine` returns `str|nil`; `confirm` returns its default for empty,
unrecognized, or end-of-input input.

The host reader uses one bounded native-producer channel. When it is full, the
reader applies backpressure until it can publish or the receiver is closed.

## Consequences

Programs can compose stdin with channel receives and `select` without a
second I/O abstraction. Input is not replayable or broadcast, and an
unresponsive consumer can delay further host reads. The standard-library
source, native registration, tests, and runtime requirements must preserve the
line and closure contract.

## Migration

None.
