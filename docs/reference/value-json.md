# Value JSON Form

This document is normative. It defines the one JSON encoding of every Slug
value, used wherever a Slug implementation shows a value to a non-Slug
consumer: JSON diagnostics, the interactive server, and host tools. See the
[decision record](../decisions/2026-10-13-value-json-form.md) for rationale.

Only encoding is defined. Decoding is reserved for a later version.

## Encoding

The canonical encoding is compact JSON text: no whitespace between tokens, and
object members in the order shown below. Equal Slug values have identical
canonical encodings.

JSON object member order carries no meaning. A document that embeds an
encoding may re-render it with any JSON serializer that preserves the JSON
data model, which may reorder object members or respell numbers with the same
value. Array order is always significant, so list elements and map entries
keep their order. A struct's field order is schema metadata, not data.

| Value | Encoding | Example |
|---|---|---|
| `nil` | `null` | `null` |
| boolean | `true` or `false` | `true` |
| number | JSON number, see [Numbers](#numbers) | `42`, `0.5`, `1e21` |
| string | JSON string, see [Strings](#strings) | `"hi"` |
| list | JSON array of element encodings, in order | `[1,"a"]` |
| map | `{"$map":[[key,value],...]}`, see [Maps](#maps) | `{"$map":[["a",1]]}` |
| bytes | `{"$bytes":"<lowercase hex>"}` | `{"$bytes":"00ff"}` |
| struct | `{"$struct":{"<field>":value,...}}`, fields in declaration order | `{"$struct":{"x":1,"y":2}}` |
| enum case | `{"$enum":"<enum name>","case":"<case name>"}` | `{"$enum":"Color","case":"Red"}` |
| other | `{"$opaque":"<kind>","display":"<text>"}`, see [Opaque values](#opaque-values) | `{"$opaque":"fn","display":"<fn>"}` |

Every JSON object in an encoding, other than a struct's field object, is a
tagged form. Exactly one member's name begins with `$`, and that member
identifies the form. A JSON object never encodes a map directly. Struct field
names are Slug identifiers and never begin with `$`.

A struct carries no schema name, because schemas have runtime identity but no
name. An enum case carries its enum and case names, but not its declaring
module.

### Numbers

A number equal to an `i64` value is written as that integer in decimal, with a
leading `-` when negative. This includes integral floats, so `1.0` encodes as
`1` and `-0.0` encodes as `0`.

Any other finite number uses the shortest decimal digits that round-trip to
the same binary64 value. It uses plain decimal notation when its magnitude is
at least `1e-6` and less than `1e21`, and otherwise exponent notation with a
lowercase `e`, an optional `-` exponent sign, and no leading `+` or zeros:
`0.1`, `1.5e-7`, `1e21`.

JSON parsers differ in integer precision. A consumer that reads numbers as
binary64 loses precision for integers beyond `2^53`.

### Strings

A string is a JSON string containing the Slug string's Unicode scalar values.
`"` and `\` are escaped as `\"` and `\\`. U+0008, U+0009, U+000A, U+000C, and
U+000D are escaped as `\b`, `\t`, `\n`, `\f`, and `\r`. Every other code point
below U+0020 is escaped as `\u00` and two lowercase hex digits. All other code
points appear literally as UTF-8.

### Maps

A map encodes as an array of two-element `[key,value]` arrays. Entries are
sorted by their key's encoding, compared as byte strings. Map keys are
booleans, numbers, strings, or bytes, and equivalent keys have identical
encodings, so the order is total and independent of how a runtime stores or
enumerates the map.

### Opaque values

A function, channel, task, struct schema, native resource, or uninitialized
module binding encodes as an opaque descriptor. Its `$opaque` kind is one of
`fn`, `chan`, `task`, `schema`, `native_resource`, or `uninitialized`. Its
`display` text is for people and is not a compatibility contract. An opaque
descriptor carries no state or identity: two different functions may have
identical encodings.

A number that is not finite is outside Slug's value set; if one reaches the
encoder, it is opaque with kind `num`.

### Depth limit

An encoding never nests more than 96 JSON arrays and objects, which leaves
room for an enclosing document within common parser limits such as
serde_json's default of 128.

Count the arrays and objects that enclose a value as its depth `D`. A list
adds 1 level, a struct 2, and a map 3. A list, struct, or map whose added
levels would make `D` plus those levels exceed 95 is replaced by
`{"$truncated":"<kind>"}`, where the kind is `list`, `struct`, or `map`.
Every other value fits.

Encoding never fails.
