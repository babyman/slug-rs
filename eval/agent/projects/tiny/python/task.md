# Tiny interpreter

Write a Python module, `./tiny.py`, that interprets Tiny, a small scripting
language defined below. It will be graded by a hidden suite of programs that
import it with `import tiny` and run under `python3` (Python 3.9), with only
the standard library.

Define `run` and `TinyError`, described below.

## `run(source)`

Runs the Tiny program `source`, a `str`, and returns the list of lines it
printed, as a `list` of `str`, in order. A program that prints nothing returns
`[]`.

An invalid or failing program raises `TinyError`, a subclass of `Exception`,
with attributes `kind`, `line`, and `column`, where `kind` is one of the kinds
below and `line` and `column` give the position the error is reported at.
Lines and columns count from 1; a column counts characters, and a tab is one
character. The first error ends the run, and nothing printed before it is
returned.

## Lexical structure

- Whitespace is space, tab, carriage return, and line feed. `#` begins a
  comment that runs to the end of the line.
- An integer is a sequence of ASCII digits, such as `0` or `42`. There are no
  signs, fractions, or exponents in literals; `-5` is negation applied to `5`.
  A literal larger than 9223372036854775807 is a `syntax` error at its first
  digit.
- A string is written in double quotes. Its escapes are `\n`, `\t`, `\"`, and
  `\\`. Any other character after a backslash is a `syntax` error at the
  backslash. A string with no closing quote before the end of its line or of
  the input is a `syntax` error at its opening quote.
- A name is a letter or `_` followed by letters, digits, and `_`, where a
  letter is an ASCII letter. These names are keywords: `let`, `fn`, `return`,
  `if`, `else`, `while`, `true`, `false`, `nil`, `and`, `or`, `not`, `print`.
- The other tokens are `+ - * / % == != < <= > >= = ( ) { } [ ] , ;`.
- Any other character is a `syntax` error at that character.

## Grammar

```
program    = { statement } ;
statement  = "let" name "=" expr ";"
           | name "=" expr ";"
           | "print" expr ";"
           | "if" "(" expr ")" block [ "else" ( block | if-statement ) ]
           | "while" "(" expr ")" block
           | "fn" name "(" [ params ] ")" block
           | "return" [ expr ] ";"
           | block
           | expr ";" ;
block      = "{" { statement } "}" ;
params     = name { "," name } ;
expr       = or ;
or         = and { "or" and } ;
and        = equality { "and" equality } ;
equality   = comparison { ( "==" | "!=" ) comparison } ;
comparison = additive { ( "<" | "<=" | ">" | ">=" ) additive } ;
additive   = term { ( "+" | "-" ) term } ;
term       = unary { ( "*" | "/" | "%" ) unary } ;
unary      = ( "-" | "not" ) unary | postfix ;
postfix    = primary { "(" [ expr { "," expr } ] ")" | "[" expr "]" } ;
primary    = integer | string | "true" | "false" | "nil" | name
           | "(" expr ")" | "[" [ expr { "," expr } ] "]"
           | "fn" "(" [ params ] ")" block ;
```

All binary operators group to the left. A statement that begins with a name
followed by `=` is an assignment. A statement that begins with `fn` followed by
a name is a function declaration; `fn` followed by `(` begins an expression
statement.

A program that does not match the grammar is a `syntax` error at the first
token that cannot continue a valid program, or, when the input ends too early,
at the position just after the last character of the input. A `return` outside
any function body is a `syntax` error at the `return`. Syntax errors are found
before anything runs, so a program with one prints nothing.

## Meaning

- **Scopes.** Names are lexically scoped. The program and each block are
  scopes; a function body is a block. `let` creates a name in the current scope,
  replacing one already created there. `fn name(...)` creates `name` in the
  current scope like `let`, before its body can run, so functions can call
  themselves. Reading or assigning a name that no enclosing scope has is a
  `name` error at the name.
- **Functions** are values. A function captures the scopes where it was
  created, and assignments through them are shared, so a function can keep a
  counter in an enclosing `let`. A call evaluates the callee, then the
  arguments from left to right. Calling a value that is not a function is a
  `type` error at the call's `(`; calling with the wrong number of arguments is
  an `arity` error at the `(`. A function that ends without `return`, or with
  a bare `return;`, returns `nil`.
- **Builtins.** `len(x)` is the number of characters of a string or elements of
  a list, and a `type` error at the `(` for anything else. `str(x)` is the text
  `print` would write for `x`. They are names in the program's scope and can be
  replaced like any other.
- **Integers** are 64-bit; programs in the suite never overflow. `+`, `-`, and
  `*` are the usual operations. `/` divides and truncates toward zero, so
  `-7 / 2` is `-3`; `%` is the remainder with the sign of the dividend, so
  `-7 % 2` is `-1`. Dividing by zero with either is a `division` error at the
  operator.
- **Strings and lists.** `+` joins two strings or two lists. `x[i]` is the
  element `i` of a list, or the one-character string at `i` of a string,
  counting from 0. An index that is not an integer, or indexing anything else,
  is a `type` error at the `[`; an index outside `0` to `len(x) - 1` is an
  `index` error at the `[`.
- **Comparison.** `==` and `!=` accept any two values: values of different
  types are unequal, lists are equal when their elements are, and functions
  only to themselves. `<`, `<=`, `>`, and `>=` compare two integers, or two
  strings by their characters' Unicode values; anything else is a `type` error
  at the operator.
- **Logic.** `and` and `or` evaluate their right operand only when needed.
  Every operand they evaluate must be a boolean, and `not`'s operand too; other
  values are a `type` error at the operator. `if` and `while` conditions must
  be booleans; other values are a `type` error at the `if` or `while`.
- **Other operators.** `+` with any other operand types, `-`, `*`, `/`, and `%`
  with anything but two integers, and `-` (negation) of anything but an
  integer, are `type` errors at the operator.
- **Printing.** `print x;` adds one line: an integer in decimal, such as `-12`;
  a string as its characters; `true`, `false`, and `nil`; a list as its
  elements in brackets, separated by `, `, with each string element in double
  quotes, such as `[1, "a", [true]]`; and a function as `<fn name>` when
  declared with a name, or `<fn>` otherwise.

## Notes

- Write your own test programs beside `tiny.py` to check your work. The
  graded suite covers every rule above, including error positions, and
  includes programs that run loops and recursion for some thousands of steps,
  which must finish in a few seconds.
