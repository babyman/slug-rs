# Slug Language Specification

## Status and scope

This document is the normative specification of the Slug language. It defines
the source-language behavior that a conforming Slug implementation must expose.
It does not prescribe a parser, bytecode format, object representation,
scheduler, or host-language implementation.

This edition specifies the source-language core, module system, metadata and
foreign declarations, optional static checking, error behavior, and structured
concurrency. Library APIs are specified in their individual reference pages.

The developer's guide teaches the language and architectural decision records
explain why a decision was made. Neither is normative. The accompanying
[Runtime Requirements](runtime-requirements.md) specify the observable
execution requirements behind this language specification.

## Conformance and sources

A conforming implementation accepts programs described by this specification
and produces the specified observable behavior. Until every section has a
dedicated conformance case, the repository's tests are the executable evidence
for currently supported behavior:

- grammar: [`slug.ebnf`](slug.ebnf);
- accepted syntax and error behavior: `../../tests/conformance/legacy-syntax`;
- language-wide behavior: `make test`.

If an implementation, a test, and this specification disagree, treat that as a
documentation or implementation defect. Do not infer a language guarantee from
an implementation detail alone.

## Notation

The grammar uses the following EBNF conventions:

- `"token"` is a literal token.
- `A , B` means `A` followed by `B`.
- `A | B` means one alternative.
- `[ A ]` is optional.
- `{ A }` repeats zero or more times.

Unless stated otherwise, source examples use a newline as a statement
separator. Semicolons are also statement separators.

## Program structure

A program is a sequence of statements. A statement may be a declaration, a
control statement, a deferred expression, or an expression whose value is
ignored by the surrounding program.

```ebnf
program       = { statement_sep } , { statement , { statement_sep } } , EOF ;
statement_sep = ";" | NEWLINE ;
```

Newlines separate statements only where the lexer and parser permit automatic
statement termination. Authors should use parentheses or a continued operator
expression when a value spans lines.

Blocks are expressions containing statements:

```ebnf
block = "{" , { statement_sep } , { statement , { statement_sep } } , "}" ;
```

The value of a block is the value of its final expression. An empty block has
the value `nil`.

## Values and literals

Slug has the following literal value forms:

| Form | Examples |
|---|---|
| Nil | `nil` |
| Booleans | `true`, `false` |
| Numbers | `42`, `1_000`, `0x10` |
| Strings | `"slug"`, `'raw text'` |
| Bytes | `0x"414243"`, `0x""` |
| Lists | `[1, 2, 3]` |
| Maps | `{name: "Slug"}` |
| Functions | `fn(x) { x + 1 }` |

Strings are Slug's only textual value type. The language does not expose a
separate symbol or atom value; implementations may still intern identifiers
internally.

Bytes use pairs of hexadecimal digits. `0x""` is the empty bytes value; a
non-empty bytes literal must contain a whole number of pairs.

Bytes support integer indexing, list-style slices, and concatenation with
other bytes. Indexing returns a number from `0` through `255`; slicing and
concatenation return bytes.

```slug
val bytes = 0x"020304"
bytes[0] == 2
bytes[1:] == 0x"0304"
0x"0102" + 0x"0304" == 0x"01020304"
```

Numbers may contain underscore separators. A bare identifier or quoted string
used as a map key is a string key, so `{name: "Slug"}` and
`{"name": "Slug"}` are indexed with `["name"]`. A bracketed map key evaluates
an expression instead:

```slug
val key = "name"
val byName = {name: "Slug"}
val byQuotedName = {"name": "Slug"}
val byValue = {[key]: "Slug"}
```

## Bindings, scope, and assignment

`val` creates an immutable binding. `var` creates a binding that may later be
assigned. Both forms accept a match pattern on their left side. An identifier
binding may have a type annotation.

```ebnf
val_expr = "val" , match_pattern , [ ":" , type_annotation ] , "=" , expression ;
var_expr = "var" , match_pattern , [ ":" , type_annotation ] , "=" , expression ;
assignment = identifier , "=" , assignment | logical_or ;
```

Bindings are lexical. A name resolves to its nearest enclosing binding. An
assignment changes an existing `var` binding and is invalid for a `val` binding
or an unknown name. It evaluates to the value assigned, so an assignment may be
the final expression of a function body or appear inside another expression.

At module top level, `{*}` may be used as a declaration pattern. It requires a
map with string keys and creates one top-level binding per entry. This is the
module-import selection form: `val {*} = import("slug.std")`.

```slug
var count = 0
count = count + 1

val next = (count = count + 1)

val label = "requests"
```

## Expressions and operators

Slug is expression-oriented. Declarations, conditionals, blocks, matches, and
function literals can appear where an expression is expected.

Operator precedence, from lowest to highest, is:

