# HTTP server library

Write a Slug module, `./http.slug`, that serves HTTP/1.1 over `slug.net.tcp`,
for building web apps. It will be graded by a hidden suite: a server program
that imports it with `val http = import("http")`, defines routes and handlers
with it, and serves them, while a client sends it requests over loopback and
checks each response byte for byte. Also write `./app.slug`, a small example
app, described at the end.

Your environment allows network connections on loopback only, so you can
run a server on `127.0.0.1` and test it with a client: a Slug program using
`slug.net.tcp`, or `curl`.

Export exactly the functions below.

## `serve(listener, handler, options = {})`

Serves HTTP on `listener`, a `tcp.Listener` that is already listening, until
the listener is closed, then returns `nil`. The grader's server program looks
like this:

```slug
val tcp = import("slug.net.tcp")
val http = import("http")

val listener = tcp.listen("127.0.0.1", 0)
println(tcp.localPort(listener))
http.serve(listener, http.router([
    ["GET", "/hello", fn(request) { "hello" }],
]), {idleTimeout: 2000})
```

Each connection is served in its own task, so a slow or idle client never
delays another. On one connection, requests are handled one at a time, in
order, and each gets its response before the next is read.

`handler` is called with each request and returns its response. `options` may
hold:

| Option | Default | Meaning |
|---|---|---|
| `idleTimeout` | 5000 | Milliseconds a connection may wait for the start of its next request before the server closes it, without a response. |
| `headerTimeout` | 5000 | Milliseconds from the first byte of a request until its header section is complete. When it elapses, the server responds `408` and closes the connection. |
| `maxHeaderBytes` | 8192 | The largest request line plus header section, in bytes, counting line endings and the blank line. |
| `maxBodyBytes` | 1048576 | The largest request body, in bytes, after chunked decoding. |

## Requests

A request is a map:

| Key | Value |
|---|---|
| `method` | The method, as sent, such as `"GET"`. Methods are case-sensitive. |
| `path` | The target's path, percent-decoded, such as `"/files/a b"` for `/files/a%20b`. |
| `query` | A map of the query string's parameters, percent-decoded, with `+` as a space. A parameter without `=` has the value `""`. When a name repeats, the last value wins. |
| `headers` | A map from each header name, lowercased, to its value without surrounding spaces or tabs. When a name repeats, the values are joined with `", "` in order. |
| `body` | The body as bytes; `0x""` when there is none. |
| `params` | The path parameters `router` matched, as a map; `{}` otherwise. |

## Responses

A handler returns a response, either a string, the body of a `200` response
whose `Content-Type` is `text/plain; charset=utf-8`, or a map:

| Key | Value |
|---|---|
| `status` | The status code, an integer from 100 to 599 other than 1xx. |
| `headers` | Optional: a map from header name to value, written as given. |
| `body` | Optional: a string, sent as UTF-8, or bytes. Absent means empty. |

The server writes the status line `HTTP/1.1 <status> <reason>` with the
reason phrase from the table below, or `Unknown` for any other status, then
the handler's headers in any order, then these, unless the handler set a
header of the same name, compared case-insensitively:

- `Content-Length`, the body's length in bytes, always;
- `Content-Type: text/plain; charset=utf-8` for a string body and
  `application/octet-stream` for a bytes body, when the body is not empty;
- `Connection: close` when the server closes the connection after this
  response.

A handler's `Content-Length` is ignored: the server always sends the body's
real length. A response to `HEAD` has the headers a `GET` would have,
including its `Content-Length`, and no body.

When a handler throws, the response is `500` with an empty body, and the
connection stays open.

| Status | Reason |
|---|---|
| 200 | OK |
| 201 | Created |
| 204 | No Content |
| 301 | Moved Permanently |
| 302 | Found |
| 304 | Not Modified |
| 400 | Bad Request |
| 404 | Not Found |
| 405 | Method Not Allowed |
| 408 | Request Timeout |
| 413 | Content Too Large |
| 414 | URI Too Long |
| 431 | Request Header Fields Too Large |
| 500 | Internal Server Error |
| 501 | Not Implemented |
| 505 | HTTP Version Not Supported |

A `204` or `304` response has no body and no `Content-Length` or
`Content-Type`, whatever the handler returned.

## `router(routes)`

Returns a handler that dispatches by method and path. `routes` is a list of
`[method, pattern, handler]` entries. A pattern is a path whose segments are
literal, `:name`, which matches one non-empty segment and records it in
`params` under `name`, or a final `*name`, which matches the remaining
segments, one or more, and records them joined with `/`. Segments are what
lies between `/`s, so a trailing `/` ends with an empty segment: `/users/`
does not match `/users`. `/users/:id` matches `/users/7` with `{id: "7"}`,
and `/files/*rest` matches `/files/a/b` with `{rest: "a/b"}` and `/files/`
with `{rest: ""}`, but not `/files`. Matching is against the decoded path.

The first route whose pattern and method match handles the request. A `GET`
route also handles `HEAD`. When some route's pattern matches but none of
their methods does, the response is `405` with an `Allow` header listing the
methods of those routes, `HEAD` included where `GET` is, in the order the
routes first list them, separated by `", "`. When no pattern matches, the
response is `404`. Both have empty bodies.

## The protocol

The server reads requests as RFC 9112 describes, with these rules and limits.

**Request line.** `method SP target SP version`, where the target starts with
`/` and the version is `HTTP/1.1` or `HTTP/1.0`. Any other `HTTP/x.y` version
is `505`; a line that is not of this form is `400`. A request line longer
than `maxHeaderBytes` is `414`. Empty lines before a request line are
ignored.

**Line endings.** Lines end with CRLF; a bare LF is also accepted. A bare CR
is `400`.

**Headers.** `name: value`. A name is one or more token characters (letters,
digits, and ``!#$%&'*+-.^_`|~``); anything else, including a space before the
colon, is `400`. A line that starts with a space or tab, continuing the line
before, is `400`. A header section, with the request line, larger than
`maxHeaderBytes` is `431`. An HTTP/1.1 request without `Host` is `400`.

**Bodies.** A request has a body when it has `Content-Length` or
`Transfer-Encoding`. A `Content-Length` that is not a decimal integer, or
several that differ, is `400`. `Transfer-Encoding: chunked` is decoded:
chunk sizes are hexadecimal, chunk extensions after `;` are ignored, and
trailer fields after the last chunk are read and discarded. A request with
both `Transfer-Encoding` and `Content-Length` is `400`. Any transfer coding
other than `chunked`, alone and last, is `501`. A body larger than
`maxBodyBytes` is `413`, sent as soon as the length is known, after which the
connection closes.

**`Expect: 100-continue`.** When a request asks for it and has a body, the
server sends `HTTP/1.1 100 Continue` followed by a blank line before reading
the body, unless it is answering with `413` without reading it.

**Persistence.** An HTTP/1.1 connection stays open after each response, unless
the request has `Connection: close`, which the response then also sends. An
HTTP/1.0 connection closes after each response. A client may send its next
requests before it has read a response: the server answers them in order.

**Errors.** After a `400`, `408`, `413`, `414`, `431`, `501`, or `505`, the
server sends `Connection: close` and closes the connection, since it may not
know where the next request starts. These responses have empty bodies. When
the client closes or the connection fails partway through a request, the
server closes the connection without a response.

## `app.slug`

A program that serves an example app with your library on port 8080 of
`127.0.0.1`: a small notes service with routes to list, create, read, and
delete notes kept in memory, which shows how the library is used. It may use
`slug.fmt.json` for its bodies. It is read, not graded, so keep it short and
clear.
