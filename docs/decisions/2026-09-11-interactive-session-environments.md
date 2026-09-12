# Retain interactive compiler snapshots and runtime environments

## Context

Interactive source submissions must resolve bindings declared by earlier
submissions without replaying prior source. Their runtime values and closures
must also survive, while parser and semantic failures leave the prior session
usable.

## Decision

Each interactive session retains a compiler state containing a root semantic
snapshot plus mutable and callable global metadata. A later submission seeds
semantic analysis and bytecode compilation from that state. The state is
replaced only after its submission executes successfully.

Each session also owns one reference-stable VM global environment. The shared
VM temporarily runs a submission in that environment and restores its host
global environment afterward. Closures created during a submission retain the
session environment directly.

## Consequences

Ordinary bindings and closures persist without source replay, and parser,
semantic, and compiler failures cannot alter the retained compiler state.
Runtime mutations are not transactional: a runtime failure may have changed
the session environment, while its compiler state remains the last successful
snapshot. Rollback remains deferred work.

The initial per-session environment starts with cloned host bindings. Parent
layering and deliberately shared bindings remain a multi-session milestone.

## Migration

None.
