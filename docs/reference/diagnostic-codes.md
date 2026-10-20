# Diagnostic Codes

This document is normative. It lists every stable diagnostic code. A code names
a failure class; it is the stable key for tools and agents. Diagnostic message
text is not stable and may change while its code stays the same. A code is
never reused for a different failure class.

Codes appear as the `code` member of every [diagnostic document](diagnostics.md),
from `--diagnostic-format=json` and from the interactive server. They are
unique across categories.

## Source codes

Source codes classify parse and semantic errors. Rust hosts read them as
`slug_frontend::SourceErrorCode`.

| Code | Category | Failure | Example message |
|---|---|---|---|
| `unexpected_character` | parse | A character that starts no token. | ``unexpected character `#` `` |
| `unterminated_literal` | parse | A string, byte literal, block comment, or documentation block without its closing delimiter. | `unterminated string` |
| `invalid_number` | parse | A malformed number literal, including separators, hexadecimal digits, exponents, non-finite values, and integers larger than 9223372036854775807. An out-of-range integer offers to append `.0`, making it a float: as a fix when the float holds the same value, and otherwise as a suggestion, since the float rounds it. | `invalid number separator` |
| `invalid_byte_literal` | parse | A malformed `0x"..."` byte literal. | `byte literal must contain complete hexadecimal byte pairs` |
| `expected_syntax` | parse | The next token cannot continue the construct being parsed. | `expected }` |
| `invalid_documentation` | parse | A malformed or misplaced documentation block or tag. | `every non-empty documentation line must begin with *` |
| `top_level_only` | parse, semantic | A declaration form used below top level. | `export declarations are only valid at top level` |
| `invalid_parameters` | parse | A parameter list with a misplaced, repeated, or defaulted variadic parameter. | `variadic parameter must be final` |
| `invalid_argument_order` | parse | A positional or spread argument after a named argument. | `positional argument cannot appear after a named argument` |
| `invalid_pattern` | parse, semantic | A pattern form that is not allowed where it appears. | `list spread pattern must be final` |
| `empty_enum` | parse | An enum without cases. | `enum must declare at least one case` |
| `nesting_limit_exceeded` | parse | Source nested deeper than the parser accepts. | `source nesting limit exceeded` |
| `duplicate_name` | parse, semantic | A name declared twice where names must be unique, such as fields, parameters, enum cases, types, or match bindings. | ``duplicate struct field `x` `` |
| `duplicate_signature` | semantic | Two callables with the same name and signature in one scope. | ``duplicate callable signature for `f` `` |
| `unknown_name` | semantic | A reference to a binding that the checker can prove does not exist. | ``unknown pinned binding `x` `` |
| `unknown_type` | semantic | A type, type constructor, enum, or type module prefix that does not exist. | ``unknown type `Missing` `` |
| `unknown_member` | semantic | A struct field or enum case that the type does not declare. | ``struct has no field `z` `` |
| `missing_field` | semantic | A struct construction without a required field. | ``missing required struct field `name` `` |
| `type_mismatch` | semantic | A value whose type does not satisfy its use. | `expected num, got str` |
| `invalid_type_arguments` | semantic | Type arguments of the wrong number or kind. | `wrong number of type arguments` |
| `recursive_type_alias` | semantic | A type alias that refers to itself. | `recursive type alias: A -> B -> A` |
| `immutable_assignment` | semantic | An assignment to a `val` binding. | ``cannot assign to immutable binding `x` `` |
| `invalid_control_flow` | semantic | `return` or `recur` where it is not allowed. | `recur is only valid in tail position` |
| `invalid_match` | semantic | A match whose subject or type constraints are malformed. | `match requires a subject or pipeline input` |
| `non_exhaustive_match` | semantic | A match that does not cover every possible value. | `non-exhaustive match; missing nil` |
| `unreachable_match_case` | semantic | A match case that earlier cases already cover. | `match case is unreachable` |
| `no_matching_overload` | semantic | A call that no overload or inferred alternative accepts. | ``no matching overload for `f` `` |
| `ambiguous_overload` | semantic | A call that more than one overload accepts equally. | ``ambiguous overload for `f` `` |
| `repeated_map_key` | semantic | A map literal that repeats a constant key. | ``map literal repeats key `a` `` |
| `lost_update` | semantic | An assignment to a `var` that reads it before a call that can assign it, so the call's change is discarded. | ``this assignment to `output` discards what the call to `evaluate` at 6:28 assigns to it: ...`` |
| `invalid_entrypoint` | semantic | A program whose `main` entrypoint cannot be identified. | ``program has more than one eligible `main` entrypoint`` |
| `internal` | semantic | An implementation invariant failed. Report it as a bug. | `missing resolved match constraint` |

## Runtime codes

Runtime and module diagnostics use these codes: `invalid_bytecode`, `type`,
`name`, `arity`, `divide_by_zero`, `invalid_call`, `native`,
`native_contract`, `module`, `module_not_found`, `not_implemented`, `match`,
`thrown`, or `permission`. `permission` is an operation the user running slug
has not granted, such as `describe` of unexported declarations without
`--allow-unexported`. `module_not_found` is an import of a module that no root or clutch
provides; its diagnostic may carry [suggestions](diagnostics.md#suggestions).
Other module failures currently share the `module` code. When an imported
module does not compile, the `module` diagnostic's `cause` is that module's
source diagnostic, with its own code.

## Command-line startup codes

A command-line failure before a program runs has category `usage`, `io`,
`host`, or `artifact`. Every `usage` failure has code `usage`; its message
names the subcommand. Other startup failures use these codes:

| Code | Category | Failure |
|---|---|---|
| `entry_read` | io | The entry source cannot be read. |
| `artifact_read` | io | A `.cslug` file cannot be read. |
| `artifact_write` | io | A `.cslug` file cannot be written. |
| `clutch` | host | A clutch cannot be activated. |
| `builtin` | host | Host builtins cannot be registered. |
| `compile` | artifact | A program cannot be compiled into an artifact or report. |
| `encode` | artifact | An artifact cannot be encoded. |
| `decode` | artifact | A `.cslug` file cannot be decoded. |
| `load` | artifact | A decoded artifact cannot be loaded. |

The interactive server's `protocol` and `host` codes belong to its unstable
protocol and are not listed here.