| Operators | Associativity |
|---|---|
| `=` | right |
| `||` | left |
| `&&` | left |
| `==` `!=` | left |
| `<` `<=` `>` `>=` | left |
| `|` `^` `&` | left |
| `<<` `>>` | left |
| `+` `-` | left |
| `*` `/` `%` | left |
| `:+` | left |
| `+:` | right |
| prefix `!` `-` `~` | right |
| pipeline `/>` | left |
| calls, indexing, dot access, struct initialization and copy | left |

Bitwise operators (`&`, `|`, and `^`) accept two integers or byte operands.
When either operand is bytes, an integer from `0` through `255` becomes a
one-byte value and the result is bytes. A shorter non-empty bytes operand
repeats to match the longer operand; an operation with empty bytes returns
empty bytes. `~` accepts either integers or bytes and complements every byte
of a bytes operand. Shifts (`<<`, `>>`) accept integers only. A shift count
must be an integer from `0` through `63`; invalid operand or shift-count
combinations are checked runtime type errors. Right shifts are arithmetic: they
preserve the sign of a negative integer.

```slug
0x"ff00" & 0x"0ff0" == 0x"0f00"
0x"ff" ^ 0x"0000" == 0x"ffff"
255 & 0x"0ff0" == 0x"0ff0"
```

`+` concatenates two lists or two byte values into a new value of the same
collection type. It also concatenates a string with any value, converting the
right operand to its display form. The directional collection operators also
produce new values: `collection :+ value` appends one value, while
`value +: collection` prepends one value. `+:` is right-associative, so
`1 +: 2 +: bytes` prepends `1` and then `2` to `bytes`. They accept lists or
bytes; a bytes element must be an integer from `0` through `255` or a one-byte
`bytes` value. Other collection operands produce a checked runtime type error.

```slug
"list of two + " + 1 == "list of two + 1"
"items: " + [1, 2] == "items: [1, 2]"
"data: " + {status: "ok"} == "data: {\"status\": \"ok\"}"
1 +: 0x"0203" + 0x"04" :+ 5 == 0x"0102030405"
0x"01" +: 0x"0203" + 0x"04" :+ 0x"05" == 0x"0102030405"
1 +: 2 +: 3 +: 0x"" == 0x"010203"
```

`string * count` repeats a string `count` times. The count must be a
non-negative integer. Negative, non-integer, or counts whose result cannot be
represented or reserved produce checked runtime type errors.

`if` is an expression. Its condition is parenthesized, and both branches are
blocks or nested `if` expressions:

```slug
val max = fn(a, b) {
  if (a > b) { a } else { b }
}
```

The pipeline operator passes its left value as the first argument to the
expression on its right. Therefore `x /> f(y)` is equivalent to `f(x, y)`.
When the right expression is a `match` without an explicit subject, the left
value becomes that match's subject instead of a function argument.

```slug
val double = fn(n) { n * 2 }
val result = 10 /> double
```

### Pipeline match expressions

Use `value /> match { ... }` to classify or destructure a pipeline value. The
`match` on the right MUST omit its normal subject expression: the pipeline
supplies it. This is equivalent to `match value { ... }` and the selected case
result may continue through the pipeline.

```slug
val describe = fn(value) {
  value /> match {
    [] => "empty"
    [head, ...] if head > 0 => "positive head"
    _ => "other"
  }
}

val first = [1, 2, 3]
  /> match { [head, ...] => head }
  /> double
```

`value /> match other { ... }` is invalid because it supplies two subjects.
This form differs from `fn(...) match { ... }`: a pipeline match receives the
value to its left, while a function match body constructs its subject from the
function's already-bound parameters.

## Functions and calls

Functions are first-class values. A function has a parameter list and either a
block body or a pattern-matching body. Functions and foreign declarations may
declare generic type parameters, and declarations, parameters, returns, and
struct fields may have type annotations.

```ebnf
function_literal = "fn" , [ type_parameters ] , "(" , [ parameters ] , ")" ,
                   [ ":" , type_annotation ] , ( block | match_body ) ;
type_parameters  = "<" , identifier , { "," , identifier } , ">" ;
parameter        = { tag } , [ "..." ] , ( identifier | "_" ) ,
                   [ ":" , type_annotation ] , [ "=" , expression ] ;
match_body       = "match" , "{" , [ match_case , { case_sep , match_case } ] , "}" ;
```

Parameters may have default expressions. A variadic parameter uses `...` and
must be final. Calls accept positional, named, and spread arguments:

`_` is a discard parameter. It accepts its positional argument but does not
introduce a name into the function body. Multiple discard parameters are
allowed. Because it has no binding name, a discard parameter cannot be supplied
by a named argument.

```ebnf
call_arg  = spread_expr | named_arg | expression ;
spread_expr = "..." , expression ;
named_arg = identifier , "=" , expression ;
```

Named arguments use `=`, not `:`.

```slug
val greet = fn(name, title = "Mx") { "Hello $title $name" }

greet("Slug")
greet(name = "Slug", title = "Dr")
```

