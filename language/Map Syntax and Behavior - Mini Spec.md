# Maps

This supplement defines the currently implemented source syntax for maps. The
[Language Specification](language-specification.md) and the public `slug.std`
module define the broader collection API.

## Literals and keys

```slug
val bySymbol = {name: "Slug", :status: :ok}
val field = "name"
val byValue = {[field]: "Slug"}
```

A bare identifier in a map literal is a **symbol** key. Therefore `{name: x}`
and `{:name: x}` use the same key. Bracketing a key evaluates an expression,
so `{[field]: x}` uses the value of `field` as its key. Map literal entries are
evaluated in source order.

A key must be hashable. Numbers, strings, bytes, symbols, and booleans are
hashable. `nil`, lists, maps, functions, and structs are not valid map keys.

## Access

```slug
val user = {name: "Slug"}
user.name       // symbol-key lookup
user[:name]     // equivalent symbol-key lookup
user["name"]   // string-key lookup
```

Bracket access evaluates its index expression. Dot access uses the identifier
as a symbol key. For compatibility with existing string-keyed maps, a dot
lookup that has no matching symbol key also looks for a string key of the same
name. Missing map keys evaluate to `nil`.

Maps do not have special method-call syntax. `m.key()` means ordinary lookup
followed by an ordinary call; no implicit map receiver is inserted.

## Collection operations

The standard library exposes map operations such as `get`, `put`, `remove`,
and `keys` through the public library surface. Their signatures, update
semantics, and errors are defined by the versioned library sources in
`../../lib/slug`, not by this syntax supplement.
