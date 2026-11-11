# Clutch Assets

Status: Accepted

## Context

A clutch sometimes needs to ship files that are not Slug source: the web
front end in `slug.games.crawl` serves htmx, and a game ships images or a
font. The only way to carry one was to wrap it in a Slug string module by
hand, which needs a raw string that the file never contains, a regeneration
step for each update, and text that is not the vendored file byte for byte.

A program cannot read such a file from disk when it runs: a `.cslug`
artifact and ESP32 firmware never resolve source, clutches, or files, and a
library module should not need filesystem access to read its own data.

## Decision

A file in a clutch's `assets/` folder is an asset of that clutch, with no
manifest entry. `asset("name")` is a compile-time form, recognised by name
like `import`, whose one argument is a string literal: a relative path in
`assets/` written with `/`, such as `"htmx.js"` or `"images/foo.png"`. The
compiler reads the file through the host's module resolver and the call
evaluates to its contents as `bytes`, a constant of the compiled module.

- Only a module whose source file lies inside an indexed clutch, including
  that clutch's test files and programs, can name assets, and only its own
  clutch's. Another clutch's assets are reached through that clutch's
  exports.
- An invalid name, a name that is not a file in `assets/` with exactly that
  spelling (case included), a file outside the folder through a link, and a
  call that is not exactly one string literal are compile errors with the
  code `invalid_asset`.
- The media type of an asset served over HTTP comes from its name's
  extension, through `slug.net.http`'s `mediaType(name)`, not from the clutch
  format.

## Consequences

Vendored files ship unchanged, and a closed `.cslug` artifact or firmware
image carries each asset its modules name as an ordinary bytes constant, so
the artifact format does not change. Files nobody names are never read.
Because nothing lists a clutch's assets, a packaging tool learns which ones
a program uses from compilation rather than from `clutch.toml`.

Like `import`, the form is recognised by name, so a local binding named
`asset` does not change what `asset("...")` means. Programs outside a clutch
have no assets; that needs a project manifest, which does not exist yet.
Assets are read once per compilation; a running program does not see a file
that changes after its module compiled.

## Migration

None. `slug.games.crawl` replaces its generated `slug.games.crawl.htmx`
module with `assets/htmx.js`.
