# Safe Markdown rendering in Slug

## Status

Accepted (2026-11-16). Prompted by Orble's request for a maintained
Markdown renderer and HTML sanitization boundary for signed posts
(`../orble/docs/slug-markdown-requirements.md`).

## Context

Orble renders user-written Markdown into server-produced pages. It needs a
documented dialect, raw HTML disabled with its treatment documented, an
output policy that excludes executable elements, event attributes, and
unsafe URLs (including encoded and control-character schemes), a way to
keep a private feed from fetching third-party images, no I/O while
rendering, checked failures with documented limits, and the same behavior
from source and compiled programs.

The usual route is a native parser (cmark or md4c) followed by an HTML
sanitizer. Neither native parser's safe mode supports the URL policy Orble
needs (cmark's allows `data:` images and has no way to refuse external
images), so the policy would be our code either way, in C, built and staged
for every platform.

## Decision

`slug.text.markdown`, a clutch written in Slug with no native code,
exports `toHtml(source, options)`.

- The dialect is CommonMark 0.31.2 with GitHub's tables, strikethrough,
  and extended autolinks (`www.`, `http(s)://`, `ftp://`, `mailto:`,
  `xmpp:`, and bare email addresses, as cmark-gfm finds them today). The
  parser is a port of cmark's block and inline algorithms, delimiter and
  bracket stacks included, so its time is linear in the input.
- Raw HTML is text. HTML blocks are not recognized, so their lines are
  paragraph text; an inline tag, comment, declaration, processing
  instruction, or CDATA section is recognized as CommonMark defines it and
  shown escaped, with no Markdown read inside it. Showing the markup, rather
  than dropping it as cmark's safe mode does, keeps what the author wrote
  visible.
- No sanitizer: the renderer builds every element itself, from a fixed list
  of elements and attributes, and escapes every text and attribute value.
  Input HTML never reaches the output, so there is nothing to sanitize.
- URLs are classified as a browser would read them: after Markdown's
  escapes and entities are resolved, leading and trailing C0 controls and
  spaces are removed, tabs and line endings are removed everywhere, any
  other control character refuses the URL, and the scheme is the WHATWG
  scheme syntax before the first `:`. A path starting with two slashes or
  backslashes is a network-path reference to another host. `links` and
  `images` options list the allowed schemes and `"relative"`; the defaults
  are relative, `http`, `https`, and `mailto` for links and relative only
  for images, so a page fetches no third-party images unless asked to.
  `javascript`, `vbscript`, `data`, and `file` cannot be allowed. A refused
  link renders as its text and a refused image as its description.
- Limits are options with defaults: `maxBytes` (65536) for the source in
  UTF-8 bytes, and `maxDepth` (32), separately for block containers and for
  inline elements. Over either, `toHtml` throws `markdown.limit` with
  `data` `{limit, max}`; bad options throw `markdown.options`. Nothing else
  fails.

## Consequences

The renderer runs wherever Slug runs, needs no staging, and its source and
compiled behavior are the same code. The clutch's tests run the 652
CommonMark examples and GitHub's 21 extension examples; the 77 where this
dialect differs by design (72 raw HTML, 5 extended autolinks) have their
expected output in `assets/commonmark-deviations.json`. Hostile input is
checked against an allowlist of the output, and cmark's pathological inputs
run in linear time. `make test-clutches` runs the tests from source and,
compiled, from a `.cslug`.

Speed is that of Slug code: about 45 ms for 60 KB of prose in a release
build, and a few hundred ms for 64 KB of pathological input. Native string
searches (`slug.bytes.find`/`findAny`) carry the per-character scans for
ASCII text.

The parse tree is not exported. A later need (rendering to something other
than HTML, or caching parses) can export it once its shape is worth
promising.

## Migration

None.
