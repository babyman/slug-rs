# Slug nil-loader crate boundary

This crate will implement the `slug-loader` contract by rejecting every
external import. It must never read host state or link desktop mechanisms.

Use `make test-nil-loader` for its focused suite. Add its no-host-access proof
when the contract is extracted; do not add temporary desktop fallbacks.
