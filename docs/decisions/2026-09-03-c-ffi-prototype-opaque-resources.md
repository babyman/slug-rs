# Represent C handles as checked Slug resources

## Context

Scalar FFI calls and module-state teardown do not validate the important case
where C allocates a handle that must survive one call, reject a wrong or closed
handle later, and release its allocation exactly once.

## Decision

The prototype module descriptor may declare named resource types, each with a
non-null C destructor. C callbacks transfer non-null pointers through the host
resource setter, borrow matching pointers only for the current callback, and
request close by argument index. The existing Slug native-resource facade owns
type identity, open/closed state, and final-release cleanup; its close and drop
paths invoke the C destructor at most once.

## Consequences

- C pointers never become direct Slug values or escape callback-scoped borrows.
- Wrong-type and closed-handle failures retain the checked native-resource
  diagnostics already exercised by the Rust facade.
- Resource declarations and destructors are validated before functions are
  registered, but arbitrary pointer types, ownership sharing, and asynchronous
  use remain outside the prototype.

## Migration

The unstable prototype minor version is now 2. Existing experimental C module
descriptors append null resource-table fields when they declare no resources.
