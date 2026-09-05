# Nominal types

Slug has three top-level declarations that introduce type names:

```slug
export resource File

export enum SeekFrom {
  Start
  Current
  End
}

export type Path = str
```

`resource` and `enum` introduce nominal types. `type` introduces a transparent
alias. These declarations are valid only at module top level. `export` makes a
type name available to importers in the module's compile-time type namespace;
ordinary values and type names remain separate namespaces.

## Resource handles

`resource File` declares an opaque, nominal native handle type. Slug has no
literal, constructor, field access, cast, serialization, or structural
destructuring operation for a resource handle. Only a native callback owned by
the declaring module can create one.

Each declared resource name corresponds to exactly one native resource type
registered by the same module. A native registration has an internal host name
and a Slug-visible name; the latter must match the source declaration exactly.
A foreign call crossing that boundary MUST validate declared resource arguments
and results against that registration.
The validation applies even when a call is dynamically reached or optional
static checking is disabled.

Resource handles may occur in every ordinary type position, including unions,
collections, channels, function signatures, and whole-case type constraints:

```slug
export foreign openRead = fn(path:str):File
export foreign readLine = fn(file:File):str|nil

val files:list<File> = []
val pending:chan<File> = chan(8)
```

Resource types are distinct from every other type, including another resource
type declared in the same module. There is no broad `resource` source type:
an API accepting more than one handle kind MUST state a union of its accepted
nominal types. A resource's open state is dynamic; using a correctly typed
handle after `close` remains a checked runtime error.

## Enumerations

An enum declares a closed, nominal type and one immutable, fieldless value for
each case:

```slug
export enum SeekFrom {
  Start
  Current
  End
}

val from:SeekFrom = SeekFrom.Start
```

Cases are always qualified. `SeekFrom.Start` is a valid enum-value expression;
bare `Start` is an unknown name. The same rule applies to match patterns:

```slug
match from {
  SeekFrom.Start => 0
  SeekFrom.Current => 1
  SeekFrom.End => 2
}
```

Enum values compare equal only when both their enum identity and case are the
same. They are not strings or numbers, cannot be forged by casts, and do not
support explicit discriminants, bit flags, methods, or payload-bearing cases.
An enum's complete case set is available to closed-match coverage checking.

## Aliases

`type Name = Annotation` gives an existing type another name:

```slug
export type Path = str
export type MaybeFile = File|nil
```

Aliases are transparent. `Path` and `str` are interchangeable for
assignability, overload selection, callable-signature identity, and runtime
matching. An alias does not create a value, a constructor, or a new nominal
identity. Recursive alias definitions are source errors. Generic aliases and
strong typedefs are outside this feature.

## Imports

An exported type is selected through the compile-time type namespace of an
imported module binding. Given a module binding named `fs`, `fs.File` names the
exported `File` type:

```slug
val fs = import("slug.io.fs")
val file:fs.File = fs.openRead("records.csv")
```

The identifier before the first `.` in such a type path MUST resolve to a
module binding created by `import`. The remaining path selects an exported type
from that module. A type path preserves its declaring module and declaration
name as nominal identity; importing, renaming, or aliasing a type MUST NOT make
two independently declared resource or enum types equal.

Enum declarations also expose their enum namespace as an ordinary immutable
value. Importing an enum therefore preserves qualified case access:

```slug
val options = import("slug.io.options")
val from:options.SeekFrom = options.SeekFrom.Start
```

Importing an enum does not inject its case names into the importing module.

## Implementation status

The current Rust subset implements nominal resource declarations, exported
resource type paths such as `fs.File`, the `File` signatures in `slug.io.fs`,
and checked source/native resource-registration agreement. Typed foreign
resource argument/result validation, typed resource match constraints, enums,
and aliases remain unimplemented.
