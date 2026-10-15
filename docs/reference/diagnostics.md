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
| `frames` | Runtime call frames, innermost first, each `{function, location}`. Empty for other failures. Frames that only run deferred work are omitted. |
| `thrown` | Present only when `code` is `thrown`: the thrown payload in the [value JSON form](value-json.md). `throw nil` is reported as `"thrown": null`. |
| `native` | `{code, data}` when a native function failed: the native error code and its data in the value JSON form, or `null` data. Otherwise `null`. |
| `cause` | A nested diagnostic for an error that was replaced while unwinding, or `null`. |

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

## Interactive server

A failed interactive response carries the document as its `error` member. The
server's `protocol` category and its `protocol` and `host` codes belong to its
unstable protocol.

## Version history

Version 1, emitted only by the command line, also had a `kind` member and no
`native` member, and had no `protocol` category. Version 2 removes `kind`:
`code` identifies the failure class and `category` the stage. Interactive
diagnostics before version 2 used the categories `source`, `runtime`,
`protocol`, and `host`.
