# Conformance Fixtures

Each portable conformance fixture consists of a Slug source file and an adjacent
TOML sidecar named `<source-stem>.fixture.toml`. For example,
`arithmetic.slug` is described by `arithmetic.fixture.toml`.

The sidecar is versioned data, not Slug source. It makes the expected host
boundary explicit without embedding one implementation's test framework in the
fixture.

```toml
schema = 1
outcome = "success"
stdout = "42\n"
stderr = ""
module_root = "."
library_root = "lib"
timeout_ms = 1000
```

## Required fields

- `schema` is the integer `1`.
- `outcome` is one of `success`, `parse-error`, `semantic-error`, or
  `runtime-error`.

## Optional fields

- `stdout` and `stderr` are exact expected streams when present.
- `module_root` and `library_root` are paths relative to the fixture sidecar.
  They must not be absolute or escape the fixture directory.
- `timeout_ms` is a positive integer execution limit.
- `diagnostic` is an exact expected diagnostic string. It is valid only for a
  non-success outcome.

Absent stream fields leave that stream unchecked. An absent timeout leaves the
runner's default limit in effect. A runner must reject a missing sidecar,
unsupported schema, unknown field, malformed value, or invalid field
combination rather than guessing an expectation.

Fixtures may place imported source and configuration files below their declared
roots. The runner supplies no ambient project, library, environment, or command
line state unless a later metadata schema explicitly adds it.

Run a fixture directory with:

```sh
slug-fixtures path/to/fixtures --slug path/to/slug
```
