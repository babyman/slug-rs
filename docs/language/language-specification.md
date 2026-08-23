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
- accepted and error behavior: `../../tests/vm-conformance`;
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
| Bytes | `0x"414243"` |
| Lists | `[1, 2, 3]` |
| Maps | `{name: "Slug"}` |
| Functions | `fn(x) { x + 1 }` |

Strings are Slug's only textual value type. The language does not expose a
separate symbol or atom value; implementations may still intern identifiers
internally.

Numbers may contain underscore separators. A bare identifier used as a map key
is a string key, so `{name: "Slug"}` is indexed with `["name"]`. A bracketed
map key evaluates an expression instead:

```slug
val key = "name"
val byName = {name: "Slug"}
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
or an unknown name.

```slug
var count = 0
count = count + 1

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
| `:+` `+:` | left |
| prefix `!` `-` `~` | right |
| pipeline `/>` | left |
| calls, indexing, dot access, struct initialization and copy | left |

Bitwise operators (`&`, `|`, `^`, and `~`) and shifts (`<<`, `>>`) accept
integers only. A shift count must be an integer from `0` through `63`; invalid
operand or shift-count combinations are checked runtime type errors. Right
shifts are arithmetic: they preserve the sign of a negative integer.

`+` concatenates two lists into a new list. The directional list operators also
produce new lists: `list :+ value` appends one value, while `value +: list`
prepends one value. The directional list operand must be a list; otherwise
evaluation produces a checked runtime type error.

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

A struct expression defines a schema. Applying a schema to `{...}` creates a
struct value. Struct fields can have type annotations and defaults. `copy`
creates a value with replacement fields. Tags on struct fields are not syntax.

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
runtime type error. The Struct Syntax and Behavior mini spec defines the
detailed rule and current implementation boundary.

## Pattern matching

`match` selects the first matching case. A case may have a guard introduced by
`if`. Pattern alternatives in one case are allowed only when they do not create
bindings. If the subject is a map literal, it must be parenthesized to avoid
being read as the case block: `match ({name: "Slug"}) { {name} => name }`.

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
identifiers (`^name`), binding patterns (`name @ pattern`), list patterns, map
patterns, and struct patterns. List and map patterns may have a final spread
pattern such as `...rest`. Exact map patterns use `{|` and `|}` and do not
permit a spread entry. A bracketed map-pattern key evaluates its expression
once before its pattern is tested, in the enclosing lexical scope before the
pattern's bindings exist. The resulting value must be a valid map key.

A struct pattern has the form `Schema {field, other: pattern}`. It matches only
a struct value created by that exact schema identity. Fields are partial: each
named field must exist and match its nested pattern, while unnamed fields are
ignored. A non-struct subject or a value from a different schema does not
match. The schema expression must evaluate to a schema; otherwise matching
produces a checked runtime type error.

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
`divide_by_zero`, `invalid_call`, `native`, or `match`; `msg` is its diagnostic
message; and `data` is `nil` until a fault defines structured extra data.

```slug
val divide = fn(a, b) {
  defer { println("finished") }
  defer onerror(err) { println("failed:", err) }
  if (b == 0) { throw "division by zero" }
  a / b
}
```

## Modules, imports, and exports

`import(name)` loads a named module and returns a string-keyed map of its
exported bindings.
Module names use dot-separated paths such as `slug.std` and `slug.channel`.
An implementation resolves a module relative to the importing source before
searching its configured library root. A missing or malformed module is a
language error.

```slug
val math = import("mod.simple")
val answer = math["forty"]
val next = math.inc(answer)
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
imported name, with a warning. When two imports provide a non-function name,
the first binding is retained with a warning. Callable imports combine only
when their signatures are distinct.

The standard library consists of modules loaded through this same mechanism.
Its public API is defined by the library reference, not by this specification.

### Program entrypoint

After a program module's top-level statements succeed, the runtime invokes the
first locally declared, top-level function named `main` that is eligible for a
call with no supplied arguments. A function is eligible when it has no
parameters or every declared parameter has a default. A required or variadic
parameter without a default makes that function ineligible.

Selection follows source declaration order, including the order of callable
overloads. Imported functions named `main` do not participate. If no local
`main` is eligible, evaluation ends after the top-level statements. If several
are eligible, only the first is invoked, using ordinary default-argument
binding.

```slug
val main = fn(mode = "serve") {
  println(mode)
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

## Tags, documentation, and foreign declarations

A tag has the form `@name` or `@name(arguments)` and prefixes a `val`, `var`,
`foreign`, or exported declaration. Tags may also prefix function parameters.
Tag arguments are expressions evaluated in the declaration's module
environment. Tags attach metadata and do not, by themselves, change evaluation
semantics. `export` is a reserved keyword rather than a tag name, so `@export`
is invalid.

```slug
@deprecated
export val increment = fn(n) { n + 1 }

export foreign trim = fn(value:str)
```

The `export` modifier makes a top-level binding visible to `import`. Callable
declarations with the same name form overloads when their signatures are
distinct. This includes a local function and a `foreign` declaration. A
duplicate callable signature is an error; a non-callable `val` remains
immutable.

A `foreign` declaration names a host-supplied callable in the current module.
It is resolved before module code executes. If no implementation is registered
for the module-qualified name, evaluation fails with an unknown-foreign error.
Foreign declarations use the declared parameter, return-type, and tag metadata
for ordinary call binding and introspection.

A doc block uses `/** ... */`. Every non-empty content line must begin with
`*`, otherwise parsing fails. At top level, a doc block attaches to the next
`val`, `var`, or `foreign` declaration, whether or not it is exported, allowing
intervening tags, the `export` modifier, and comments. The first meaningful doc
block in a module is the module documentation when it is followed by a blank
line. Documentation and tags are observable through `slug.meta` introspection.

## Static checking

Slug always performs semantic validation, including `recur` tail-position
validation, struct-schema checks, and program-entrypoint discovery. Inferred
type checking is optional and is enabled by the CLI `-type-check` flag. When it
is enabled, type diagnostics prevent execution. Without it, type tags remain
metadata and unsupported operations fail through normal runtime errors.

The current Rust subset parses and retains declaration, parameter, return, and
struct-field annotations. Its optional checker rejects directly provable
annotation mismatches in declarations, parameter defaults, function returns,
struct defaults, and calls to statically known annotated functions. It infers
generic arguments from annotated call positions and supports explicit type
applications. Richer expression inference remains future work.

The checker recognizes the built-in value categories `nil`, `bool`, `num`,
`str`, `bytes`, `list`, `map`, `fn`, `task`, `chan`, and `struct`, plus
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

```slug
var label:str|nil = "ready"
label = nil

val names:list<str|nil> = ["Ada", nil]
val scores:map<str, num|nil> = {ada: 10, bob: nil}
```

The common parameterized forms are `list<T>`, `map<K, V>`, `chan<T>`,
`task<T>`, and `struct<Name>`. A bracketed type such as `[str, num]` is a
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
`nil`; use `T|nil` wherever a nilable argument or result is intended.

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
is a library callable, conventionally imported from `slug.channel`. `await` is
also a `select` case form, not an independently reserved expression keyword.

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

`recv(channel)` blocks for a value or returns `nil` after a closed channel has
drained. Sending `nil` or sending on a closed channel is a runtime error.
Closing a channel is idempotent. The selection policy among simultaneously
ready cases is intentionally unspecified.

## Implementation-independent limits

This specification deliberately leaves bytecode representation, host scheduling
fairness, foreign-function ABI details, memory management, and resource limits
to the runtime requirements and host implementation. Such choices must not
change the observable language behavior specified here.
