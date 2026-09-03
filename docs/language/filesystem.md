# Filesystem resources

The `slug.io.fs` module provides explicit-lifecycle text-file streams:

```slug
val fs = import("slug.io.fs")

val file = fs.openRead("records.csv")
defer fs.close(file)

match fs.readLine(file) {
  nil => "empty"
  line => line
}
```

`openRead(path)`, `openWrite(path)`, and `openAppend(path)` return opaque file
resources. `openWrite` creates or truncates its file; `openAppend` creates it
when missing and writes at its end. File resources are not numbers, structs,
maps, or source-level constructors. Their module ownership, resource kind, and
open state are validated by the native boundary.

`readLine(file):str|nil` returns one line without a final `\n`, also removing
the preceding `\r` from CRLF input. Empty lines are `""`; end of file is `nil`.
`write(file, content):num` writes all of `content` and returns its UTF-8 byte
length. Reading a writable handle or writing a readable handle is a checked
runtime error.

`close(file):nil` is idempotent. Any later operation on that handle produces a
checked resource-closed error. Programs must register `close` with `defer`
immediately after a successful open. Runtime destruction and shutdown may
release forgotten resources, but do not provide prompt release, flushing, or
observable cleanup-error semantics.

The current source type system does not expose a `resource` annotation. File
resources are nevertheless opaque and checked at runtime; source-level nominal
resource types remain deliberately deferred by the resource lifecycle decision.
