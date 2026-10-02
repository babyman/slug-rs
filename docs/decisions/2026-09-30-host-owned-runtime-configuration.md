# Host-owned runtime configuration

## Context

`slug-vm` currently owns both the runtime operations that implement `cfg`,
entrypoint arguments, and argument maps, and the desktop policy that reads
TOML files, environment variables, and command-line options. That makes a
restricted host carry filesystem and process-configuration code it must never
use. It also makes a launcher policy appear to be a VM requirement.

The crate-organization decision already assigns configuration assembly to the
`slug` launcher or an embedding host. This record defines the corresponding
runtime boundary.

## Decision

`slug-vm` will expose only an unstable `VmConfiguration` callback contract.
The contract resolves a key against a Slug fallback value and supplies the
already-selected entrypoint arguments and argument map. It performs no file,
environment, TOML, or command-line processing.

The `slug` crate will own the concrete configuration store and desktop
collection precedence. `slug-frontend` module hosts will retain an injected
`VmConfiguration`, while restricted hosts will provide an explicit in-memory
implementation. No configuration implementation is shared by default across
all hosts.

## Consequences

### Positive

The VM has no filesystem, environment, TOML, or launcher-policy dependency.
Desktop and restricted hosts make their configuration authority explicit, and
`cfg` retains its existing source behavior.

### Negative

Host implementations must carry a small configuration object, and test hosts
must provide one when they exercise configuration-sensitive behavior.

### Neutral

This changes an unstable Rust embedding seam only. Existing Slug source,
configuration precedence, diagnostics, and entrypoint argument behavior remain
unchanged.

## Migration

Move the existing concrete configuration implementation and its tests to
`slug`. Replace VM references with the callback contract, then prove the
restricted-host dependency tree still excludes desktop and configuration
collection code.
