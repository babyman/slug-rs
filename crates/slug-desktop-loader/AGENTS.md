# Slug desktop-loader crate boundary

This crate implements importer-relative, project-root, library-root, and
Clutch resolution plus import-scoped native activation. The executable must
supply roots and policy; this crate must not inspect `SLUG_HOME`, arguments,
or configuration files.

Use `make test-desktop-loader` for its focused suite. Preserve transactional
activation and library-lifecycle tests when that behavior moves here.
