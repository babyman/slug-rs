# Standard input

The `slug.io.stdin` module provides process standard input as a shared stream
of text lines and small interactive-console helpers.

```slug
val { readLine, readLines, prompt, confirm } = import("slug.io.stdin")
```

`readLines():chan<str|nil>` returns the one channel associated with the
process's standard input. Repeated calls return that same channel. It is a
single-consumer stream: receiving through two references distributes lines
between their receivers and does not broadcast them.

Each input line is emitted as a `str` without its final `\n`; a preceding `\r`
in a CRLF ending is also removed. Empty lines are emitted as `""`, and an
unterminated final line is emitted before end of input. At end of input the
channel closes, so `slug.channel.recv` returns `nil` after any already-buffered
lines. A host input read failure also closes the stream.

`readLine():str|nil` receives the next line from `readLines()`. `prompt(message)`
writes `message` with no added newline and returns `readLine()`. `confirm`
writes its message with `[Y/n]` or `[y/N]` according to its default. It returns
true for `y`, `yes`, `true`, or `1`, case variants shown by the module source;
it returns false for their negative counterparts. Empty, unrecognized, and
end-of-input values return the supplied default.

The host implementation may buffer a finite number of unread lines, but the
buffer size is not part of the public API. It must apply backpressure rather
than silently discard accepted input.
