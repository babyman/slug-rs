# Clutch Assets Plan

This task list implements [Clutch Assets](../decisions/2026-10-25-clutch-assets.md):
files in a clutch's `assets/` folder, embedded at compile time by
`asset("name")`.

## Scope and invariants

- `asset` is a compile-time form. No VM instruction, runtime builtin, or
  `.cslug` format change is involved: an asset is a bytes constant.
- The frontend never touches storage. It validates the call and the name,
  then asks its resolver for the bytes; the desktop resolver owns the
  `assets/` folder, containment, and exact-case checks.
- A resolver that provides no assets (static, nil, closed-artifact hosts)
  refuses every request with a checked error.
- Interactive compilation and `compile` without a resolver have no assets.

## 1. Resolver contract

- [x] `ModuleResolver::asset(module, name)` in `slug-loader`, defaulting to a
  refusal, so existing resolvers keep compiling.
- [x] `ModuleHost` and the CLI's `DesktopLoader` forward it.

## 2. Desktop resolver

- [x] A module's clutch: the indexed clutch root that contains its canonical
  source path, so modules, test files, and programs in a clutch qualify.
- [x] Read `<root>/assets/<name>`: each path segment must match a directory
  entry exactly (case included), the file must be a regular file inside the
  folder after links resolve, and at most 16 MiB.
- [x] Errors name the clutch and suggest a differently-cased match.

## 3. Frontend

- [x] Name rule: non-empty `/`-separated segments, none `.` or `..`, no
  `\` or NUL.
- [x] Pre-pass over each module's syntax: every `asset(...)` call must have one
  positional string literal; a piped call is an error. Each named asset is
  loaded once, through the resolver, before analysis.
- [x] The checker types the call `bytes`; lowering emits its constant.
- [x] `invalid_asset` diagnostic code.

## 4. Library and adoption

- [x] `slug.net.http` exports `mediaType(name)` from a fixed extension table.
- [x] `slug.games.crawl` ships `assets/htmx.js` and its licence, and drops the
  generated `slug.games.crawl.htmx` module.

## 5. Tests and documentation

- [x] Module-loader tests: a clutch module, its test file, and a program in it
  read an asset; a nested name works; missing, wrongly-cased, escaping,
  non-literal, piped, and out-of-clutch uses fail with `invalid_asset`.
- [x] A compiled `.cslug` artifact runs with the asset embedded, without the
  clutch present.
- [x] Language specification, experimental-clutches reference, diagnostic
  codes, support matrix, and changelog.
