# Mustache templates

Write a Slug module, `./mustache.slug`, that renders Mustache templates. It
will be graded by a hidden suite of programs that import it with
`val mustache = import("mustache")`, render templates, and compare the output
exactly, character for character, including spaces and line endings.

Export exactly these two functions.

## `parse(template:str)`

Checks `template`'s syntax and returns it in a form `render` accepts in place
of the text, so a template used many times is parsed once. What it returns is
up to you. A malformed template throws a syntax error (see
[Errors](#errors)), even in parts that would never be rendered.

## `render(template, data, partials:map = {})`

Renders `template`, either its text or what `parse` returned for it, against
`data`, and returns the result as a string. `partials` maps names to template
text, for the partial and parent tags. Rendering the text and rendering its
parsed form give the same result.

`render` of a text template throws the errors `parse` would. Partials are
parsed when they are first rendered, so a malformed partial throws only if
it is rendered.

## Data

Data is built from maps with string keys, lists, strings, numbers,
booleans, and `nil`, nested freely; the hidden suite builds it from JSON or
as Slug literals. No other kinds of values are used.

**Truthiness.** `nil`, `false`, and an empty list are *falsy*. Every other
value is *truthy*, including `""`, `0`, and an empty map.

**As text.** A string is itself; `nil` is the empty string; a number or a
boolean is the text Slug gives it in `"" + value`, such as `85`, `1.21`,
`-2.5`, and `true`. No case interpolates a list or a map.

## Templates

A template is text with *tags*. A tag starts with the opening delimiter and
ends with the closing delimiter, `{{` and `}}` unless a set delimiter tag has
changed them. The character right after the opening delimiter decides the
tag's kind:

| Tag | Kind |
|---|---|
| `{{name}}` | variable, HTML-escaped |
| `{{{name}}}` | variable, not escaped |
| `{{&name}}` | variable, not escaped |
| `{{#name}}` … `{{/name}}` | section |
| `{{^name}}` … `{{/name}}` | inverted section |
| `{{!text}}` | comment |
| `{{>name}}` | partial |
| `{{<name}}` … `{{/name}}` | parent |
| `{{$name}}` … `{{/name}}` | block |
| `{{=open close=}}` | set delimiters |

Any other character, or none, starts a variable whose name is the tag's
whole content: `{{%x}}` names `%x`.

A tag's content runs from after the opening delimiter (and the kind's
character, if it has one) to the first occurrence of the closing delimiter
after it, except that a `{` tag ends at the first `}` followed by the
closing delimiter, and a set delimiter tag at the first `=` followed by the
closing delimiter. So a comment ends at the first closing delimiter, even
inside text that looks like a tag: `a{{! {{b}} }}c` renders `a }}c`. Text
outside tags is copied to the output unchanged, and a tag never spans into
the output.

A tag's *name* is its content with spaces, tabs, carriage returns, and line
feeds removed from both ends: `{{# list }}` names `list`, and
`{{{ name }}}` names `name`. A name must be non-empty and contain none of
those characters. A comment's content may be anything, including line
breaks.

## Names and the context stack

Rendering keeps a *context stack* of values. It starts holding just `data`.
A section pushes a value onto it while rendering its contents; the top is
the value pushed last.

A name resolves to a value like this:

- `.` is the value on top of the stack.
- Otherwise the name is split at each `.` into parts. The first part is
  looked up in each value on the stack from top to bottom; it is found in
  the first value that is a map containing that key, *even when the key's
  value is `nil`*. Maps that do not contain the key, and values that are not
  maps, are passed over. If no value on the stack contains it, the name
  resolves to `nil`.
- Each later part is looked up only in the value the parts before it gave,
  which must be a map containing that key; otherwise the whole name resolves
  to `nil`. No other values on the stack are tried, and lists are not
  indexed by number. `{{a.b.c}}` with `a.b` lacking `c` is the empty string
  even if the stack holds another `c`, and a key that itself contains a
  `.`, such as `"a.b"`, is never found.

## Variables

`{{name}}` is replaced by the value's text, HTML-escaped: `&` becomes
`&amp;`, `<` becomes `&lt;`, `>` becomes `&gt;`, and `"` becomes `&quot;`;
nothing else changes, not even `'`. `{{{name}}}` and `{{&name}}` insert the
text unescaped. Inserted text is never itself treated as a template.

## Sections

`{{#name}}` … `{{/name}}` renders its contents depending on the value:

- a non-empty list: once for each item in order, with the item pushed on the
  stack;
- any other truthy value: once, with the value pushed on the stack;
- a falsy value: not at all.

`{{^name}}` … `{{/name}}` renders its contents once, with nothing pushed,
when the value is falsy, and not at all otherwise.

An end tag `{{/name}}` closes the innermost open section, inverted section,
parent, or block, and must have the same name.

## Partials

`{{>name}}` renders the partial template `partials[name]` in place, with
the current context stack and the current block arguments (see
[Inheritance](#inheritance)). A missing partial renders as nothing. Partials
may render partials, including themselves.

A partial is parsed on its own, starting with the `{{` `}}` delimiters,
whatever delimiters the template that uses it has set.

When a partial tag is standalone (see [Standalone lines](#standalone-lines)),
the spaces and tabs before it on its line are its *indentation*: before it
is parsed, the partial's text gets the indentation inserted at its start and
after each line ending in it, except a line ending that is the partial's
last character. Blank lines are indented too. A partial tag that is not
standalone has no indentation. Text a variable inserts is not indented.

## Set delimiters

`{{=open close=}}` replaces the delimiters for the rest of the template
being parsed, including after the end of any section it is in. Its content,
between the `=` signs, must be exactly two words separated by spaces or
tabs, which may also surround them: `{{= <% %> =}}` sets `<%` and `%>`.
With new delimiters every kind of tag works the same way: `<%{name}%>`,
`<%#list%>`, and `<%={{ }}=%>`.

## Inheritance

A block `{{$name}}` … `{{/name}}` marks part of a template that a template
using it may replace. A parent tag `{{<name}}` … `{{/name}}` renders the
partial named `name`, like a partial tag, but passes *arguments*: each block
tag directly inside the parent tag is an argument, its contents replacing
the block of the same name in the partial. Everything else directly inside a
parent tag is ignored, including text, other tags, and blocks nested inside
other sections there.

Rendering keeps a set of block arguments, empty at the top level.

- A block renders the argument with its name when there is one, and its own
  contents otherwise. Either renders with the context stack and arguments in
  effect where the block tag is, not where the argument was written.
- A parent tag renders its partial with the arguments it passes, except that
  for any name the current arguments already hold, the current argument wins.
  So in a chain of parents, the outermost template's argument is used.
- A partial tag passes the current arguments on unchanged, as does a
  section.

A parent tag's partial is parsed and indented as a partial tag's is; a
missing one renders as nothing. A block outside any parent tag simply
renders its contents unless an argument replaces it.

## Standalone lines

A *line* is the text after a line feed (or the template's start) up to and
including the next line feed (or the template's end). A line is
*standalone* when it holds at least one tag, every tag on it is a section,
inverted section, end, comment, partial, parent, block, or set delimiter
tag, and everything else on it is spaces, tabs, and its line ending (a line
feed, or a carriage return and line feed). A tag belongs to the line it
starts on, so a comment spanning several lines can make the line it starts
on standalone; that line then runs to the line feed after the comment ends.

A standalone line renders only its tags: the spaces and tabs around them and
its line ending are removed. A partial or parent tag on a standalone line
takes the spaces and tabs before the line's first tag as its indentation.
Variable tags are never standalone, and a line with a variable or any other
text is not standalone.

```text
"| A\n  {{#t}}\n| B\n  {{/t}}\n| C\n"   with t true   renders   "| A\n| B\n| C\n"
"| A {{#t}}{{/t}} B\n"                                 renders   "| A  B\n"
```

## Errors

A malformed template throws `Error {type: "mustache.syntax", msg, data:
{line: n}}`, where `n` is the line the fault is on: one more than the number
of line feeds before the opening delimiter of the offending tag, counted in
the template or partial being parsed. `msg` is up to you. Each of these is a
syntax error, reported at the tag named:

- an opening delimiter with no closing delimiter after it (for a `{` tag, no
  `}` and closing delimiter; for a set delimiter tag, no `=` and closing
  delimiter): the opening tag;
- a name that is empty or contains a space, tab, carriage return, or line
  feed: that tag;
- a set delimiter tag that does not hold exactly two delimiters: that tag;
- an end tag with no open section, inverted section, parent, or block, or
  whose name differs from the innermost open one: the end tag;
- a section, inverted section, parent, or block with no end tag: the tag
  that opens it.

An end tag that does not match is reported rather than the sections it
leaves open. No case has any other combination of faults.
