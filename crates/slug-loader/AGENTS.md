# Slug loader crate boundary

This crate will define only external-import contracts: logical requests,
opaque identities, source payloads, checked failures, and import-scoped
activation. It must not depend on `slug-vm`, a concrete loader, or desktop
policy.

Use `make test-loader` for its focused suite. Keep the seam empty until the
contract moves here in the loader-extraction step.