Positional arguments bind from left to right. After a named argument, every
remaining argument must be named. A parameter may be assigned only once;
unknown names, duplicate assignments, too many positional arguments, and a
missing parameter without a default are errors. Excess positional values bind
to the final variadic parameter. A named value for a variadic parameter must be
a list. Default expressions evaluate in the function's defining module
environment, rather than the caller's environment.

### Function match bodies

A function may use `match` directly after its parameter list and optional
return type instead of writing a block. This is a function match body, not an
ordinary `match expression`: it has no explicit subject expression. The runtime
constructs its subject from the arguments after ordinary positional, named,
default, and variadic binding has completed.

- With exactly one declared parameter, the subject is that parameter's value.
- With zero parameters or two or more parameters, the subject is a list of the
  declared parameter values in declaration order.
- A variadic parameter is already a list. Thus `fn(first, ...rest) match`
  matches `[first, rest]`, while `fn(...rest) match` matches `rest` directly.

Cases use the ordinary pattern, guard, result, and first-match rules. Generic
parameters and parameter or return annotations have the same meaning as for a
block-bodied function. `recur(...)` remains valid in a tail-position case
result.

```slug
val differenceFromMean = fn(xs:list<num>, mean:num):num match {
  [[], _] => 0
  [[head, ...], mean] => head - mean
}

differenceFromMean([5, 8], 3) // 2
```

## Collections, access, and structs

Lists and maps support indexing. List indices may be negative. List slice syntax
is `[start:end]`, with an optional `:step`. The start, end, and step expressions
are each optional: an omitted start is `0`, an omitted end is the list length,
and an omitted step is `1`. Negative slice bounds count from the end of the
list. Bounds outside the list are clamped to its limits, and a step must be a
positive integer. Slicing produces a new list; maps and structs cannot be
sliced.

```slug
val xs = [10, 20, 30, 40]
xs[1]
xs[-1]
xs[1:3]
xs[:1]
```

Dot access is shorthand for string-key map access where supported:

```slug
val user = {name: "Slug"}
user.name
user["name"]
```

A struct expression defines a schema and produces a value of type `schema`.
Applying a schema to `{...}` creates a struct value. When the schema expression
is a direct, statically known binding `S`, construction has type `struct<S>`;
otherwise it has the less precise type `struct`. Struct fields can have type
annotations and defaults. `copy`
creates a value with replacement fields. Tags on struct fields are not syntax.

With `-type-check`, a directly known schema binding also checks supplied and
required fields and their values. Direct string field access and copies through
a known `struct<S>` use the declared field types. Aliased and imported schema
bindings retain this precision; dynamically selected schemas remain generic
`struct` values and use their ordinary checked runtime behavior.

In a `struct<S>` annotation, `S` must resolve lexically to a schema binding.
Its nominal identity is the schema value's identity, not the spelling of `S`:
an alias denotes the original schema, and a later shadowing binding does not
change the meaning of an already established `struct<S>` type.

```slug
val User = struct {
  name:str,
  active = true,
}

val first = User { name: "Slug" }
val second = first copy { active: false }
```

Each evaluation of a struct expression creates a distinct schema identity.
Fields retain declaration order and names must be unique. Default expressions
evaluate once, in source order, when the schema expression is evaluated.
Construction rejects unknown or duplicate fields and missing required fields.

Schema values compare by identity. Struct values compare equal only when they
have the same schema identity and equal field values in schema order. Dot access
and string-key bracket access read fields; invalid or unknown field access is a
runtime type error. The [Structs](structs.md) supplement defines the detailed
rule and current implementation boundary.

## Pattern matching

`match` selects the first matching case. A case may have a guard introduced by
`if`. Pattern alternatives in one case are allowed only when they do not create
bindings. If the subject is a map literal, it must be parenthesized to avoid
being read as the case block: `match ({name: "Slug"}) { {name} => name }`.
A guard whose ordering comparison receives incompatible operand types evaluates
to `false`, allowing matching to continue with the next case; ordering
comparisons outside guards retain their checked runtime-error behavior.

```slug
val classify = fn(value) {
  match value {
    0 => "zero"
    n if n > 0 => "positive"
    _ => "negative"
  }
}
```

Patterns include literals, `_` as a wildcard, identifier bindings, pinned
identifiers (`^name`), binding patterns (`name @ pattern`), list patterns
(which also match bytes as numeric byte sequences), and
map patterns. List and map patterns may have a final spread pattern such as
`...rest`. `{}` matches only an empty map. Exact non-empty map patterns use
`{|` and `|}` and do not permit a spread entry. A bare identifier or quoted
static string denotes a string map key; only a bare identifier may omit its
value pattern. A bracketed map-pattern key evaluates its expression once before
its pattern is tested, in the enclosing lexical scope before the pattern's
bindings exist. The resulting value must be a valid map key.

