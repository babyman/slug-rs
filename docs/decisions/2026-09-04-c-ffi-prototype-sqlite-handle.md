# Exercise C resource handles with SQLite

## Context

The custom counter resource validates the host's lifecycle mechanics, but a
real C library can reveal assumptions about string borrowing, native errors,
and handle teardown that a hand-written allocation fixture cannot.

## Decision

The FFI prototype includes a test-only SQLite adapter linked against the local
SQLite development library. It exposes an in-memory database resource plus
`exec`, scalar `queryInt`, and `close`. SQL is borrowed as length-delimited
UTF-8 only during the callback. SQLite failures use the `sqlite.error` native
error code. Database handles use the ordinary opaque-resource destructor path.

## Consequences

- The prototype validates a real third-party C handle and a borrowed text
  argument without making SQLite a runtime dependency of Slug itself.
- The fixture establishes that adapter code must use explicit input lengths;
  it cannot assume a host string is NUL-terminated.
- Prepared statements, bound values, row iteration, transactions, and
  filesystem/database configuration are intentionally deferred.

## Migration

The unstable prototype minor version is now 4 because the host table appends a
callback-scoped text-argument accessor. Existing fixtures remain compatible
when rebuilt against the header.
