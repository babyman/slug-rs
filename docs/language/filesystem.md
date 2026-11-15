# Filesystem resources

The `slug.io.fs` module reads and writes files, as text lines or bytes, and
inspects and changes directories:

```slug
val fs = import("slug.io.fs")

val file:fs.File = fs.openRead("records.csv")
defer fs.close(file)

match fs.readLine(file) {
  nil => "empty"
  line => line
}
```

## Waiting

Every operation runs on one native I/O thread. A call queues its work and
parks the calling task until the thread replies, so a slow or busy disk holds
up only that task; the program's other tasks run meanwhile. Operations run
in the order they are called, so each file's reads and writes keep their
order. In a runtime built without Slug-managed concurrency a call waits the
same way, with no other tasks to run.

## Files

`openRead(path)`, `openWrite(path)`, and `openAppend(path)` return opaque file
resources, represented by the nominal source type `File`. `openWrite` creates
or truncates its file; `openAppend` creates it when missing and writes at its
end. File resources are not numbers, structs, maps, or source-level
constructors. Their module ownership, resource kind, and open state are
validated by the native boundary.

`readLine(file):str|nil` returns one line without a final `\n`, also removing
the preceding `\r` from CRLF input. Empty lines are `""`; end of file is `nil`,
and a final line without a newline is still a line. `fs.readLine(file) ?? ""`
gives a `str`, with `""` at end of file. To bound native memory use, a line may
contain at most 16 MiB of UTF-8 bytes; reading a longer line, one containing a
NUL byte, or one that is not UTF-8 throws a `native.io` error.

`read(file, maxBytes = 65536):bytes|nil` returns the next bytes, at most
`maxBytes`, from 1 to 16 MiB, or `nil` at end of file. It may return fewer
bytes than remain. `read` and `readLine` share the file's position.

`write(file, content):num` writes all of `content`, text as UTF-8 or bytes,
and returns its length in bytes. `sync(file):nil` waits until what has been
written is on the file's device.

`close(file):nil` is idempotent. Any later operation on that handle produces a
checked resource-closed error. Programs must register `close` with `defer`
immediately after a successful open. Runtime destruction and shutdown may
release forgotten resources, but do not provide prompt release or observable
cleanup-error semantics. A file closes after the work already queued for it.

`File` distinguishes this opaque native handle from ordinary Slug values and
from every other resource type. See [Nominal types](nominal-types.md) for the
general resource rule.

## Paths

- `stat(path):map|nil` describes what is at `path` without following a final
  symbolic link, or is `nil` when nothing is there: `kind` is `"file"`,
  `"directory"`, `"link"`, or `"other"`, `size` is in bytes, and `modifiedMs`
  is milliseconds since the Unix epoch, as `slug.time.nowMs` gives.
- `list(directory):list<str>` gives the entries' names, sorted by byte value,
  without `.` and `..`.
- `makeDirectory(path, parents = false)` makes a directory; with `parents` it
  makes missing parents too, and a directory already there is not an error.
- `rename(from, to)` renames within one filesystem, replacing a file at `to`
  atomically: another program sees the old file or the new one, never
  neither.
- `remove(path)` removes a file, a symbolic link, or an empty directory.
  `removeAll(path)` removes a directory and everything under it, never
  following symbolic links; nothing at `path` is not an error.
- `temporaryDirectory(prefix = "slug-"):str` makes a new directory that only
  its owner can read, under `$TMPDIR` or `/tmp`, named `prefix` and six random
  characters, and returns its path. The caller removes it.

A file written durably is written under a temporary name in the same
directory, `sync`ed, and renamed into place.

## Errors

Failures throw `Error` values, which `defer onerror` can handle. A system
failure has type `native.io`, a `msg` naming the operation, the path where
there is one, and the reason, and `data` of `{code, errno}`:

```slug
Error {
  type: "native.io",
  msg: "cannot open notes.txt: no such file or directory",
  data: {code: "not_found", errno: 2},
}
```

`code` is one of `not_found`, `exists`, `permission`, `not_directory`,
`is_directory`, `not_empty`, `cross_device`, `no_space`, `read_only`,
`too_many_open`, `name_too_long`, `link_loop`, `wrong_mode` (reading a file
opened for writing, or writing one opened for reading), `invalid`, `busy`, or
`io`. It is the same on every platform; `errno` is the system's own number.
Running out of native memory throws `native.alloc`. Using a closed handle or
passing a value that is not an open `File` remains a checked boundary error
rather than a thrown value.

## Installation

The experimental CLI installation provides this module through the
`slug.io.fs` entry in `$SLUG_HOME/clutch/manifest.toml`, which selects the
`$SLUG_HOME/clutch/slug.io.fs.clutch` clutch. The module is Slug code over typed
native bindings declared in the clutch manifest, with the `events` native
profile for its replies. The clutch contains its C source and header under
`native/source/`; `make stage-native-clutches` builds the library for the
current platform. It supports macOS and Linux; other platforms receive a
checked clutch-load error.
