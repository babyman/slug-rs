# Diagnostic Documents

This document is normative. It defines the version 2 diagnostic document,
emitted by `slug --diagnostic-format=json` (see
[Errors](../language/errors.md#command-line-json-diagnostics)) and by the
interactive server in failed responses. Both emit the same document for the
same failure. See the [decision record](../decisions/2026-10-14-one-diagnostic-document.md).

## Shape

```json
{
  "version": 2,
  "category": "semantic",
  "code": "type_mismatch",
  "message": "operator `+` does not accept num and bool",
  "location": { "path": "example.slug", "line": 1, "column": 3 },
  "range": { "start": { "line": 1, "column": 1 }, "end": { "line": 1, "column": 9 } },
  "expected": null,
  "actual": null,
  "fixes": [],
  "suggestions": [],
  "frames": [],
  "native": null,
  "cause": null
}
```

Every member is always present, `null` when it does not apply, except
`thrown`.

| Member | Meaning |
|---|---|
| `version` | `2`. |
| `category` | The stage or service that failed: `parse`, `semantic`, `module`, `runtime`, `usage`, `io`, `host`, `artifact`, or `protocol`. |
| `code` | The stable failure class listed in [Diagnostic Codes](diagnostic-codes.md). Match on `code`, never on `message`. |
| `message` | Human-readable text. Not stable. |
| `location` | `{path, line, column}`: one point, or `null` when no source position is known. For an operator error it is the operator. |
| `range` | `{start, end}`, each `{line, column}`: the extent of the offending source in `location`'s file, with `end` exclusive. It starts at or before `location`. A parse error's range covers the offending token or malformed literal; a semantic error's range covers the offending expression. `null` for runtime and startup failures and for source errors whose extent is not tracked. |
| `expected` | What the parser or checker required at `location`, when known: a type such as `num`, or a token description such as `` `]` ``. Otherwise `null`. |
| `actual` | What it found instead, when known: a type such as `str`, or a token description such as ``name `x` `` or `end of input`. Otherwise `null`. |
| `fixes` | Machine-applicable fixes, each `{message, edits}`; see [Fixes](#fixes). Empty when no fix is certain. |
| `suggestions` | Likely corrections in the same form as `fixes`, which may change what the program means; see [Suggestions](#suggestions). Usually empty. |
| `frames` | Runtime call frames, innermost first, each `{function, location}`. Empty for other failures. Frames that only run deferred work are omitted. |
| `thrown` | Present only when `code` is `thrown`: the thrown payload in the [value JSON form](value-json.md). `throw nil` is reported as `"thrown": null`. |
| `native` | `{code, data}` when a native function failed: the native error code and its data in the value JSON form, or `null` data. Otherwise `null`. |
| `cause` | A nested diagnostic, or `null`: for an import of a module whose source does not compile, that module's `parse` or `semantic` diagnostic, with its own location, range, and fixes; otherwise an error that was replaced while unwinding. |

Lines and columns are 1-based. Columns count Unicode scalar values.

`expected` and `actual` are descriptive text. Types use the checker's type
display, and tokens use a backquoted spelling or a kind such as `number`.
Their wording may change; `code` remains the stable key. Currently a parse
error at a token reports `actual`, a missing delimiter or keyword also reports
`expected`, and a type mismatch against a single required type reports both.

## Fixes

Each fix has a human-readable `message` and a list of `edits`. Each edit is
`{range, text}`: replace the source in `range`, which uses the same positions
as the document's `range`, with `text`. An empty range inserts and an empty
`text` deletes. Edits apply to `location`'s file, against the text the
diagnostic was reported for, and all edits of one fix apply together.

A fix is offered only when applying it cannot change what the rest of the
program means. It may expose a further error, which can carry its own fix.
Currently two fixes exist:

| Code | Fix |
|---|---|
| `expected_syntax` | At the end of input, insert the missing closing delimiter `)`, `]`, `}`, or `\|}` after the last token. The parser has already read everything after the opening delimiter as inside it, so closing it there completes the program as parsed. |
| `unreachable_match_case` | Delete the unreachable case, from its first pattern to the end of its body. Offered only when its patterns evaluate nothing when tested: no pinned names and no computed map keys. |

Rust hosts apply a fix with `slug_frontend::apply_fix`.

## Suggestions

A suggestion has the same `{message, edits}` form as a fix, but is only
likely: applying it may change what the program means, so review it first.
Its `edits` may be empty when the corrected source cannot be located. These
kinds exist:

| Code | Suggestion |
|---|---|
| `invalid_number` | For an integer literal larger than 9223372036854775807 whose value a float would round, appending `.0` to make it a float; the message gives the rounded value. When the float holds the value exactly, the edit is a fix instead. |
| `module_not_found` | Each available module whose name is close to the requested one, nearest first, at most three: one with the same last segment, such as `slug.io.stdin` for `slug.stdin`, or else one within a small edit distance, such as `utils` for `utlis`. The desktop host considers installed clutch modules and the `.slug` files in each searched directory. When the `import` names the module with a plain string literal, the diagnostic's `location` and `range` are that literal and each suggestion replaces it. |

## Text form

Without `--diagnostic-format=json`, the command line renders the same document
as text. Its first line is `slug: <kind> error: <message>`, where `<kind>` is
`parse`, `semantic`, or `runtime` (for `runtime` and `module` diagnostics), or
`slug: <message>` for startup failures. The following lines show, when present:

- `--> path:line:column`, then the source lines ending at that line with the
  range underlined, or the column marked, when the source can be read;
- each frame as `in <function> at path:line:column`;
- `= code: <code>`;
- `= expected <expected>, found <actual>`, or either alone;
- `= native: <code> <data>`;
- each fix as `= fix: <message>` and each suggestion as
  `= suggestion: <message>`, each followed by its edits as
  `at line:column[-line:column]: insert|replace with|delete`;
- the `cause`, after a blank line, rendered the same way with `caused by:` in
  place of `slug:`.

A `thrown` payload appears in the message. The text form is for people and is
not a stable contract; tools read the JSON form.

## Interactive server

A failed interactive response carries the document as its `error` member. The
server's `protocol` category and its `protocol` and `host` codes belong to its
unstable protocol.

## Version history

Version 1, emitted only by the command line, also had a `kind` member and no
`native` member, and had no `protocol` category. Version 2 removes `kind`:
`code` identifies the failure class and `category` the stage. Interactive
diagnostics before version 2 used the categories `source`, `runtime`,
`protocol`, and `host`. Version 2 added `suggestions` after its first release;
documents without it have no suggestions.
