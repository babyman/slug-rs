# Slug loader crate boundary

This crate defines only external-import contracts: logical requests,
opaque identities, source payloads, checked failures, and import-scoped
activation. It must not depend on `slug-vm`, a concrete loader, or desktop
policy.

Use `make test-loader` for its focused suite. Keep the contract independent of
the VM, concrete loaders, and desktop policy.
