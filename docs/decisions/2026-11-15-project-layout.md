# Project layout

## Status

Accepted (2026-11-15). Refines
[project root marker](2026-11-15-project-root-marker.md), which made the
project root and the module root the same directory.

## Context

The project root marker made `slug.toml` mark a project, and used that
directory both for the project's files (its `slug.toml`, its `clutch/`
repository) and as the module root that imports search. A project that keeps
its source in `src/` therefore has to keep `slug.toml` in `src/` too. Orble,
a web application developed outside this repository to test the experience
of building a Slug project, does exactly that, and records it as an open
problem: configuration does not belong in the source tree, and with
`slug.toml` at the top of the project, `import("orble.app")` looks for
`orble/app.slug` beside it, not in `src/`.

Programs outside a clutch also have no assets. Orble reads its template and
stylesheet at run time through `slug.io.fs`, with paths relative to the
working directory, so a compiled `.cslug` does not carry them and the
program depends on where it runs from. A clutch embeds files from its
`assets/` folder with `asset("name")` instead
([clutch assets](2026-10-25-clutch-assets.md)), and that decision's
consequences note that a program outside a clutch would need a project
manifest to have assets.

A project that grows into a library should be able to become a clutch
without rewriting its imports or asset names.

## Decision

A Slug project has one layout, with nothing to configure:

```text
project/
  slug.toml      the project root marker and its configuration
  src/           the project's modules: the module root imports search
  assets/        files `asset("name")` embeds
  tests/         the project's test programs
  clutch/        optional project clutches, for native code
```

- **`src/` is the module root.** In a project, imports search the
  importer's directory, then `<project root>/src`, then the library root,
  then clutch providers. The project root itself is not searched. A module's
  name is its path under `src/`: `src/orble/app.slug` is `orble.app`.
- **`assets/` holds the project's assets.** A module whose source file is
  inside the project root, and not inside an indexed clutch, can embed files
  from `<project root>/assets/` with `asset("name")`, under the same name,
  containment, case, and size rules as clutch assets. That covers `src/` and
  programs elsewhere in the project, such as `tests/`. A module in a clutch,
  including a project clutch, uses its clutch's assets.
- **`tests/` holds the project's test programs.** A test program there
  imports the modules it tests from `src/` by name, and its helpers beside
  it by the importer-relative rule: `tests/all.slug` imports `web` from
  `tests/web.slug`. Modules in `src/` cannot import tests, since `tests/` is
  not under the module root. Test programs run by path, `slug tests/all.slug`,
  and follow [Testing a Program](../language/testing.md); they are ordinary
  programs, with no extra access to unexported declarations unless the user
  grants `--allow-unexported`.
- **Each directory is optional to have, not to name.** A project without
  `src/` has no project modules, one without `assets/` has no assets, and
  one without `tests/` has no tests. The not-found import error and the
  asset error name the directory to create. None can be renamed or
  relocated.
- **Native code lives in a project clutch.** A project's C sources and their
  `[native]` table belong to a clutch in `clutch/`, indexed by
  `clutch/manifest.toml` and staged with
  `scripts/stage-native-clutches.sh --repository clutch`
  ([layered clutch repositories](2026-11-15-layered-clutch-repositories.md)).
  `src/` holds only Slug.
- **`slug.toml` stays configuration.** Layout has no keys in it, so it holds
  only values programs read through `cfg`.
- **Programs outside a project are unchanged.** With no `slug.toml` at or
  above the entry, the program's directory is the module root, as now, and
  it has no assets. A configured module root (`SLUG_FIXTURE_MODULE_ROOT`) is
  also used as given and has no project assets.
- **Entry programs are still named by path**, from anywhere in the project:
  `slug src/main.slug`, `slug tests/all.slug`.

## Consequences

Every Slug project reads the same way: configuration at the top, modules in
`src/`, embedded files in `assets/`, tests in `tests/`, native code in a
project clutch. Agents and people can rely on that
without reading a manifest. Orble moves `slug.toml` to its top level, keeps
its modules in `src/`, moves `src/tests.slug` and `src/tests/` to
`tests/`, and embeds its
`templates/` and `static/` files from `assets/`, so `build/orble.cslug`
carries them and runs from any directory.

A project becomes a clutch by copying: `src/`, `assets/`, and `tests/` keep
their paths, `clutch.toml` maps each module to its file, such as
`"orble.app" = { source = "src/orble/app.slug" }`, and its `[tests]` table
lists the files in `tests/`. Imports and asset names do not change; a
project clutch's native sources move with it. Listing every module and test
file is the remaining manual step; a tool can generate it from `src/` and
`tests/` later. A future `slug test` can find a project's tests in
`tests/` as it finds a clutch's through `[tests]`.

A project cannot keep its modules at its root, or put another language's
sources in `src/`. That is the cost of a fixed layout; native code has its
place in a project clutch. A program with no `slug.toml` still keeps
modules beside it, for scripts and small programs.

The desktop resolver gains a project assets root beside its clutch
repository, and `clutch_containing` keeps priority, so a module in a project
clutch never reads the project's assets.

Documents to update when implemented: `language/configuration.md` (the
project root section), `language/testing.md` (tests in a project), the assets section of
`reference/experimental-clutches.md` and of the language specification, and
the clutch assets decision's note about programs outside a clutch.

Tests: a program in `src/` and one in `tests/` importing from `src/`; a
`tests/` program importing a helper beside it; a module at the project root
not importable by name; a project without `src/`
naming it in the error; project assets from `src/` and from `tests/`; a
project clutch using its own assets, not the project's; a `.cslug` carrying
a project asset; a program with no `slug.toml` unchanged.

## Migration

A project whose modules sit beside its `slug.toml` must move them into
`src/`. None exists in this repository. Orble kept `slug.toml` in `src/`,
where its modules would resolve from `src/src/`, so it moved `slug.toml` to
its top level, its tests to `tests/`, and its served files to `assets/` when
this landed.
