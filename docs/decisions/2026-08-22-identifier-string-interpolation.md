# Use identifiers for string interpolation

## Context

The target string syntax previously described arbitrary expressions inside
interpolation delimiters. That would require a second expression parsing
context inside string lexing and makes evaluation boundaries harder to read.

## Decision

Non-raw strings interpolate `$identifier` only. The identifier resolves through
ordinary Slug lexical and global lookup. Property access and arbitrary
expressions are not interpolation syntax; programs compute such values before
constructing the string.

## Consequences

String interpolation preserves ordinary name diagnostics and can use a compact
private bytecode operation. Raw strings retain `$identifier` literally.

## Migration

Use a named intermediate value and `$name` for interpolation.
