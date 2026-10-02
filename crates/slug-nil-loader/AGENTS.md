# Slug nil-loader crate boundary

This crate implements the `slug-loader` contract by rejecting every
external import. It must never read host state or link desktop mechanisms.

Use `make test-nil-loader` for its focused suite. Do not add desktop fallbacks.
