# slug.string provides sort and join

## Status

Accepted

## Context

Ordering and assembling strings is common, and Slug programs hand-wrote
insertion sorts and string-building loops to do it. Agents writing Slug did the
same, and a JSON serializer needs both to produce sorted object keys and
comma-separated members. [The core clutch's string module](2026-10-16-core-clutch-string.md)
is the place for string operations, and strings now have an ordering
([string ordering](2026-10-16-string-ordering.md)).

## Decision

`slug.string` exports two more functions, written in Slug.

- `sort(strings:list<str>):list<str>` returns a new list in ascending order by
  the ordering of `<`, lexicographic by UTF-8 bytes. It is a stable merge sort,
  so equal strings keep their order, and it leaves its argument unchanged.
- `join(strings:list<str>, separator:str = ""):str` concatenates the strings
  with `separator` between adjacent elements; an empty list gives `""`.
- Both require every element to be a string and throw an `Error` whose `type`
  is `string.type` otherwise, rather than converting values to their display
  form.

They are Slug functions rather than native bindings because typed bindings do
not take lists.

## Consequences

### Positive

- Sorting and joining strings needs no hand-written loops.

### Negative

- Byte order is not a natural-language collation: `"B"` sorts before `"a"`.

### Neutral

A general sort for other element types or with a comparison function belongs
in `slug.std` if it is added.

## Migration

None.
