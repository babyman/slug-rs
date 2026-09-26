# Slug frontend crate boundary

This crate will own source syntax, semantic analysis, lowering, source
diagnostics, and the module graph caches. It may depend on `slug-vm` and
`slug-loader`, but it must not own desktop filesystem or environment policy.

Use `make test-frontend` for its focused suite. During the seam-only step this
crate intentionally has no behavior; add tests with each moved frontend unit.
