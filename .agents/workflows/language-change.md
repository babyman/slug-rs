# Language Change Workflow

Use this workflow for any change to Slug source syntax, semantics, diagnostics,
or runtime behavior visible to a Slug program.

## 1. Establish the rule

Read the applicable document in `docs/language/`. If no document owns the rule,
write or extend a Markdown specification before relying on implementation code
as the sole definition. State the valid programs, result, and invalid cases.

If the documents leave a Slug-visible outcome open, establish the selected rule
in its owning normative document before implementation. Create a decision
record when the choice meets `docs/decisions/README.md`; the record explains
the rationale, but does not replace the normative requirement. Do not create a
record merely to repeat behavior the existing documentation already decides.

## 2. Identify the implementation surface

- `src/source.rs` owns lexing, parsing, and source-to-bytecode compilation.
- `src/bytecode.rs` owns VM-internal instruction representation.
- `src/vm.rs` owns execution and runtime error construction.
- `src/value.rs` owns language values and value operations.
- `src/main.rs` owns the public command-line boundary and error presentation.

Keep semantic rules out of the CLI where they can be represented by the source
compiler or VM.

## 3. Implement with tests

Add or update the smallest focused test first:

- Use `tests/vm.rs` for bytecode execution, frames, globals, captures, and
  source-span behavior.
- Use `tests/cli.rs` for accepted source, output, exit status, and rendered
  diagnostics.

An error-path change needs a regression assertion that checks the returned
Slug error or public CLI diagnostic instead of a host panic.

## 4. Synchronize the language record

Update all applicable artifacts in the same change:

- `docs/language/slug.ebnf` for grammar changes.
- `docs/language/language-specification.md` for language-wide semantics.
- A focused note in `docs/language/` for the feature's detailed rule.
- `docs/language/runtime-requirements.md` for observable runtime guarantees.
- `README.md` when the implemented subset or public usage changes.

Update `docs/language-support.tsv`, regenerate the support matrix, and create
a decision record when the decision meets `docs/decisions/README.md`.

## 5. Verify and hand off

Run the focused test during development, then run `make check`. Record the
commands actually run and any intentionally unrun validation in the handoff.
