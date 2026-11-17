# Project root marker

## Status

Accepted (2026-11-15). Prerequisite of
[layered clutch repositories](2026-11-15-layered-clutch-repositories.md).
Implementation: [project roots and layered clutches](../planning/completed/project-roots-and-layered-clutches.md).

## Context

The desktop runner's project module root is the directory of the entry
program, unless the host configures one (`SLUG_FIXTURE_MODULE_ROOT`, the
`-root` of the reference command line). That root serves two purposes:

- the second import candidate, after the importer's own directory;
- the location of the project `slug.toml`
  ([configuration](../language/configuration.md#sources-and-precedence)).

A program in a subdirectory of its project, such as `tests/jobs_test.slug` or
`bin/serve.slug`, therefore gets the subdirectory as its root. It cannot
import the project's top-level modules by name, and it does not see the
project's `slug.toml`. The resolver works around the first gap with a hint
(`modules_above` in `slug-desktop-loader`) that suggests `../jobs.slug`
when a module is found above a searched root. Agents writing eval programs
hit this. Project-local clutches would have the same gap.

Most toolchains find the project root from a marker file: Cargo
(`Cargo.toml`), Go (`go.mod`), npm (`package.json`), Bundler (`Gemfile`),
Zig (`build.zig`), Git (`.git`). They search upward from the working
directory because the user runs a project command. Slug, like `node
file.js`, is given a file, so the search starts from that file, as Node's own
`node_modules` lookup starts from the importing file.

## Decision

`slug.toml` marks a project root.

When the host configures no module root, the runner finds the entry program
as it does now (the path as given, then the library fallback), then searches
the entry's directory and each of its ancestors in turn for a file named
`slug.toml`. The first directory that has one is the project module root.
If none has one, the root is the entry's directory, as now.

- The search runs over the canonical entry directory and stops at the first
  match. It stops at the filesystem root and has no other ceiling. A root
  above the entry's directory is used in canonical form; imported modules
  are already identified by canonical path, so diagnostics do not change.
- A nested `slug.toml` starts an independent project. Workspaces, where one
  root spans several projects, are not part of this decision.
- An entry found through the library fallback (`slug hello`, reading
  `$SLUG_HOME/lib/hello.slug`) does not search. Its root stays the
  library directory, so `$SLUG_HOME/lib/slug.toml` is never treated as a
  project marker for an installed tool.
- A configured module root is used as given, with no search.
- The interactive server (`--server`) searches from the application's
  directory, or from the working directory when it has no application.
- The file's presence marks the root. Its contents stay configuration, with
  the same precedence as today. An empty `slug.toml` is a valid marker.
- Import order is unchanged: the importer's directory, the project root, the
  library root, then clutch providers. Only which directory is the project
  root changes.

## Consequences

`tests/` and `bin/` programs import top-level project modules by name and see
the project configuration, with no `-root` and no `../` paths. The
`modules_above` hint still helps projects with no `slug.toml`. Its message
should suggest adding one.

A program below an unrelated `slug.toml` in an ancestor directory joins that
project: its imports and configuration resolve from there. A stray
`~/slug.toml` would capture every program under the home directory, the
same hazard as a stray `package.json`. The runner does not guard against it;
a configured root overrides the search.

Startup does a few more `stat` calls: at most one per ancestor directory of
the entry.

The resolved root belongs in diagnostics that name import candidates, so a
surprising root is visible.

Documents to update when implemented: `language/configuration.md` (project
TOML location and the library source path section),
`language/runtime-requirements.md` (command-line runner and configuration
sources), and the module-resolution section of
`reference/clutches.md`.

Tests: CLI tests for a subdirectory entry importing a root module and reading
root configuration, a nested project, a library-fallback entry not searching,
a configured root not searching, and the no-marker fallback. Conformance
fixtures set their root explicitly and are unaffected.

## Migration

No `slug.toml` exists in this repository today, so no checked-in program
changes root. A program outside the repository changes behavior only if an
ancestor directory has a `slug.toml`. Such a program can keep its old root
through the configured module root.
