# Nominal types implementation checklist

This is the dependency-ordered implementation checklist for the adopted
nominal-resource, fieldless-enum, and transparent-alias design. It does not
define language semantics; [Nominal types](../language/nominal-types.md) and
the language specification remain authoritative.

## Cross-cutting rules

- Keep nominal identities private to the compiler, VM, and native boundary;
  they are not portable bytecode or Rust API commitments.
- Preserve checked source and runtime errors. No invalid declaration, native
  mismatch, or malformed private bytecode may panic the host.
- Add CLI tests for source syntax and diagnostics, module-loader tests for
  exported/imported identity, and VM tests only for new runtime values or
  bytecode behavior.
- At each completed slice, update the EBNF, normative language record, support
  matrix, README capability statement, changelog, and applicable decision
  record; finish with `make check`.

## 1. Nominal resource handles

### Complete

- [x] Parse top-level `resource Name` declarations and reject nested forms.
- [x] Remove the broad `resource` source annotation.
- [x] Retain a nominal resource identity in source signatures and callable
  resolution.
- [x] Export resource type metadata into imported module snapshots; resolve
  `fs.File` type annotations.
- [x] Migrate `slug.io.fs` and the C-FFI source fixtures to named handles.
- [x] Prove that incompatible nominal resource parameters are rejected.

### Remaining

- [x] Reject duplicate resource declarations and conflicts between exported
  type names and other exported type declarations.
- [x] Require each `resource Name` declaration to match exactly one native
  resource registration owned by that module; reject missing, extra, or
  differently named registrations before module execution.
- [x] Validate declared foreign resource arguments and results at the native
  boundary, including dynamic call paths and callbacks returning the wrong
  resource kind.
- [x] Add typed whole-case resource constraints and lower them to checked
  private match metadata.
- [x] Decide and implement resource metadata visibility without adding a
  runtime value or constructor for a resource type.
- [x] Add focused negative tests for duplicate declarations, unknown imported
  types, wrong native result kinds, wrong dynamic arguments, and closed handles.

## 2. Shared nominal identity infrastructure

- [x] Extract the duplicated schema/resource identity mechanics into one
  private nominal-identity representation with stable declaring-module and
  declaration-name components.
- [x] Represent exported types separately from runtime value exports in module
  snapshots and loader metadata.
- [x] Support qualified type paths through imported module bindings, including
  deterministic diagnostics for unknown module bindings, unknown type members,
  and non-module prefixes.
- [x] Preserve nominal identity through aliases, collection inference, unions,
  callable signatures, imported overload snapshots, and module caching.
- [x] Audit union normalization, type display, overload specificity, and match
  coverage so nominal identity is compared by identity rather than spelling.

## 3. Fieldless enums

### Declaration and namespace

- [ ] Lex and parse top-level `enum Name { Case, ... }` declarations.
- [ ] Reject nested declarations, duplicate enum names, duplicate cases, empty
  or malformed case lists according to the normative grammar, and collisions
  with enum value namespaces.
- [ ] Introduce a nominal `Enum` type with identity and its closed ordered case
  set in semantic/module metadata.
- [ ] Compile an enum declaration to an immutable enum namespace value while
  retaining its type only in the type namespace.
- [ ] Export enum type metadata and its namespace value; preserve both across
  normal imports and module caching.

### Values and expressions

- [ ] Add a private runtime enum value carrying enum identity and case identity.
- [ ] Resolve `SeekFrom.Start` and imported `options.SeekFrom.Start` as enum
  values; reject bare `Start`, missing cases, and selection from non-enum
  values with checked source errors.
- [ ] Define enum display/debug output and equality: only the same enum
  identity and case compare equal.
- [ ] Keep enums distinct from strings, numbers, structs, maps, and resources;
  add no casts, integer discriminants, flags, payloads, or constructors.

### Types and matching

- [ ] Resolve local and imported enum types in annotations, unions,
  collections, channels, parameters, and returns.
- [ ] Parse qualified enum case patterns without treating the first name as a
  binding; lower them to private match patterns with no accidental capture.
- [ ] Make enum types runtime-checkable in whole-case constraints.
- [ ] Extend closed-match coverage to enumerate all enum cases, respecting
  guards and existing unreachable/disjoint diagnostics.
- [ ] Prove local/imported equality, mismatch, qualification, exhaustive and
  non-exhaustive matches, guarded cases, and dynamically unknown fallbacks.

## 4. Transparent aliases

### Declaration and resolution

- [ ] Lex and parse top-level `type Name = Annotation` declarations.
- [ ] Build a separate alias table in the compile-time type namespace; aliases
  create neither a runtime value nor a nominal identity.
- [ ] Resolve aliases transitively before type assignability, union
  normalization, overload identity, and runtime-match lowering.
- [ ] Detect direct and indirect cycles with a source-located cycle chain.
- [ ] Reject generic aliases and aliases that require unsupported forward or
  cross-module recursive resolution until separately designed.

### Modules and semantics

- [ ] Export/import aliases through qualified paths such as `paths.Path`.
- [ ] Preserve the underlying resource, enum, schema, and collection identity
  when an alias crosses a module boundary or is locally renamed.
- [ ] Prove aliases are transparent in declarations, calls, overload selection,
  collections, union deduplication, typed matches, and schema `struct<S>`
  references.
- [ ] Prove aliases do not act as constructors or strong typedefs and do not
  change runtime display or equality.

## 5. Final integration audit

- [ ] Update native ABI documentation and registration APIs to name declared
  resource types and expose checked enum-case access where foreign APIs need it.
- [ ] Audit public library declarations and C fixtures for retired broad
  `resource` usage.
- [ ] Add conformance fixtures for public syntax, module imports, diagnostics,
  and observable enum behavior.
- [ ] Regenerate documentation output and run `make check`; run the
  feature-gated FFI prototype suite when native-resource validation changes.