A whole case pattern may have a postfix type constraint, written
`pattern: Type`. It is part of matching rather than a declaration annotation:
the subject must satisfy both the constraint and the pattern before its guard
is evaluated. Type-constraint failure selects the next case and does not
produce a runtime type error. Constraints are not permitted inside nested
patterns, so map entries retain their ordinary `key: pattern` syntax.

```slug
match value {
  {} => "empty map"
  user @ {age: 43, name}: struct<User> => name
  {"status": 200} => "ready"
  {|k1, k2|}: map<str, str> => "two strings"
  b: bool => "$b is bool"
  _: struct => "another struct"
}
```

Direct value-category annotations, `struct<Name>`, unions composed from
runtime-checkable annotations, and recursively checked `list<T>` and
`map<K, V>` annotations are runtime-checkable. `any` matches non-nil values;
`any|nil` matches every value. `schema` matches schema values only. A
`struct<Name>` constraint requires the exact
schema identity named by `Name`; the schema binding must resolve, and a
resolved non-schema value is a checked runtime type error. Function signatures,
task or channel payload types, tuple types, and generic parameters are not
runtime-checkable and are source errors in a case constraint. The focused
The [Match and Destructuring](match-and-destructuring.md) supplement defines
the complete rule.

With `-type-check`, a match whose subject is a closed union of direct runtime
categories or exact `struct<Name>` identities receives conservative coverage
diagnostics. An unguarded irrefutable pattern (`_`, a binding, or an `@`
pattern around one) covers its whole type constraint; without a constraint it
covers every remaining member. The checker reports disjoint constrained cases,
unreachable unguarded cases, and uncovered remaining members. Guards are
always potentially false, and structural list, map, literal, and pinned
patterns do not establish coverage. `any`, `unknown`, collection types,
parameterized runtime values, and generic `struct` identity remain dynamic;
their matches retain the ordinary `nil` result when no runtime case matches.

```slug
val headOrZero = fn(xs) {
  match xs {
    [head, ...] => head
    [] => 0
  }
}
```

## Control transfer and deferred work

`return expression` completes the current function. `throw expression` starts
language-level error propagation. `recur(...)` is a function-level tail-call
operation and is valid only in tail position.

An uncaught `throw` terminates the current program with a runtime error that
retains the thrown Slug value, the `throw` source location, and available Slug
call frames.

## Recursion and repetition

Recursion is Slug's only source-language looping construct. Slug has no
`while`, `for`, or `loop` form, and it has no `break` or `continue` statement.
Programs express repetition by calling a function recursively. In a tail
position, `recur(...)` is the stack-safe form: it restarts the current function
with new argument values instead of making another call.

```slug
val sumTo = fn(n, total = 0) {
  if (n == 0) { total } else { recur(n - 1, total + n) }
}

sumTo(10) // 55
```

`defer` registers work to run when its enclosing scope exits. `defer onsuccess`
runs only after successful completion. `defer onerror(name)` runs only during
error propagation and binds the error to `name`.

A thrown value may be any Slug value. Slug has no `try` or `catch` construct;
`defer onerror` is the recovery mechanism. If an error handler returns normally,
it handles the active error: its result becomes its enclosing function's result
and its caller continues. Re-propagation requires an explicit `throw`. A
throw from a deferred action replaces the active error and retains it as the
cause. Runtime faults, including invalid calls and unknown names, use the same
error-unwinding path. Errors include available source location and Slug call
frames, while deferred helper frames are omitted.

The current core subset implements `defer`, `defer onsuccess`, and `defer
onerror`: actions run in last-in, first-out order when their scope exits.

`defer onsuccess` runs only on normal scope completion. It is skipped while a
throw or checked runtime fault is unwinding.

`defer onerror(err)` receives the original thrown value for `throw value`. For
a checked VM fault it receives a string-keyed map with `type`, `msg`, and
`data` fields. `type` is one of `invalid_bytecode`, `type`, `name`, `arity`,
`divide_by_zero`, `invalid_call`, `native`, `module`, or `match`; `msg` is its
diagnostic message; and `data` is `nil` until a fault defines structured extra
data.

```slug
val divide = fn(a, b) {
  defer { println("finished") }
  defer onerror(err) { println("failed:", err) }
  if (b == 0) { throw "division by zero" }
  a / b
}
```

## Modules, imports, and exports

`import(name, ...)` loads one or more named modules and returns a string-keyed
map of their exported bindings. Modules are loaded in argument order.
Module names use dot-separated paths such as `slug.std` and `slug.channel`.
An implementation resolves a module relative to the importing source, then the
project module root, before searching its configured library root. The command
line runtime uses `$SLUG_HOME/lib` as that root when `SLUG_HOME` is set. A
missing or malformed module is a language error.

```slug
val math = import("mod.simple")
val answer = math["forty"]
val next = math.inc(answer)

var {*} = import(
  "slug.std",
  "slug.test",
  "imports.defaults"
)
```

