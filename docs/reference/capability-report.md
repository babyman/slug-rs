# Capability Report

This document is normative. It defines the version 1 capability report: a
structured answer to "what can this program touch outside its own Slug code?"
See the [decision record](../decisions/2026-10-14-capability-report.md) for
rationale.

## Command

```sh
slug report program.slug
slug report program.cslug
```

The command writes one JSON document to standard output and exits `0`. A
`.cslug` path reports a compiled artifact; any other path reports source and
its static imports, resolved as `slug program.slug` would resolve them. A
missing path, an extra argument, or a failure to read, resolve, compile, or
decode the program writes a startup diagnostic to standard error and exits `1`.
`--diagnostic-format=json` before `report` selects JSON diagnostics as for
other commands.

Reporting compiles the program but never runs it.

## Document

```json
{
  "version": 1,
  "entry": "app.slug",
  "closed": false,
  "modules": [
    {
      "name": "app.slug",
      "imports": ["db"],
      "dynamic_imports": [{ "line": 4, "column": 13 }],
      "builtins": ["cfg", "println"],
      "foreign": [],
      "resources": []
    },
    {
      "name": "db",
      "imports": [],
      "dynamic_imports": [],
      "builtins": [],
      "foreign": [
        {
          "name": "open",
          "exported": true,
          "arity": { "min": 1, "max": 2 },
          "parameters": [
            { "name": "path", "type": "str", "variadic": false, "default": false },
            { "name": "mode", "type": "str", "variadic": false, "default": true }
          ],
          "result": "Handle",
          "tags": ["stable"]
        }
      ],
      "resources": [{ "name": "Handle", "exported": true, "tags": ["native"] }]
    }
  ]
}
```

| Member | Meaning |
|---|---|
| `version` | Always `1` for this format. |
| `entry` | The program path exactly as given on the command line. |
| `closed` | `true` for a compiled artifact. Its import table is closed, so `modules` lists every module the program can load. `false` for source, which can import modules the report does not list through `dynamic_imports`. |
| `modules` | Every module in the program, the entry first and then in import discovery order. Each module appears once. |

Each module has these members:

| Member | Meaning |
|---|---|
| `name` | The logical name used to import it. The entry module is named by `entry`. |
| `imports` | Module names this module passes as string literals to `import`, sorted and without duplicates. |
| `dynamic_imports` | The line and column of each `import` call with an argument that is not a string literal, in source order. Present only when `closed` is `false`. |
| `builtins` | Names this module references that are supplied implicitly by the host or the `slug.builtin` module, such as `println`, `cfg`, or `Error`, sorted. |
| `foreign` | The module's top-level `foreign` declarations, in declaration order. |
| `resources` | The module's top-level `resource` declarations, in declaration order. |

Each `foreign` entry has `name`, `exported`, `arity`, and `tags`. `arity.min`
counts parameters without defaults; `arity.max` is `null` for a variadic
declaration. A report built from source also has `parameters`, each with
`name`, `type` (the annotation's canonical text, or `null`), `variadic`, and
`default`, and `result` (the result annotation, or `null`). A compiled artifact
does not retain these details, so its entries omit both members.

Each `resource` entry has `name`, `exported`, and `tags`.

`tags` lists tag names in declaration order. Tag arguments are not reported.

## Comparing reports

A report depends only on the program and the modules it resolves, not on the
machine, so two reports of the same program are equal. To review a change,
compare the reports of the two versions. JSON object member order carries no
meaning; array order is significant.

## Not reported

Version 1 does not report which native provider or clutch supplies a foreign
declaration, configuration keys read through `cfg`, tag arguments, or resource
limits.
