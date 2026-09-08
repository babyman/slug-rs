# Qualify native resource identities by module

## Context

ABI 0.10 gave one native library a shared resource lookup table so sibling
modules could exchange resources. That table used only a resource's short
name, allowing unrelated resources such as `slug.alpha.Handle` and
`slug.beta.Handle` to collide.

## Decision

Prototype ABI minor 11 identifies each native resource as
`(module_name, resource_name)`. The C resource operations accept the textual
form `module_name.resource_name`. Runtime validation retains the declaring
Slug module namespace as well as the library's shared resource scope.

## Consequences

Sharing a native library no longer merges module resource namespaces. Native
functions may consume a resource from a sibling module only by requesting its
fully qualified identity. Functions and resource declarations continue to
share one library lifecycle and cleanup ordering.

## Migration

Native adapters and fixtures replace short resource names in `set_resource`,
`argument_resource`, and `close_resource` calls, and select
`slug-ffi-prototype/0.11`. ABI 0.10 libraries are rejected.
