# JSON library

Write a Slug module, `./json.slug`, that parses and serializes JSON
(RFC 8259). It will be graded by a hidden suite of programs that import it
with `val json = import("json")`.

Export exactly these two functions.

## `parse(text:str)`

Returns the Slug value for the JSON document `text`:

| JSON | Slug |
|---|---|
| object | map with string keys; when a key repeats, the last value wins |
| array | list |
| string | `str` |
| number | `num` |
| `true`, `false` | `bool` |
| `null` | `nil` |

- Whitespace is space, tab, line feed, and carriage return, and may surround
  any value. Exactly one value must be present; anything after it other than
  whitespace is an error.
- Numbers follow JSON grammar: an optional `-`, then `0` or a digit sequence
  not starting with `0`, an optional fraction (`.` and at least one digit),
  and an optional exponent (`e` or `E`, optional `+` or `-`, at least one
  digit). `+1`, `01`, `.5`, `1.`, and `1e` are invalid. A number whose value is
  an integer that fits in a signed 64-bit integer parses to exactly that
  integer, so `9007199254740993` is exact. Any other number parses to the
  binary64 value nearest to the exact decimal value, rounding a tie to even,
  however many digits it has: `2.5E-3` is `0.0025`, `1.0 == 1`, and
  `3.141592653589793238462643383279` is `3.141592653589793`. A number that
  underflows parses to `0`. A number whose magnitude is too large for binary64
  is an error.
- Strings support the escapes `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`,
  and `\uXXXX` with upper- or lowercase hex digits. A `\u` escape that is a
  UTF-16 high surrogate followed immediately by a `\u` low surrogate forms one
  character; a surrogate escape that is not part of such a pair is an error.
  Characters below U+0020 must not appear unescaped. Other characters,
  including non-ASCII ones, appear as themselves.

An invalid document throws an `Error` (from `slug.builtin`) with
`type: "json.parse"`, any `msg`, and `data: {offset: n}`. `n` is the
zero-based character index of the first character at which the text can no
longer be valid JSON, or `len(text)` when the text ends too early. For example:

| `text` | offset |
|---|---|
| `[1,]` | 3 |
| `tru` | 3 |
| `01` | 1, the trailing `1` |
| `{"a" 1}` | 5 |
| `"a` | 2 |

For an invalid escape, the offset is that of the offending character after the
backslash, or of the first invalid hex digit; for an invalid surrogate, it is
that of the `\` beginning the surrogate escape.

## `stringify(value)`

Returns compact, canonical JSON text for `value`: no whitespace anywhere.

- `nil` is `null`, booleans are `true` and `false`.
- A number is written exactly as Slug displays it in string interpolation,
  `"$n"`: `42`, `-0.5`, `0.0025`, and `1.0` writes `1`.
- A string is written in double quotes. `"` and `\` are escaped as `\"` and
  `\\`; line feed, carriage return, tab, backspace, and form feed as `\n`,
  `\r`, `\t`, `\b`, and `\f`; every other character below U+0020 as `\u00`
  followed by two lowercase hex digits. Every other character, including `/`
  and non-ASCII characters, is written as itself.
- A list is an array of its elements in order.
- A map is an object with its keys sorted in ascending order by Slug's string
  ordering (`<`), so `{"b": 1, "a": 2}` writes `{"a":2,"b":1}`.

A value with no JSON form, such as a function, bytes, a struct, an enum value,
or a map with a non-string key, anywhere inside `value`, throws an `Error` with
`type: "json.type"` and any `msg`.

## Notes

- `slug.string` provides `ord` and `chr` for converting between characters and
  Unicode scalar values, `chars` for splitting a string into characters, and
  `sort` and `join` for lists of strings; see
  slug-docs/library/core/string.slug.
- Write your own test programs beside `json.slug` to check your work. The
  graded suite covers every rule above, including edge cases, and includes
  large and deeply nested documents that must be handled in a few seconds.
