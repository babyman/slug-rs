# Module not found suggestions, and text from the diagnostic document

## Status

Accepted

## Context

Agents guess module names from what a module is called rather than where it
sits: in one JSON-library evaluation run, an agent imported `slug.stdin` four
times before finding `slug.io.stdin`. The failure shared the `module` code
with every other module failure, pointed at `import` rather than the name, and
named nothing that exists. Separately, text diagnostics showed only the
message, an excerpt, and frames, so the JSON form's code, expected and actual
values, and fixes were invisible to anyone reading text. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

- A missing module has the runtime code `module_not_found`, which is also the
  `type` `defer onerror` sees. Other module failures keep `module`.
- `ModuleLoadError::NotFound` carries suggestions chosen by the resolver,
  through the shared `suggest_module_names`: names with the same last segment
  when there are any, otherwise names within an edit distance that grows with
  the last segment's length, at most three. The desktop host considers clutch
  modules and the `.slug` files in each searched directory, without walking a
  source tree.
- The diagnostic document gains `suggestions`, in the same form as `fixes`.
  A suggestion may change what the program means, so it is kept apart from
  `fixes`, which must not. Not-found suggestions replace the module name.
- Bytecode spans are points, so the host finds the name's range by lexing the
  importing file at the `import` call when it reports the error, and moves the
  location to the name. Without readable source, suggestions carry no edits.
- Text output renders the diagnostic document itself, so text and JSON report
  the same members. The headline is unchanged; the code, expected and actual
  values, native code, fixes, suggestions, and cause follow the excerpt.

## Consequences

### Positive

- A guessed module path names the module that exists, as an applicable edit.
- Text readers see everything a JSON reader sees.

### Negative

- Text diagnostics are longer: every one gains a `= code:` line.
- Handlers matching `type == "module"` for a missing module now see
  `module_not_found`.

### Neutral

The interactive server reports the code and suggestions but has no source to
locate the name, so its suggestions carry no edits. A missing module imported
by another module still reports as that module's `module` failure.

## Migration

Match `module_not_found` where code matched `module` for a missing module.
