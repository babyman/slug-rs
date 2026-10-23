# Values have one JSON form

## Status

Accepted

## Context

Hosts, diagnostics, and tools need to show Slug values to programs that are not
Slug, including agents, review tools, and editors. JSON diagnostics omitted
thrown payloads because Slug had no stable value serialization. The interactive
server rendered values its own way, collapsing maps, bytes, structs, and enums
to display text. Planned capability reports,
session inspection, and replay records all need the same answer, so one form
must own it. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Every Slug value has exactly one JSON encoding, defined in
[Value JSON Form](../reference/value-json.md).

- `nil`, booleans, numbers, strings, and lists use the matching JSON type.
- Every JSON object is a tagged form with exactly one member whose name is
  the tag: `$map`, `$bytes`, `$struct`, `$enum`, `$opaque`, or `$truncated`.
  A decoder can therefore always tell a value's kind without a schema.
- JSON object member order carries no meaning, because many JSON libraries
  do not preserve it. The encoder writes one canonical text, but a document
  that embeds it may be re-rendered by any serializer that keeps the JSON data.
- Maps encode as `{"$map": [[key, value], ...]}` because keys may be
  booleans, numbers, strings, or bytes. Entries are sorted by the encoded
  key's text, so a map has the same encoding on every run and in every
  runtime, regardless of internal layout or enumeration order.
- Numbers use the map-key canonical form: a number equal to an `i64` is
  written as that integer, so `1`, `1.0`, and `-0.0` encode as `1`, `1`,
  and `0`. Other numbers use the shortest text that round-trips binary64.
- Bytes are lowercase hex, matching the `0x"..."` literal.
- Structs encode their fields in declaration order. Struct schemas have
  runtime identity but no name, so the encoding carries no schema name.
- Functions, channels, tasks, schemas, native resources, and uninitialized
  bindings are opaque: a kind and display text, with no state or identity.
- Encoding never fails. A value nested beyond the depth limit is replaced by a
  `$truncated` marker, and a non-finite number, which the VM's finite-number
  rule should already exclude, is opaque.

This round defines encoding only. The tags are chosen so a decoder can be added
without changing the encoding.

## Consequences

### Positive

- JSON diagnostics, the interactive server, and later reports share one value
  form, so a consumer learns it once.
- Equal values have equal encodings, so encoded values can be compared and
  diffed as text.

### Negative

- Encoded structs cannot be matched back to a schema without context from the
  producing program.
- Common string-keyed maps are more verbose than a plain JSON object.

### Neutral

The encoder lives in `slug-vm`, has no dependencies, and is available on
embedded targets.

## Migration

The interactive server's `value` results change from its earlier rendering to
this form. JSON diagnostics gain a `payload` member. The server protocol is not
yet stable, so no compatibility path is kept.
