# Represent VM faults as error maps in `defer onerror`

## Context

`defer onerror(err)` must receive a Slug value for both explicit throws and VM
faults. Throwing must preserve arbitrary user values, while VM faults need a
stable representation that handlers can inspect without parsing diagnostics.

## Decision

An explicit `throw value` passes `value` unchanged to an error handler. A
checked VM fault passes a string-keyed map with `type`, `msg`, and `data`.
`type` is the fault class, `msg` is the human-readable message, and `data` is
`nil` unless the fault later defines structured information.

## Consequences

- Error handlers can classify VM faults using stable field names.
- CLI diagnostic wording remains independent from handler behavior.
- The distinction between user-thrown values and VM faults is intentional.

## Migration

None. `defer onerror` is not yet implemented.
