# Test data

These files are the clutch's test fixtures; no module embeds them.

- `commonmark-0.31.2.json`: the examples of the CommonMark Spec 0.31.2
  (https://spec.commonmark.org/0.31.2/spec.json), unchanged. The spec is
  by John MacFarlane, under CC-BY-SA 4.0.
- `gfm-0.29-extensions.json`: the table, strikethrough, and autolink
  extension examples of the GitHub Flavored Markdown Spec 0.29-gfm
  (https://github.com/github/cmark-gfm, `test/spec.txt`), extracted to the
  same JSON form, with `→` written as a tab. Under CC-BY-SA 4.0.
- `commonmark-deviations.json`: this clutch's own: the output it gives for
  the CommonMark examples it renders differently by design, with the
  reason, `raw-html` or `extended-autolink`.
