# Tests are written and run in Slug

## Status

Accepted

## Context

The core clutch's behaviour was tested from Rust, by running Slug source
through the CLI and comparing its output, and `@testWith` examples in
`slug.std` were never run. Writing the JSON library's tests that way would be
slow to read and write. `describe`, `slug.fmt.toml`, and clutch `[tests]`
declarations now give a Slug program what it needs to find and run tests.

## Decision

- `slug.test`, in the core clutch, holds the tools tests use: `assertEqual`,
  `assertNotEqual`, `assertTrue`, `assertFalse`, `assertNear`,
  `assertThrows`, and `fail`. A failed assertion throws an `Error` of type
  `test.failure` with `{expected, actual}` as data. It also holds what a
  runner needs: `attempt`, `examples` for one `@testWith` tag, `check` for
  one example, and `runTest` for one `@test` function, each returning a
  failure map instead of throwing.
- A function tagged `@test` takes no arguments and passes when it returns
  without throwing and does not return `false`. `@testWith` holds examples in
  pairs, an argument list and the expected result, as `slug.std` already
  wrote them, as many as one tag holds.
- The runner is `lib/test.slug`, run as an installed tool:
  `slug --allow-unexported test ./path/to/name.clutch ...`. For each clutch,
  it reads `clutch.toml`, describes every module the clutch provides and every
  test module it declares, unexported declarations included, and runs their
  `@test` and `@testWith` declarations. It prints each failure with its
  arguments and expected and actual values, or the error thrown, then a
  summary per clutch.
- Slug has no way to set an exit status, so the runner throws a short message
  when any test failed, which exits nonzero.

## Consequences

### Positive

- Tests are Slug, beside the modules they check, and `@testWith` examples
  document and test a function at once.

### Negative

- A failing run ends with a runtime-error diagnostic pointing into the runner,
  after its own report.
- Only installed clutches can be tested; directory and file targets come
  later.

## Migration

None.
