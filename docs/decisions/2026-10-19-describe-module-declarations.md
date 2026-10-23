# Describe a module's declarations

## Status

Accepted

## Context

A test runner written in Slug needs to find a module's `@test` and
`@testWith` declarations and call them, and agents and documentation tools
benefit from the same metadata. Compiled programs already retain each
top-level declaration's name, mutability, export status, documentation, and
tags, and a private VM instruction evaluates tag arguments once while the
module initializes ([2026-08-23](2026-08-23-module-declaration-metadata.md)).
Nothing exposed it to Slug code; the planned `slug.meta` module was never
built.

The useful `@testWith` cases are often on unexported helpers, but returning
unexported declarations, values included, to any program would make
`export` a convention: a module's raw natives, such as `slug.string`'s
`ordRaw`, are unexported because their wrappers validate what they assume.

## Decision

`describe(module, unexported = false)` is a VM builtin. It initializes the
named module, as `import` does, and returns its top-level declarations in
source order, one map per binding: `name`, `exported`, `mutable`,
`documentation` without its comment markers, `tags` as `{name, arguments}`
with the evaluated arguments, and `value`.

- It describes modules only. Describing bindings or the current scope would
  need local names and tags in compiled programs, which nothing yet needs and
  embedded targets would pay for.
- It takes a module name rather than an `import` result, which is an ordinary
  map with no link back to its module.
- Module instances retain every top-level binding's value at the end of
  initialization, so unexported values are available.
- `unexported = true` requires the `describe.unexported` permission, which
  only the user running slug grants, with `--allow-unexported`. A program or
  its configuration cannot grant it; without it the call is a `permission`
  runtime error. This is the first permission a host grants rather than a
  capability it supplies, so hosts gain `VmHost::permits`, false by default.
- Builtins accept named arguments, mapped to their parameter names.

## Consequences

### Positive

- A test runner, documentation tool, or agent can read `@testWith` examples
  and call the functions they describe, from Slug.
- Encapsulation holds for every program not run with `--allow-unexported`.

### Negative

- Hosts without module metadata, including compiled `.cslug` artifacts, cannot
  describe modules yet.
- A program describing its own entry module would initialize a second copy,
  because the entry is not a module graph instance.

### Neutral

- `slug.meta` is not planned any more.

## Migration

None. A program that defines its own `describe` shadows the builtin.
