# Uploads, binary files, and content hashes

## Purpose

Let a Slug web app accept large uploads and keep them as files, without
holding a whole upload in memory or stalling other requests. Orble's files
milestone needs bounded uploads, immutable blobs named by their hash,
downloads, and file operations to manage them (Orble's
`docs/slug-development.md`, "Upstream needs").
The same filesystem operations give test tooling its temporary directories.

## What exists

- `slug.net.http` reads each request body in full before calling the
  handler, as `request.body` bytes, up to the server's `maxBodyBytes` (1 MiB
  by default). It decodes chunked bodies and answers `Expect: 100-continue`
  as soon as the head is read, before the handler can decide to refuse. The
  reader grows its buffer by concatenation, so a large body is copied
  repeatedly. `form` decodes URL-encoded bodies; there is no multipart.
- Responses can already stream: a body may be a channel of strings or bytes.
- `slug.io.fs` opens text files and reads them line by line, and writes
  text. It cannot read or write bytes, list or make directories, rename,
  remove, report a file's size, or flush a file to disk.
- No library hashes anything.

## Design

Each part below is usable alone. They land in the order 1, 2 with 4, then
3.

### 1. Binary files and directories in `slug.io.fs`

Done 2026-11-15, with the producer lists below. macOS and Linux; the untested
Windows library entry was dropped, as `slug.net.tcp` never had one.

Files, added to the existing `File` resource:

- `read(file, maxBytes = 65536):bytes|nil`: the next bytes, at most
  `maxBytes`, or `nil` at end of file.
- `write(file, content:str|bytes)`: accepts bytes as well as text.
- `sync(file)`: flushes the file to the device (`fsync`), for writes that
  must survive a crash.
- `readLine` stays as it is, for text.

Paths:

- `stat(path):map|nil`: `{kind: "file"|"directory"|"other", size,
  modifiedMs}`, or `nil` when nothing is there. `modifiedMs` matches
  `slug.time.nowMs`.
- `list(directory):list<str>`: entry names, sorted, without `.` and `..`.
- `makeDirectory(path, parents = false)`.
- `rename(from, to)`: atomic replacement within one filesystem, the step that
  makes a fully written file visible under its final name.
- `remove(path)`: a file or an empty directory. `removeAll(path)` removes a
  tree; it does not follow symbolic links.
- `temporaryDirectory(prefix = "slug-")`: a new, private directory under the
  system's temporary location (`mkdtemp`), which the caller removes.

Writing a blob durably is then ordinary Slug: write to a temporary name in
the same directory, `sync`, `rename` to the final name. The module documents
that recipe instead of wrapping it.

Errors stay `Error` values of type `native.io`, with the system's reason in
`msg` and its error code in `data`, so a caller can tell "not found" from
"permission denied".

#### Lists through producers

Producers send one scalar per message: nil, bool, integer, float, text, or
bytes. Producer API version 2 adds `send_list(producer, items, count)`, which
sends a flat list of those scalars as one message. Nested lists and maps stay
out; the Slug side builds a map from a list in one line.

- One message per reply arrives whole or not at all, so a wrapper never sees
  half a reply when the channel closes between sends.
- One wake of the waiting task per reply, not one per value.
- The new function is appended to the API table, so libraries built against
  version 1 keep working.

Process control and an HTTP client need multi-value replies too; TCP's
`chan<bytes|num>`, where a number is an error code, can move to it later.

#### Off the scheduler

Every file operation runs on one shared native I/O thread, as TCP's reads
and writes run on native threads, so only the calling task waits on the
disk. `readLine`, `openRead`, and the other existing calls move there too.

- The module's native profile becomes `events`. An operation queues a request
  with a producer for a reply channel the Slug wrapper made, then waits on
  that channel with `select`, parking only its task.
- A reply is one message, a flat list `[status, value, ...]`, the shape the
  synchronous natives already return (`readLineRaw` gives `[0, line]`). The
  Slug wrapper matches it into a value, a map, or an `Error`.
- `write` waits for its reply like every other operation, parking only its
  task, so a failure is reported by the `write` that caused it. A queue that
  returns at once, as `tcp.write` does, can come later if measurement shows
  writers waiting on the disk; it would report failures at a later call.
- One thread serves every file in order, which keeps each file's operations
  ordered without locking. If one slow disk holding up the others shows up
  in practice, the queue can later move to a small pool keyed by file.

### 2. Streamed request bodies in `slug.net.http`

The server stops reading the body before calling the handler. The handler
asks for it, with a limit that suits the route:

- `http.body(request, maxBytes = options.maxBodyBytes):bytes`: the whole
  body, as `request.body` was. A second call returns the same bytes.
- `http.chunks(request, maxBytes):chan<bytes>`: the body as it arrives, as
  `tcp.chunks` gives a connection's bytes. The channel closes at the end of
  the body. Passing `maxBytes` is required, so each streaming route states
  how large an upload it accepts. Past it, the server answers `413`.
- `form(request)` reads the body through `body`, so callers of `form` are
  unchanged.

Consequences:

- `request.body` goes away. Crawl's web module and Orble's auth handlers move
  to `http.body(request)` or `form`. This is a breaking change.
- `100 Continue` is sent when the handler first reads the body. A handler that
  refuses a request without reading it (`401`, `403`, `413` by
  `Content-Length`) saves the client from uploading the body.
- A body the handler did not finish reading closes the connection after the
  response, rather than reading and discarding an upload the handler
  refused.
- The idle timeout applies to each piece of a streamed body, as it does now.
- The reader collects pieces in a list and joins them once, so `body` no
  longer copies a large body repeatedly.

### 3. Content hashes: `slug.crypto.hash`

An incremental SHA-256, in a new native clutch with a small, public-domain C
implementation, no system dependencies:

```slug
val hash = import("slug.crypto.hash")

val digest = hash.sha256()
hash.update(digest, chunk)      // for each chunk
hash.hex(hash.finish(digest))   // "9f86d0…"
hash.hex(hash.sha256Of(data))   // all at once
```

An upload handler hashes each chunk as it writes it, so naming a blob by its
hash costs no second pass over the file.

### 4. Multipart forms

Browsers send file inputs as `multipart/form-data`, HTMX included. A
streaming parser in `slug.net.http`, written in Slug over `http.chunks`:

- `http.parts(request, maxBytes):chan<map>` yields each part as `{name,
  filename, contentType, chunks}`, where `chunks` is that part's own channel
  of bytes. The next part arrives once the previous part's channel is
  drained.
- Ordinary fields come as parts with no `filename`. A caller that wants them
  as a map reads those parts' bytes.

This is the largest piece, and it lands with part 2. A client that sends a
file as the raw request body (`fetch(url, {method: "PUT", body: file})`)
needs only part 2.

## Decisions

Settled 2026-11-15:

1. **File I/O stays off the scheduler.** It runs on a native I/O thread
   (part 1, "Off the scheduler"), not as short blocking calls, and not by
   asking programs to move I/O into a `fork`.
2. **`request.body` is removed.** Handlers call `http.body` or `http.chunks`;
   Crawl's web module and Orble's auth handlers move with the change.
3. **Multipart lands with streamed bodies.** Orble's upload screen is
   expected to be an HTMX form.

## Not in scope

File locking, watching for changes, memory-mapped files, permissions and
ownership, symbolic link creation, and `Range` requests for downloads. A
download is already a streamed response body: read the file in chunks and send
them through the body channel.
