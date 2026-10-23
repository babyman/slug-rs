# Clutches declare their test modules

## Status

Accepted

## Context

A test runner written in Slug, run as `slug --allow-unexported test
./path/to/foo.clutch`, needs a clutch's test files. Slug cannot list
directories, and `import` takes module names, not paths, so the runner can
find tests only through the clutch's manifest, which it reads with
`slug.fmt.toml`, and import them only by name.

## Decision

A clutch's `clutch.toml` may list its test files:

```toml
[tests]
files = ["tests/string.slug"]
```

- A test file's module name is the clutch's name, its directory name without
  `.clutch`, then the file's path without `.slug`, with `/` as `.`, so the
  manifest needs no names and the runner derives the same ones.
- Test files are paths inside the clutch to `.slug` files, and their names
  must be valid and must not match a provided module.
- A test module resolves only for clutches in the repository index, and only
  when the user grants `describe.unexported`, the permission that lets it see
  its clutch's unexported declarations. Without it, importing one says so.
- A test file's existence is checked when it is imported, as a module source
  is, so a broken test entry never stops other programs from loading.

## Consequences

### Positive

- Tests ship inside the clutch, beside the modules they check, and need no
  directory listing to find.

### Negative

- Only installed clutches' tests can run; a clutch elsewhere on disk must be
  installed first.

## Migration

None.
