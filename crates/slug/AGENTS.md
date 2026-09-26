# Slug executable crate boundary

This crate will compose the VM, frontend, and selected loader. It will own
desktop CLI policy, configuration assembly, entry lookup, builtin
registration, and server mode; loaders must not absorb those responsibilities.

Use `make test-slug` for its focused suite. The current crate is an empty seam;
do not add a competing executable before the runner-migration step.