Only a top-level declaration prefixed with the `export` keyword is exported.
The keyword may modify a `val`, `var`, or `foreign` declaration and is invalid
in a nested scope. Exported bindings may be selected from the module map or
destructured with an ordinary binding pattern:

```slug
export val increment = fn(n) { n + 1 }
export foreign trim = fn(value:str)
```

```slug
val { map, filter } = import("slug.std")
var {*} = import("slug.channel")
```

Imports are live bindings, rather than snapshots. Reading an imported
non-function export observes its current binding in the defining module.
Modules are cached before top-level execution, so cyclic imports are allowed.
Statically knowable top-level bindings are declared before execution; using one
before it has initialized is a runtime error. A local declaration may shadow an
imported name, with a warning. When multiple imported modules provide the same
non-function name, the first loaded binding is retained with a warning. Callable
imports with the same name and signature likewise retain the first loaded
callable and issue a warning; callables with distinct signatures combine into
the imported overload set.

## Implicit builtin module

`slug.builtin` is the small, host-provided foundation module. When the host
registers matching bindings, its exports are implicitly available in every
module and may also be imported explicitly with `import("slug.builtin")`.
Local declarations take precedence over implicit builtin bindings. A host that
does not provide `slug.builtin` injects nothing; it does not create unbound
placeholder names.

The bundled declaration module documents `cfg(key, default)`,
`print(...values)`, `println(...values)`, and `len(value)`. The host registers
its functions
independently of the file. When present,
`lib/slug/builtin.slug` documents those functions and may export foundational
Slug values such as `Error`. The module is intentionally limited to primitives
and universally shared values. General utilities and channel operations belong
to ordinary explicit modules such as `slug.channel` and `slug.std`.

The standard library consists of modules loaded through this same mechanism.
Its public API is defined by the library reference, not by this specification.

### Output

`print(...values)` and `println(...values)` each accept zero or more positional
arguments, evaluate them in ordinary left-to-right call order, and return
`nil`. Each argument is rendered using the value's display representation; a
`str` is written as its contents without quotes. The rendered arguments are
separated by one ASCII space. `print` writes no trailing newline, while
`println` appends exactly one `\n` after the final rendered argument. Thus
`print()` writes nothing and `println()` writes one newline. Both functions
write to standard output.

### Length

`len(value)` accepts exactly one value and returns a non-negative integer:

- for `str`, the number of Unicode scalar values;
- for `bytes`, the number of bytes;
- for `list`, the number of elements; and
- for `map`, the number of entries.

Calling `len` with any other value, including `nil`, or with an argument count
other than one is a checked runtime error.

### Program entrypoint

After a program module's top-level statements succeed, the runtime invokes one
local, top-level function named `main` when it has one of these exact
signatures:

```slug
val main = fn() { ... }
val main = fn(args:list) { ... }
val main = fn(args:map) { ... }
```

The zero-argument form receives no values. The `list` form receives the raw
arguments following the entry program, in order. The `map` form receives the
parsed argument map with `"options"` and `"positional"` entries. Its option-map
keys use the resulting configuration keys, including entry-module prefixes for
undotted options.

The parameter name is not significant, but the one parameter must be required,
non-variadic, and annotated exactly `list` or `map`. An unannotated parameter,
a parameter with another annotation, or a defaulted or variadic parameter does
not define an entrypoint. Imported `main` bindings do not participate.

A program may define at most one eligible local `main`; multiple eligible
declarations are a semantic error. Other local functions named `main` remain
ordinary callable declarations. If no eligible local `main` exists, evaluation
ends after the top-level statements. A top-level failure prevents entrypoint
invocation.

```slug
val main = fn() {
  println("serve")
}
```

```slug
val main = fn(args:list) {
  println(args)
}
```

```slug
val main = fn(args:map) {
  println(args.positional)
}
```

## Configuration

`cfg(key, default)` is a builtin that reads the immutable, process-wide
configuration supplied by the runtime. Its key must be a string and its fallback
is required. If no configured value exists, it returns the fallback. A key with
a dot is absolute. A key without a dot is relative to the calling module, so
`cfg("port", 8080)` in `slug.web.server` reads `slug.web.server.port`.

The configuration value is determined before program evaluation. Its source,
layering, key names, and value conversions are runtime requirements rather than
language syntax. See [Configuration](configuration.md) for the portable
configuration contract.

## Not implemented placeholder

`???` evaluates by raising a checked runtime error with the message `not implemented`.
It is a temporary source placeholder and does not produce a value.

## Tags, documentation, and foreign declarations

A tag has the form `@name` or `@name(arguments)` and prefixes a `val`, `var`,
`foreign`, or exported declaration. Tags may also prefix function parameters.
Tag arguments are expressions evaluated in the declaration's module
environment. Tags attach metadata and do not, by themselves, change evaluation
semantics. `export` is a declaration modifier, not metadata. `@export` is its
retired export marker, but remains an ordinary valid tag name with no export
semantics. Write `export val` or `export foreign` to make a declaration
exported.

