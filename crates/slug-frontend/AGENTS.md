# Slug frontend crate boundary

This crate will own source syntax, semantic analysis, lowering, source
diagnostics, and the module graph caches. It may depend on `slug-vm` and
`slug-loader`, but it must not own desktop filesystem or environment policy.

Use `make test-frontend` for focused frontend work. Module loading behavior is
covered by `make test-modules`; keep desktop filesystem, environment, and
native-activation policy in the selected loader and executable host.
