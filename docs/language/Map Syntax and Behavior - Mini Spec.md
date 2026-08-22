# Maps

This supplement defines the currently implemented source syntax for maps. The
[Language Specification](language-specification.md) and the public `slug.std`
module define the broader collection API.

## Literals and keys

```slug
val byName = {name: "Slug", status: "ok"}
val field = "name"
val byValue = {[field]: "Slug"}
```

A bare identifier in a map literal is a string key. Therefore `{name: x}` and
`{["name"]: x}` use the same key. Bracketing a key evaluates an expression, so
`{[field]: x}` uses the value of `field` as its key. Map literal entries are
evaluated in source order.

A key must be hashable. Numbers, strings, bytes, and booleans are hashable.
`nil`, lists, maps, functions, and structs are not valid map keys.

Map patterns use the same bare and bracketed key forms. A bracketed
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

## Collection operations

The standard library exposes map operations such as `get`, `put`, `remove`,
and `keys` through the public library surface. Their signatures, update
semantics, and errors are defined by the versioned library sources in
`../../lib/slug`, not by this syntax supplement.