The current Rust subset accepts tags on `val` and `var` declarations and on
function parameters. It evaluates their arguments in the current lexical
environment when the corresponding declaration or function literal is
evaluated; declaration tags run before the declared value. Tagged foreign
declarations retain their metadata, but host resolution and `slug.meta`
introspection are not implemented yet.

The subset also parses strict documentation blocks on top-level `val`, `var`,
and `foreign` declarations, as well as a first module doc block followed by a
blank line. It retains top-level declaration documentation and evaluated tag
metadata in the module model; metadata introspection is not implemented yet.

```slug
@deprecated
export val increment = fn(n) { n + 1 }

export foreign trim = fn(value:str)
```

The `export` modifier makes a top-level binding visible to `import`. Callable
declarations with the same name form overloads when their signatures are
distinct. This applies independently in each lexical scope and at module scope;
later declarations append to the current scope's callable set rather than
replacing an earlier closure. This includes a local function and a `foreign` declaration. A
duplicate callable signature is an error; a non-callable `val` remains
immutable.

Callable signature identity is canonical and structural. It includes generic
arity and use by declaration-order position, each parameter's call-visible
label, normalized annotation, default-presence, and variadic status. Generic
parameter names do not participate. Union annotations are flattened,
deduplicated, and canonically ordered; tuple elements and type arguments remain
ordered. An unannotated parameter canonicalizes to `any|nil`, and a discard
parameter has no call label. Default expressions and return annotations do not
participate in identity. Signature equality is distinct from assignability,
which is used only to determine overload applicability and specificity.

When applicable candidates have equivalent instantiated parameter types, the
candidate with lower generic arity is more specific. A non-generic concrete
overload therefore takes priority over a generic fallback that inference made
equivalent for this call. Candidates with equal generic arity remain tied;
declaration or import order does not resolve the ambiguity.

A `foreign` declaration names a host-supplied callable in the current module.
Before that module initializes, the runtime resolves each declaration against
the host's module-qualified foreign-function registry. The host function is
then visible only through the declared local binding. The registered callable
must have the same arity range as the declaration, including omitted defaults
and variadic calls. An unavailable or incompatible binding is a checked
foreign-resolution runtime error; it is never silently substituted with an
unrelated host global.

Each resolved foreign binding privately retains the canonical identity of its
source declaration. This lets a statically selected overload dispatch to the
declared foreign member without exposing source type annotations to native code
or adding runtime type validation. Repeated compatible foreign declarations and
mixed foreign/local callable declarations therefore retain each distinct
declared member in their live overload set.

A doc block uses `/** ... */`. Every non-empty content line must begin with
`*`, otherwise parsing fails. At top level, a doc block attaches to the next
`val`, `var`, or `foreign` declaration, whether or not it is exported, allowing
intervening tags, the `export` modifier, and comments. The first meaningful doc
block in a module is the module documentation when it is followed by a blank
line. Documentation and tags are observable through `slug.meta` introspection.

## Static checking

Slug always performs semantic validation, including `recur` tail-position
validation, struct-schema checks, program-entrypoint signature validation,
and resolution of statically known overloads. Type annotations do not introduce
runtime validation or coercion. Parameter annotations participate in mandatory
resolution of statically known overloads. Optional type checking uses
annotations for additional diagnostics and is enabled by the CLI `-type-check`
flag. When it is enabled, those additional type diagnostics prevent execution;
it does not change overload selection for a program accepted in both modes.

Because a call spread has runtime-determined arity, a call to a statically known
overload set with one or more `...spread` arguments is a semantic error. This
also applies when a pipeline supplies its leading positional argument. A call
to one statically known callable remains valid and uses ordinary runtime spread
binding.

The current Rust subset parses and retains declaration, parameter, return, and
struct-field annotations. Its optional checker rejects directly provable
annotation mismatches in declarations, parameter defaults, function returns,
struct defaults, and calls to statically known annotated functions. Function
expressions infer structural `fn<R, P...>` value types from their parameter
annotations and declared or inferred result; that precision is retained through
ordinary bindings and collection inference. It infers generic arguments from
annotated call positions and supports explicit type applications. Successful
match type constraints narrow case-local bindings. Flow-sensitive narrowing
and inference for the remaining dynamic expression forms remain future work.

When `-type-check` is enabled, operators, indexing, and slicing also check
statically known operand families. Numeric arithmetic, bitwise and shift
operators, unary numeric operations, ordering comparisons, list append and
prepend, string concatenation and repetition, list concatenation, indexing,
and list slicing reject a fully known incompatible operand. Equality and
logical operators accept every value type. `unknown`, `any`, and unions that
include either remain dynamic and do not introduce a static diagnostic.
Successful operations retain their result type: list access yields its element
type, map access yields its value type plus `nil`, list slices retain their
element type, and list combination operations union their element types. The
`num` annotation is broader than the VM's integer-only bitwise, shift, index,
and slice-bound operations, so those expressions accept `num` statically and
retain their checked runtime error for a non-integral value.

