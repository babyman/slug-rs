# Host-Owned Module Resolution

## Context

The current `ModuleLoader` combines two different responsibilities. It is the
desktop host's filesystem, library-root, and experimental-Clutch policy, but it
also owns the runtime work needed to compile modules, retain semantic import
snapshots, initialize cached instances, and make `import()` work. The command
line runner independently implements a related entry-program filesystem search.

That coupling makes an embedded host carry desktop assumptions even when every
module is compiled into firmware. In particular, the VM obtains an importer by
turning a source-span path into a filesystem path. A module's source identity,
cache identity, and diagnostics should not require it to live in a filesystem.

## Decision

Module resolution is a host service. The VM and the filesystem-free module
runtime request a logical module name and an opaque importing-module identity;
they do not inspect paths, environment variables, Clutch manifests, or storage
locations. A deliberately small resolver interface returns a resolved module
with:

- an opaque stable identity for cache and cycle detection;
- a diagnostic identity or display name;
- a source payload now, and a validated artifact payload when `.cslug` exists;
- an optional host activation lease for module-scoped native registrations.

The filesystem-free module runtime retains compilation, semantic-snapshot,
module-instance, and initialization behavior. It uses the same resolver for
static-import analysis and runtime `import()`.

The desktop host provides one concrete `DesktopLoader`. It owns both entry
program lookup and subsequent import resolution because they share roots,
diagnostics, source reading, and canonical identity. They remain distinct
operations: entry lookup preserves explicit-path and bare-library behavior,
while import lookup preserves importer-relative, project-root, library-root,
then Clutch precedence. Experimental Clutch parsing and native-plugin loading
remain desktop-host policy.

An embedded host provides a separate resolver backed by a compiled-in module
table. Its modules resolve from flash/ROM and do not require filesystem or
Clutch-directory access. The first embedded representation is uncompressed and
directly addressable. It may embed source while the portable `.cslug` contract
is unimplemented; no private VM bytecode becomes an embedded artifact format.

The initial implementation keeps these components in the existing crate with
a desktop-host feature boundary. A separate Cargo crate is deferred until an
actual embedded build demonstrates that feature gating cannot exclude desktop
dependencies or meet its target requirements.

## Consequences

### Positive

Desktop behavior has one owner for entry and import lookup. The VM can be
compiled without filesystem resolution machinery, and a ROM-backed resolver
can exercise the existing module semantics. Resolver-agnostic tests can prove
that desktop and embedded hosts have the same import, cache, and initialization
behavior for the same module graph.

### Negative

The current `PathBuf`-based caches and errors must be converted to logical
module identity plus host-provided diagnostic detail. Clutch activation must be
made an explicit optional host lifecycle hook instead of leaking desktop policy
into ordinary module resolution. The refactor must preserve existing CLI
search order and diagnostics exactly where they are observable.

### Neutral

This decision does not alter Slug `import()` syntax, resolution precedence, or
module initialization semantics. It does not publish a Rust loader API,
implement `.cslug`, define a Clutch archive format, or require `no_std`.
`docs/reference/compiled-artifacts.md` remains the gate for a portable artifact
payload.

## Migration

None for Slug programs, installed desktop libraries, or Clutches. The Rust
embedding surface is pre-release and may replace `ModuleLoader` construction
with the new host/runtime assembly during implementation.
