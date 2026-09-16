# Maps

This supplement defines the currently implemented source syntax for maps. The
[Language Specification](language-specification.md) and the public `slug.std`
module define the broader collection API.

## Literals and keys

```slug
val byName = {name: "Slug", status: "ok"}
val byQuotedName = {"name": "Slug", "status": "ok"}
val field = "name"
val byValue = {[field]: "Slug"}
val multiline = {
  'first': 1,
  'second': 2,
}
```

A bare identifier or quoted string in a map literal is a string key. Therefore
`{name: x}`, `{"name": x}`, and `{["name"]: x}` use the same key. Bracketing a
key evaluates an expression, so `{[field]: x}` uses the value of `field` as its
key. Map literal entries are evaluated in source order.
Newlines immediately inside the braces or after an entry comma are whitespace;
entries remain comma-delimited.

A key must be hashable. Numbers, strings, bytes, and booleans are hashable.
`nil`, lists, maps, functions, and structs are not valid map keys.
An integer and a float key are equal only when the float exactly represents
that integer, so `1` and `1.0` address the same key without conflating large
integers above binary64's exact range. `0` and `-0.0` are equal. NaN is not a
valid map key; infinities may be keys but never equal integers.

Map patterns use the same bare, quoted, and bracketed key forms. A bracketed
map-pattern key is evaluated once before its pattern is tested; an unhashable
result follows the runtime type-error path.

## Access

```slug
val user = {name: "Slug"}
user.name       // string-key lookup
user["name"]   // equivalent string-key lookup
```

Bracket access evaluates its index expression. Dot access uses the property
identifier as a string key. Missing map keys evaluate to `nil`.

Maps do not have special method-call syntax. `m.key()` means ordinary lookup
followed by an ordinary call; no implicit map receiver is inserted.

## Persistent updates and keys

Maps are immutable, unordered values. `+` merges two maps into a new map, and
`-` removes one key into a new map. A right-hand merge value overwrites an
existing key. Removing a missing key leaves the map unchanged.

Two maps are equal when they have the same keys with equal values, regardless
of construction or enumeration order.

`map copy { key: value }` is a convenient persistent update for string keys.
It replaces existing keys and adds missing keys. Each key may occur only once
in the copy body.

```slug
val base = {name: "Slug", status: "ready"}
val updated = base + {status: "done", version: 1}
val withoutName = updated - "name"
val configured = withoutName copy { timeout: 5000, mode: "fast" }
```

Maps are unordered. Map construction still evaluates entries in source order,
but that order is not retained by the resulting value. `keys(map)` is a
`slug.std` foreign function that returns the current keys in unspecified order.
Programs that require an order must order the returned list explicitly. Keys
retain their language value types rather than being converted to strings.

```slug
val {keys} = import("slug.std")
len(keys(updated)) // 3
```
