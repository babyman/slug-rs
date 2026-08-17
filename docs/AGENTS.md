# Documentation Rules

`docs/` is the canonical home for Slug documentation. Keep each fact in one
authoritative document; link to it rather than copying it into another guide.

## Language documents

- `language/language-specification.md` owns semantics.
- `language/slug.ebnf` owns grammar.
- Focused language documents own detailed feature rules.
- `language/runtime-requirements.md` owns observable runtime guarantees.
- The support matrix reports implementation status and is never a substitute
  for the normative documents.

When changing source behavior, update the relevant language documents,
`language-support.tsv`, and tests in the same change. Regenerate the matrix
with `make docs-generate` and validate it with `make docs-check`.

## Writing rules

- State behavior, failure modes, and compatibility promises directly.
- Label a document as normative only when it defines required behavior.
- Keep examples valid against the rule they illustrate.
- Do not document an implementation detail as a language guarantee.
- Record non-trivial decisions in `decisions/`; do not use decision records as
  a task log.
