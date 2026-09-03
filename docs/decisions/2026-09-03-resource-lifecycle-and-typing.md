# Keep resource lifecycle explicit and defer nominal resource syntax

## Context

Native resources provide the right runtime representation for file handles and
other host capabilities: they are opaque, module-owned, and type-checked by
the native interface. Treating reachability or destruction as the ordinary
close mechanism would make release timing, failure reporting, and resource
limits unpredictable. Representing handles as numbers would instead make them
forgeable and discard their module and type identity.

The source type system does not yet define a `resource` category, opaque type
declarations, or qualified type names. Adding nominal resource syntax now
would commit the language to a separate type namespace and import rules before
filesystem and other resource libraries establish the need.

## Decision

Resource lifecycle is explicit. A library exposes an idempotent `close`
operation; callers register it immediately with ordinary `defer`. Runtime
destruction and runtime shutdown are fallback leak containment only. They do
not define prompt release, flushing, lock release, or reportable cleanup
failure semantics.

Resources remain opaque at the language boundary. They have no numeric handle,
constructor, fields, serialization, or structural matching behavior. Native
operations validate resource module, resource type, and open state; using a
closed resource is a checked error.

No source-level `resource` annotation or nominal resource-type declaration is
adopted in this decision. A future design may add a broad `resource` category,
then—only when multiple libraries demonstrate the value—opaque module-owned
resource type declarations and `resource<T>` annotations. Such a proposal
must define type-name qualification, imports, inference, runtime match
constraints, and validation against native registrations. It must not model an
opaque resource as a struct or make resource destruction source-observable.

## Consequences

Filesystem and later native libraries use ordinary Slug control flow for
lifetime management rather than a special `with` form or finalization rule.
The existing native resource contract in
[`native-abi.md`](../reference/native-abi.md) remains the only resource typing
and ownership authority until a source-level design is separately adopted.

Early resource libraries may rely on runtime validation where static nominal
distinctions would eventually help, such as separating readable and writable
file handles. This postpones useful type precision, but avoids committing
syntax and namespace behavior before concrete APIs exercise it.

## Migration

None.
