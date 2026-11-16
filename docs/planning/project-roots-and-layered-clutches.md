# Project Roots and Layered Clutches Plan

This task list implements two proposed decisions, in order:

1. [Project root marker](../decisions/2026-11-15-project-root-marker.md)
2. [Layered clutch repositories](../decisions/2026-11-15-layered-clutch-repositories.md)

## Scope and invariants

- Import candidate order does not change: importer, project root, library
  root, clutch provider.
- A configured module root and conformance fixtures behave exactly as now.
- With no `slug.toml` above the entry and no project `clutch/manifest.toml`,
  every observable behavior is unchanged.
- No VM, `slug-loader`, or embedded code reads either file. Discovery lives
  in the CLI host (`crates/slug`). Combining repositories lives in
  `slug-desktop-loader`.

## 1. Project root discovery

- [x] `project_root(entry: &Path) -> PathBuf` in `crates/slug/src/host.rs`:
  canonical entry directory, walk ancestors, first with `slug.toml`, else
  the entry directory.
- [x] Use it where `program_directory` picked the root: `run`,
  `load_entry_source` (`compile`, `report`), and `run_artifact` in
  `main.rs`; and in `server.rs`, starting from the application directory
  or the working directory.
- [x] Skip the search for an entry found through the library fallback: one
  whose resolved path is not the path requested.
- [x] Have the `modules_above` hint suggest adding `slug.toml`. (The
  searched paths, project root among them, were already in the error.)
- [x] CLI tests: subdirectory entry imports a root module; subdirectory
  entry reads root `slug.toml`; nested project; library-fallback entry;
  configured root; no marker.
- [x] Update `language/configuration.md` and
  `language/runtime-requirements.md`.

**Gate:** full CLI and module-loader suites pass. No checked-in program
changes root (none has a `slug.toml` above it).

## 2. Combining repositories

- [x] `ClutchRepository::layer(self, other) -> Result<Self, ClutchRepositoryError>`:
  merge `providers` and `tests`, reject an identity indexed by both, and
  reject a test name that is also a provider name across both. Merge plugin
  initializers, rejecting an entry both define.
- [x] Record the manifest that indexed each name, and report a cross-layer
  conflict as `LayerConflict`, with both manifests and clutch directories.
- [x] Loader tests: disjoint layers resolve from both; identity conflict;
  test-name conflict; `clutch_containing` and `asset` for a project clutch.

## 3. CLI project layer

- [x] `build_default_host_vm`: load the home layer, then
  `<source_root>/clutch/manifest.toml` if it exists, and combine them.
  `source_root` is now the discovered project root. Skip the project layer
  when it is the home repository.
- [x] CLI tests: program imports a project clutch with `SLUG_HOME` providing
  `slug.std`; a `tests/` program in the same project; a conflict reports
  both manifests; a project clutch's asset.
- [x] `.cslug` run: a foreign module whose clutch is not visible is not
  found, and the error names it. No change needed.
- [x] Update `reference/experimental-clutches.md`, including the note that a
  project's native libraries load when its programs run.

**Gate:** full suite, `make docs-check`, and `cargo +esp check` for
`slug-vm` (expected untouched).

## 4. Native staging

- [x] `scripts/stage-native-clutches.sh [--repository DIR] [clutch...]`:
  default `clutch/`; stage every clutch with a `[native]` table instead of
  the fixed list. `slug.arcade`'s raylib build moved to its
  `native/stage-prerequisites.sh`.
- [x] `make stage-native-clutches` keeps staging this checkout.

## 5. Project layout

Implements [project layout](../decisions/2026-11-15-project-layout.md).

- [x] `ProgramLayout` in `crates/slug/src/host.rs`: a project's module root is
  its `src/`; configuration and `clutch/` come from the project root.
  `build_default_host_vm` takes the layout.
- [x] `DesktopResolver::with_project_root`: project assets from `assets/`,
  after the containing clutch's; the not-found error's new `hints` name a
  missing `src/`.
- [x] CLI tests: `src/` and `tests/` programs, a `tests/` helper, a root
  module not importable, a missing `src/` and `assets/`, project and clutch
  assets, a `.cslug` carrying a project asset.
- [x] Update `language/configuration.md`, `language/testing.md`, the
  specification's assets section, and `reference/experimental-clutches.md`.
- [x] Move Orble to the layout (in ../orble, uncommitted there).

## 6. First user (separate change)

- [ ] Optional: move `slug.games.crawl` into its own project with a
  `slug.toml` and `clutch/manifest.toml`, and drop its entries from the
  shared manifest. Check the Crawl benchmarks and `scripts/pgo-build.py`,
  which read clutches from this checkout.
