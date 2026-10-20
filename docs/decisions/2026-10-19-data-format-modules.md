# Data format modules are `slug.fmt.*`

## Status

Accepted

## Context

Slug needs readers and writers for data formats: TOML for clutch manifests
and configuration, read by the Slug test runner, and JSON, whose library the
agent evaluations produced. They need one naming scheme, and `fmt` elsewhere
often means printf-style formatting, as in Go and Rust.

## Decision

Modules that read or write a data format are named `slug.fmt.<format>`:
`slug.fmt.toml` now, `slug.fmt.json` next. The prefix is reserved for data
formats; Slug's string interpolation covers formatting values, so no
`slug.fmt` module formats text. Each reports invalid input as an `Error` whose
`type` is `<format>.parse`.

`slug.fmt.toml` lives in the core clutch and reads the part of TOML 1.0 that
configuration and clutch manifests use: comments, bare, quoted, and dotted
keys, `[table]` headers, basic and literal strings, decimal integers,
booleans, arrays, and inline tables. Floats, dates and times, multi-line
strings, non-decimal integers, and arrays of tables are reported as
unsupported, with their position, rather than misread.

## Consequences

### Positive

- One place to look for every format, short to type.
- The TOML subset reads every clutch manifest in the repository.

### Negative

- A TOML document that uses an unsupported feature cannot be read until the
  module grows it.

## Migration

None.