For a map literal with a static string key, the checker retains the known
binding at that key through dot-access chains. This permits a callable or
nested map stored in a literal map to remain callable or indexable through
successive `.key` accesses; lookup of an unknown map key still has the generic
`V|nil` result type.

The bitwise operators `&`, `|`, and `^` accept two integers or byte operands.
When either operand is bytes, an integer from `0` through `255` becomes a
one-byte value and the shorter non-empty byte operand repeats to the longer
length. Unary `~` complements either an integer or every byte in a bytes value.

With `-type-check`, a direct binding comparison to `nil` refines that binding
within the relevant control-flow path. `if (value != nil)` excludes `nil` in
its then branch and gives `value` type `nil` in its else branch; `== nil`
reverses the facts. The same facts apply to the evaluated right operand of
short-circuit `&&` and `||`, and a successful match guard contributes its
facts to that case result. Branch facts do not escape to the enclosing scope.
For a mutable binding, a refined assignment type survives an `if` only when
both continuing paths agree on it. The result of `if`, `&&`, or `||` is the
union of its possible result values. Other predicates and incompatible joins
remain conservative.

When a statically known value has a structural function type but no declaration
callable metadata—for example, a function selected by an `if` expression—a
positional, non-spread call has the function type's result type. With
`-type-check`, its arity and argument types must match the structural parameter
types. Structural function types do not encode parameter labels, defaults, or
variadic status, so named and spread calls retain ordinary dynamic call
behavior.

The checker recognizes the built-in value categories `nil`, `any`, `bool`,
`num`, `str`, `bytes`, `list`, `map`, `fn`, `task`, `chan`, and `struct`, plus
unions and generic parameters. Its diagnostic precision is an implementation
feature and does not add runtime coercions or change the language's dynamic
value model.

### Type annotations, unions, and generic parameters

Annotations may appear on an identifier binding, function parameter, function
return, foreign parameter or return, and struct field. They use the following
structural forms. Whitespace around punctuation is insignificant.

```ebnf
type_annotation = union_member , { "|" , union_member } ;
union_member    = named_type , [ "<" , type_annotation ,
                  { "," , type_annotation } , ">" ]
                | tuple_type ;
named_type      = identifier | "fn" ;
tuple_type      = "[" , [ type_annotation , { "," , type_annotation } ] , "]" ;
```

`A|B` describes a value that may conform to either `A` or `B`; it does not
convert between them. `nil` is an ordinary union member, so `str|nil` is the
nilable form of `str`. A declaration of `str` does not permit `nil`, while a
declaration of `str|nil` permits both a string and `nil`. Union members may be
nested within parameterized types, such as `list<str|nil>` and
`map<str, num|nil>`.

`any` is the top type for non-nil Slug values. Every non-nil type conforms to
`any`, but `nil` does not; `any|nil` is the universal type containing every
Slug value. Union normalization therefore reduces `any|str` to `any` and
`any|nil|str` to `any|nil`. An unrecognized annotation name is a source error,
not an unconstrained type. The analyzer may use a private unknown state while
inference is incomplete, but unknown is not a source-visible or exported type.

```slug
var label:str|nil = "ready"
label = nil

val names:list<str|nil> = ["Ada", nil]
val scores:map<str, num|nil> = {ada: 10, bob: nil}
```

An unannotated parameter has type `any|nil`; `fn(value)` and
`fn(value:any|nil)` therefore have identical input signatures. An unannotated
binding is inferred from its initializer, and an unannotated function result is
inferred from all reachable result expressions. If no more precise type can be
inferred, its type widens to `any|nil` before it is retained or used in overload
resolution. An explicit annotation remains the declaration's public type even
when its value is inferred more narrowly:

```slug
val inferred = fn() { 1 }             // fn():num
val widened = fn():any|nil { 1 }      // fn():any|nil
val nonNil = fn():any { "ready" }     // cannot return nil
```

The built-in `schema` type describes schema values and does not accept type
arguments. The common parameterized forms are `list<T>`, `map<K, V>`,
`chan<T>`, `task<T>`, and `struct<Name>`. A bracketed type such as `[str, num]` is a
fixed-length tuple type. A function type is written `fn<R, P1, P2, ...>`,
where the first argument is the return type and the remaining arguments are
the parameter types. For example, `fn<num, num, num>` denotes a function that
returns `num` and accepts two `num` parameters.

A function or foreign declaration introduces generic parameters immediately
after `fn`. A parameter name is a type variable scoped to that declaration and
can appear in parameter, return, nested, and union annotations:

