### Phase 4 — Treat nominal types as first-class proof

The core nominal-type representation now exists. This phase completes its use throughout static checking and proves that
nominal identity survives every relevant language and module boundary.

The governing rule remains:

> **Structural things are checked structurally; nominal things are checked by identity.**

Resources, enums, and schema-backed structs are nominal. Type aliases are transparent.

#### Task 1 — Lock down nominal assignability tests

Add focused tests proving:

```slug
resource File
resource Socket
```

means:

```text
File -> File       valid
Socket -> Socket   valid
File -> Socket     invalid
Socket -> File     invalid
```

Cover assignment, annotated declarations, function arguments, return values, and foreign calls.

#### Task 2 — Lock down enum identity

Add two enums with identical or overlapping case names:

```slug
enum ReadMode {
    Text,
    Binary,
}

enum WriteMode {
    Text,
    Binary,
}
```

Prove that values from one enum are never assignable to the other.

Cover function arguments, declarations, returns, unions, and match subjects.

#### Task 3 — Prove struct nominal identity

Ensure two schemas with structurally identical fields remain distinct:

```slug
val Point = struct {
    x:num,
    y:num,
}

val Size = struct {
    x:num,
    y:num,
}
```

`struct<Point>` must not be assignable to `struct<Size>` merely because their fields are identical.

Existing structural field checking within a known schema remains unchanged.

#### Task 4 — Prove aliases remain transparent

Add direct and chained alias tests:

```slug
type Path = str
type Filename = Path
```

Prove:

```text
Path == str
Filename == str
```

for assignability.

Also prove aliases to nominal types preserve the underlying nominal identity:

```slug
type Input = File
```

`Input` must behave exactly as `File`, not become a new nominal type.

#### Task 5 — Exercise nominal types through callable checking

Verify nominal identity is respected everywhere callable signatures are checked:

```slug
foreign read(file:File):bytes
```

must reject a statically known `Socket`.

Cover:

- ordinary functions;
- foreign functions;
- default parameters;
- variadic parameters where typed;
- higher-order function calls;
- statically selected overloads.

No call path should bypass the common assignability rules.

#### Task 6 — Exercise nominal types inside compound types

Prove identity is preserved through containers and unions:

```text
list<File> != list<Socket>
chan<File> != chan<Socket>
task<File> != task<Socket>

File|nil != Socket|nil
list<File|nil> != list<Socket|nil>
```

Where container assignability is intentionally invariant today, retain that behavior unless separately changed.

#### Task 7 — Prove nominal identity across module boundaries

Create module-level tests where two modules export nominal types with the same short name:

```text
module.a.Handle
module.b.Handle
```

They must remain distinct after import.

A value produced using `module.a.Handle` must be accepted by functions expecting the same exported identity and rejected
by functions expecting `module.b.Handle`.

Test resources, enums, and structs independently.

#### Task 8 — Prove aliases across module boundaries

Export aliases such as:

```slug
export type Path = str
export type DatabaseHandle = Database
```

and prove imported aliases retain transparency.

An alias to an exported nominal type must resolve to that original nominal identity rather than gaining identity from
the importing module.

#### Task 9 — Audit annotation resolution for nominal identity

Review every path through static annotation resolution and ensure unresolved nominal references become canonical
declaration identities before checking.

In particular, verify:

- local annotations;
- function parameter/result annotations;
- foreign signatures;
- imported module-member types;
- aliases;
- generic/container arguments;
- unions.

No semantic comparison should depend only on the displayed short type name.

#### Task 10 — Audit all static compatibility checks

Find all locations performing type compatibility or equality decisions and ensure assignability flows through the
canonical type relation rather than ad hoc comparisons.

Direct `Type` equality should only be used where exact equality is actually intended.

Add regression tests for any discovered bypasses.

#### Task 11 — Improve nominal mismatch diagnostics

When two nominal types differ, diagnostics should clearly identify the actual and expected types.

Where short names would be ambiguous, include enough module/declaration context to distinguish them.

For example, a mismatch between:

```text
module.a.Handle
module.b.Handle
```

must not be rendered merely as:

```text
expected Handle, found Handle
```

Structured diagnostics should preserve the same distinction.

#### Task 12 — Add an end-to-end Clutch/resource proof

Use the SQLite Clutch as a real integration test:

```slug
val sqlite = import("slug.db.sqlite")
val statement = import("slug.db.sqlite.statement")

val db = sqlite.open(":memory:")
val stmt = statement.prepare(db, "select 1")
```

Prove that:

- `db` is inferred as the exported SQLite `Database` resource;
- `statement.prepare` accepts that exact nominal resource;
- an unrelated resource cannot be substituted;
- the returned `Statement` retains its own module-qualified identity.

This should exercise the same semantic snapshot machinery used by ordinary source modules rather than introducing
Clutch-specific typing behavior.