```slug
val identity = fn<T>(value:T):T { value }
val firstOrNil = fn<T>(values:list<T>):T|nil {
  if (len(values) == 0) { nil } else { values[0] }
}
```

Each call instantiates a generic declaration. The checker infers a type
argument from annotated argument positions and uses the same instantiation for
every occurrence of that parameter. Thus `fn<T>(left:T, right:T):T` requires
its two arguments to agree on `T`. A bare type parameter does not accept
`nil` and cannot be instantiated with a type containing nil; use `T|nil`
wherever a nilable argument or result is intended. Nil by itself does not infer
`T` through a `T|nil` position, so the call must provide another inference
position or an explicit non-nil type argument.

Callers may provide type arguments explicitly with `name<Type>(...)`, for
example `identity<str>("Slug")`. An explicit application must name a generic
function, provide exactly one type argument per declared parameter, and be
immediately followed by its call. Type arguments are checked against the
ordinary call arguments. Slug has no bounded generic parameters, variance
annotations, type aliases, or user-declared nominal generic types.

## Concurrency, channels, and `select`

`spawn` starts a child task from a block or function body and yields a task
handle. Every program and function evaluation has an implicit root nursery.
`nursery` creates an explicit ownership boundary, and `nursery limit N` limits
its direct child spawns.

```slug
val result = nursery fn() {
  val tasks = import("slug.channel")
  val task = spawn { 20 + 22 }
  tasks["await"](task)
}
```

Task completion is cached. Awaiting a task returns that result or propagates
its error. An await marks the task's failure as observed by its owning nursery;
repeated awaits return the same cached completion. The ordinary task-await API
is the `slug.channel.await` library callable. It is implemented with the
`await` `select` case form; `await` is not an independently reserved
expression keyword or a builtin.

Root evaluation, explicit nursery bodies, and spawned tasks suspend
cooperatively on task, channel, timer, and `select` operations. Owner
settlement runs after either a successful or failed body and
propagates unobserved child failures. An explicit nursery logically cancels
pending siblings after its first unobserved child failure; cancellation does
not forcibly interrupt host execution.

A nursery limiter admits direct children up to its limit and queues further
direct spawns, releasing an admission slot at settlement. Awaiting a queued
task first drives earlier admitted direct children, preserving admission
order. A nursery limit must be a positive integer, and an admitted task retains
its permit while suspended. Ready tasks begin in spawn order; awaiting a later
task therefore first drives earlier ready siblings.

A child belongs to the current dynamic nursery. Normal nursery exit waits for
its remaining children. An explicit nursery propagates its first unobserved
child failure and logically cancels siblings. Cancellation settles a task with
an error but does not forcibly interrupt a host thread. The root nursery joins
its descendants before program completion.

A spawned task snapshots its immediate lexical bindings. Subsequent parent
assignment is not visible through those captured local bindings. Captured
values are shallow-shared, so channels and mutable objects retain identity.
Outer lexical bindings and module globals remain live, and ordinary closures
continue to share their captured mutable bindings.

`select` evaluates cases for channel receive, channel send, timers, task await,
and `_` as a default case. The selected case value is piped to its handler:

```slug
select {
  recv channel /> fn(value) { value }
  after 100 /> fn(_) { "timeout" }
  _ /> fn(_) { "default" }
}
```

`after` takes a non-negative integer delay in milliseconds and selects with
`nil` when that delay has elapsed. A select with no immediately ready
non-default case parks until one registered case becomes ready. The first case
made ready resumes the evaluation and removes every other case's waiter; those
losing cases must not consume a later channel value or task completion.
Checking immediate readiness does not run or wait for an unsettled task. A
losing task-await case does not mark that task's failure as observed.

`slug.channel.recv(channel)` blocks for a value or returns `nil` after a closed
channel has drained. Sending `nil` or sending on a closed channel is a runtime
error.
Closing a channel is idempotent. The selection policy among simultaneously
ready cases is intentionally unspecified.

The public `slug.channel` surface is `chan(capacity = 0)`, `send(channel,
value)`, `recv(channel, timeout = 0)`, `close(channel)`, `await(handle,
timeout = 0)`, `trySend(channel, value)`, and `tryRecv(channel)`. These are
library bindings, not global Slug bindings. `channel` is an internal runtime
operation. `capacity` is a non-negative integer. A zero-capacity channel
performs FIFO rendezvous between blocked senders and receivers; a positive
capacity retains that many FIFO messages. `send` returns its channel so callers
can chain sends with pipelines; `close` returns `nil`. A blocked sender that is
released because its channel closes fails as a normal `send on a closed channel`
runtime error, so its active deferred cleanup still runs.

## Implementation-independent limits

This specification deliberately leaves bytecode representation, host scheduling
fairness, foreign-function ABI details, memory management, and resource limits
to the runtime requirements and host implementation. Such choices must not
change the observable language behavior specified here.
