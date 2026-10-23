"""Writes cases.json, the HTTP project's hidden suite, for
eval/agent/harness/http_grader.py.

Each case is an exchange with one of two servers that server.slug starts:
`default`, with default options, and `tight`, with small limits and
timeouts. Expected responses are written out in full, so a case shows
exactly what the server must send.

    python3 -I eval/agent/projects/http/make_cases.py
"""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

REASONS = {
    100: "Continue", 200: "OK", 201: "Created", 204: "No Content", 301: "Moved Permanently",
    302: "Found", 304: "Not Modified", 400: "Bad Request", 404: "Not Found",
    405: "Method Not Allowed", 408: "Request Timeout", 413: "Content Too Large",
    414: "URI Too Long", 431: "Request Header Fields Too Large",
    500: "Internal Server Error", 501: "Not Implemented", 505: "HTTP Version Not Supported",
}

TEXT = "text/plain; charset=utf-8"
TIGHT = "{idleTimeout: 500, headerTimeout: 500, maxHeaderBytes: 512, maxBodyBytes: 64}"

cases = []


def case(id, steps, **extra):
    cases.append({"id": id, "steps": steps, **extra})


def request(method, target, headers=(), body="", version="HTTP/1.1", host=True):
    lines = [f"{method} {target} {version}"]
    if host:
        lines.append("Host: test")
    lines += [f"{name}: {value}" for name, value in headers]
    return "\r\n".join(lines) + "\r\n\r\n" + body


def send(text):
    return {"send": text}


def response(status, body="", headers=None, content_type=None, close=False, no_body=False, length=None):
    """The expected response: Content-Length, a default or given
    Content-Type for a body, and Connection: close, as the spec has them."""
    if isinstance(body, dict) and "repeat" in body:
        size = len(body["repeat"][0]) * body["repeat"][1]
    elif isinstance(body, dict):
        size = len(body["hex"]) // 2
    else:
        size = len(body.encode())
    expected = {name.lower(): value for name, value in (headers or {}).items()}
    if status not in (204, 304):
        expected["content-length"] = str(length if length is not None else size)
        if size and "content-type" not in expected:
            expected["content-type"] = content_type or TEXT
    if close:
        expected["connection"] = "close"
    result = {"status": status, "reason": REASONS.get(status, "Unknown"), "headers": expected}
    if no_body:
        result["no_body"] = True
    elif status not in (204, 304):
        result["body"] = body
    else:
        result["no_body"] = True
    return {"expect": result}


def rejected(status):
    return [response(status, close=True), {"closed": True}]


def echo(method="GET", path="/echo", params=None, query=None, headers=None, body=""):
    """What server.slug's echo handler answers: the request as sorted lines."""
    lines = [f"method={method}", f"path={path}"]
    lines += [f"param.{name}={value}" for name, value in (params or {}).items()]
    lines += [f"query.{name}={value}" for name, value in (query or {}).items()]
    lines += [f"header.{name}={value}" for name, value in {"host": "test", **(headers or {})}.items()]
    return "\n".join(sorted(lines) + [f"body={body}"])


def echo_response(**fields):
    return response(200, echo(**fields))


# --- Responses ---------------------------------------------------------------

case("get-a-string-response", [send(request("GET", "/hello")), response(200, "hello")])
case("head-has-the-get-headers-and-no-body",
     [send(request("HEAD", "/hello")), response(200, "hello", no_body=True), send(request("GET", "/hello")),
      response(200, "hello")])
case("empty-body-has-no-content-type", [send(request("GET", "/empty")), response(200, "")])
case("bytes-body-is-octet-stream",
     [send(request("GET", "/bytes")), response(200, {"hex": "00ff10"}, content_type="application/octet-stream")])
case("handler-content-type-replaces-the-default",
     [send(request("GET", "/typed")), response(200, "{}", headers={"Content-Type": "application/json"})])
case("handler-content-length-is-ignored", [send(request("GET", "/wrong-length")), response(200, "four")])
case("handler-headers-are-sent", [send(request("GET", "/headers")),
                                  response(200, "h", headers={"X-One": "1", "X-Two": "two"})])
case("created-with-location", [send(request("POST", "/items")),
                               response(201, "created", headers={"Location": "/items/1"})])
case("no-content-has-no-body-or-length", [send(request("GET", "/status/204")), response(204),
                                          send(request("GET", "/hello")), response(200, "hello")])
case("not-modified-has-no-body-or-length", [send(request("GET", "/status/304")), response(304),
                                            send(request("GET", "/hello")), response(200, "hello")])
case("found-has-its-reason", [send(request("GET", "/status/302")), response(302, "status body")])
case("unlisted-status-is-unknown", [send(request("GET", "/status/299")), response(299, "status body")])
case("handler-404-has-its-body", [send(request("GET", "/status/404")), response(404, "status body")])
case("throwing-handler-is-500-and-the-connection-stays-open",
     [send(request("GET", "/throw")), response(500), send(request("GET", "/hello")), response(200, "hello")])
case("large-response", [send(request("GET", "/big")), response(200, {"repeat": ["0123456789abcdef", 65536]})],
     seconds=20)

# --- Routing -----------------------------------------------------------------

case("first-matching-route-wins", [send(request("GET", "/users/me")), response(200, "me")])
case("path-parameter", [send(request("GET", "/users/7")),
                        echo_response(path="/users/7", params={"id": "7"})])
case("two-path-parameters", [send(request("GET", "/users/7/posts/12")),
                             echo_response(path="/users/7/posts/12", params={"id": "7", "post": "12"})])
case("rest-parameter", [send(request("GET", "/files/a/b/c.txt")),
                        echo_response(path="/files/a/b/c.txt", params={"rest": "a/b/c.txt"})])
case("rest-parameter-may-be-empty", [send(request("GET", "/files/")),
                                     echo_response(path="/files/", params={"rest": ""})])
case("rest-parameter-needs-a-segment", [send(request("GET", "/files")), response(404)])
case("parameter-needs-a-non-empty-segment", [send(request("GET", "/users/")), response(404)])
case("trailing-slash-is-a-segment", [send(request("GET", "/hello/")), response(404)])
case("no-route-is-404", [send(request("GET", "/nowhere")), response(404)])
case("wrong-method-is-405-with-allow", [send(request("POST", "/hello")),
                                        response(405, headers={"Allow": "GET, HEAD"})])
case("allow-lists-every-matching-route",
     [send(request("PUT", "/users/7")), response(405, headers={"Allow": "GET, HEAD, DELETE"})])
case("allow-lists-methods-in-route-order",
     [send(request("OPTIONS", "/echo")), response(405, headers={"Allow": "GET, HEAD, POST, PUT, PATCH, DELETE"})])
case("methods-are-case-sensitive", [send(request("get", "/hello")),
                                    response(405, headers={"Allow": "GET, HEAD"})])
case("head-on-a-parameter-route",
     [send(request("HEAD", "/users/7")),
      response(200, echo(method="HEAD", path="/users/7", params={"id": "7"}), no_body=True)])
case("delete-route", [send(request("DELETE", "/users/9")),
                      echo_response(method="DELETE", path="/users/9", params={"id": "9"})])
case("decoded-path-parameter", [send(request("GET", "/users/a%20b")),
                                echo_response(path="/users/a b", params={"id": "a b"})])
case("encoded-slash-matches-decoded", [send(request("GET", "/files/a%2Fb")),
                                       echo_response(path="/files/a/b", params={"rest": "a/b"})])
case("utf8-path", [send(request("GET", "/users/%C3%A9t%C3%A9")),
                   echo_response(path="/users/été", params={"id": "été"})])

# --- Parsing -----------------------------------------------------------------

case("query-parameters", [send(request("GET", "/echo?a=1&b=two")), echo_response(query={"a": "1", "b": "two"})])
case("query-plus-and-percent", [send(request("GET", "/echo?q=a+b%26c")), echo_response(query={"q": "a b&c"})])
case("query-parameter-without-value", [send(request("GET", "/echo?flag")), echo_response(query={"flag": ""})])
case("query-repeated-name-last-wins", [send(request("GET", "/echo?a=1&a=2")), echo_response(query={"a": "2"})])
case("query-empty-parts-are-skipped", [send(request("GET", "/echo?&a=1&")), echo_response(query={"a": "1"})])
case("query-encoded-name", [send(request("GET", "/echo?a%20b=c")), echo_response(query={"a b": "c"})])
case("empty-query", [send(request("GET", "/echo?")), echo_response()])
case("header-names-lowercased-and-values-trimmed",
     [send(request("GET", "/echo", [("X-Thing", "   value  ")])), echo_response(headers={"x-thing": "value"})])
case("header-tabs-trimmed", [send(request("GET", "/echo", [("X-Tab", "\tv\t")])),
                             echo_response(headers={"x-tab": "v"})])
case("repeated-headers-joined",
     [send(request("GET", "/echo", [("X-A", "1"), ("x-a", "2")])), echo_response(headers={"x-a": "1, 2"})])
case("empty-header-value", [send(request("GET", "/echo", [("X-Empty", "")])), echo_response(headers={"x-empty": ""})])
case("header-value-keeps-inner-colons", [send(request("GET", "/echo", [("X-Time", "12:30:45")])),
                                         echo_response(headers={"x-time": "12:30:45"})])
case("bare-lf-line-endings", [send("GET /hello HTTP/1.1\nHost: test\n\n"), response(200, "hello")])
case("leading-empty-lines-are-ignored", [send("\r\n\r\n" + request("GET", "/hello")), response(200, "hello")])
case("content-length-body",
     [send(request("POST", "/echo", [("Content-Length", "5")], "hello")),
      echo_response(method="POST", headers={"content-length": "5"}, body="hello")])
case("body-arriving-in-pieces",
     [send(request("POST", "/echo", [("Content-Length", "10")], "01234")), {"sleep": 0.2}, send("56789"),
      echo_response(method="POST", headers={"content-length": "10"}, body="0123456789")])
case("chunked-body",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n")),
      echo_response(method="POST", headers={"transfer-encoding": "chunked"}, body="Wikipedia")])
case("chunked-extensions-and-uppercase-hex",
     [send(request("PUT", "/echo", [("Transfer-Encoding", "chunked")], "A;name=value\r\n0123456789\r\n0\r\n\r\n")),
      echo_response(method="PUT", headers={"transfer-encoding": "chunked"}, body="0123456789")])
case("chunked-trailers-are-discarded",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "3\r\nabc\r\n0\r\nX-Trailer: t\r\n\r\n")),
      echo_response(method="POST", headers={"transfer-encoding": "chunked"}, body="abc")])
case("transfer-coding-is-case-insensitive",
     [send(request("POST", "/echo", [("Transfer-Encoding", "Chunked")], "2\r\nhi\r\n0\r\n\r\n")),
      echo_response(method="POST", headers={"transfer-encoding": "Chunked"}, body="hi")])
case("chunked-body-in-pieces",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "3\r\nab")), {"sleep": 0.2},
      send("c\r\n0\r\n\r\n"), echo_response(method="POST", headers={"transfer-encoding": "chunked"}, body="abc")])
case("equal-repeated-content-lengths",
     [send(request("POST", "/echo", [("Content-Length", "2"), ("Content-Length", "2")], "hi")),
      echo_response(method="POST", headers={"content-length": "2, 2"}, body="hi")])
case("non-utf8-body-is-bytes",
     [{"send_hex": (request("POST", "/echo", [("Content-Length", "1")]).encode() + b"\xff").hex()},
      echo_response(method="POST", headers={"content-length": "1"}, body="<bytes>")])
case("http-1-0-needs-no-host",
     [send(request("GET", "/hello", version="HTTP/1.0", host=False)), response(200, "hello", close=True),
      {"closed": True}])

# --- Rejections ----------------------------------------------------------------

case("missing-host-is-400", [send(request("GET", "/hello", host=False))] + rejected(400))
case("request-line-without-version-is-400", [send("GET /hello\r\nHost: test\r\n\r\n")] + rejected(400))
case("absolute-target-is-400", [send(request("GET", "http://test/hello"))] + rejected(400))
case("invalid-method-is-400", [send(request("GE(T", "/hello"))] + rejected(400))
case("other-http-version-is-505", [send(request("GET", "/hello", version="HTTP/2.0"))] + rejected(505))
case("http-1-2-is-505", [send(request("GET", "/hello", version="HTTP/1.2"))] + rejected(505))
case("non-http-version-is-400", [send(request("GET", "/hello", version="HTTX/1.1"))] + rejected(400))
case("header-without-colon-is-400", [send("GET /hello HTTP/1.1\r\nHost: test\r\nNoColon\r\n\r\n")] + rejected(400))
case("space-before-colon-is-400", [send("GET /hello HTTP/1.1\r\nHost : test\r\n\r\n")] + rejected(400))
case("folded-header-is-400", [send("GET /hello HTTP/1.1\r\nHost: test\r\nX-A: 1\r\n 2\r\n\r\n")] + rejected(400))
case("bare-cr-is-400", [send("GET /hello HTTP/1.1\r\nHost: te\rst\r\n\r\n")] + rejected(400))
case("invalid-percent-encoding-is-400", [send(request("GET", "/users/%zz"))] + rejected(400))
case("invalid-content-length-is-400",
     [send(request("POST", "/echo", [("Content-Length", "abc")]))] + rejected(400))
case("negative-content-length-is-400",
     [send(request("POST", "/echo", [("Content-Length", "-1")]))] + rejected(400))
case("differing-content-lengths-are-400",
     [send(request("POST", "/echo", [("Content-Length", "3"), ("Content-Length", "4")], "abcd"))] + rejected(400))
case("content-length-and-transfer-encoding-is-400",
     [send(request("POST", "/echo", [("Content-Length", "3"), ("Transfer-Encoding", "chunked")], "abc"))]
     + rejected(400))
case("unknown-transfer-coding-is-501",
     [send(request("POST", "/echo", [("Transfer-Encoding", "gzip")], "abc"))] + rejected(501))
case("chunked-not-alone-is-501",
     [send(request("POST", "/echo", [("Transfer-Encoding", "gzip, chunked")], "0\r\n\r\n"))] + rejected(501))
case("bad-chunk-size-is-400",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "zz\r\nab\r\n0\r\n\r\n"))] + rejected(400))
case("chunk-without-its-line-ending-is-400",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "2\r\nabXX0\r\n\r\n"))] + rejected(400))
case("rejection-ignores-pipelined-requests",
     [send(request("GET", "/hello", host=False) + request("GET", "/hello"))] + rejected(400))
case("body-over-the-limit-is-413-before-it-arrives",
     [send(request("POST", "/echo", [("Content-Length", "100")]))] + rejected(413), server="tight")
case("chunked-body-over-the-limit-is-413",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked")], "28\r\n" + "x" * 40 + "\r\n28\r\n" + "x" * 40
                   + "\r\n0\r\n\r\n"))] + rejected(413), server="tight")
case("body-at-the-limit-is-accepted",
     [send(request("POST", "/echo", [("Content-Length", "64")], "y" * 64)),
      echo_response(method="POST", headers={"content-length": "64"}, body="y" * 64)], server="tight")
case("header-section-over-the-limit-is-431",
     [send(request("GET", "/hello", [("X-Big", "z" * 600)]))] + rejected(431), server="tight")
case("request-line-over-the-limit-is-414",
     [send(request("GET", "/hello?" + "q" * 600))] + rejected(414), server="tight")
case("default-header-limit-is-8192",
     [send(request("GET", "/hello", [("X-Big", "z" * 9000)]))] + rejected(431))
case("default-header-limit-allows-large-headers",
     [send(request("GET", "/echo", [("X-Big", "z" * 7000)])), echo_response(headers={"x-big": "z" * 7000})])
case("default-body-limit-is-a-mebibyte",
     [send(request("POST", "/echo", [("Content-Length", "1048577")]))] + rejected(413))
case("body-of-a-mebibyte-is-accepted",
     [send(request("POST", "/echo", [("Content-Length", "1048576")])), {"send_repeat": ["b", 1048576]},
      echo_response(method="POST", headers={"content-length": "1048576"}, body="b" * 1048576)], seconds=20)

# --- Persistence ---------------------------------------------------------------

case("two-requests-on-one-connection",
     [send(request("GET", "/hello")), response(200, "hello"), send(request("GET", "/users/me")), response(200, "me")])
case("pipelined-requests-answered-in-order",
     [send(request("GET", "/hello") + request("GET", "/users/me") + request("GET", "/status/404")),
      response(200, "hello"), response(200, "me"), response(404, "status body")])
case("pipelined-requests-with-bodies",
     [send(request("POST", "/echo", [("Content-Length", "1")], "a") + request("POST", "/echo", [("Content-Length", "2")], "bc")),
      echo_response(method="POST", headers={"content-length": "1"}, body="a"),
      echo_response(method="POST", headers={"content-length": "2"}, body="bc")])
case("connection-close-is-honored",
     [send(request("GET", "/hello", [("Connection", "close")])), response(200, "hello", close=True), {"closed": True}])
case("connection-close-among-tokens",
     [send(request("GET", "/hello", [("Connection", "keep-alive, Close")])), response(200, "hello", close=True),
      {"closed": True}])
case("http-1-0-closes", [send(request("GET", "/hello", version="HTTP/1.0")), response(200, "hello", close=True),
                         {"closed": True}])
case("http-1-0-closes-despite-keep-alive",
     [send(request("GET", "/hello", [("Connection", "keep-alive")], version="HTTP/1.0")),
      response(200, "hello", close=True), {"closed": True}])
case("client-half-close-gets-its-response",
     [send(request("GET", "/hello")), {"shut_write": True}, response(200, "hello"), {"closed": True}])
case("client-closing-partway-gets-no-response",
     [send("GET /hello HTTP/1.1\r\nHost: te"), {"shut_write": True}, {"closed": True}])
case("client-closing-partway-through-a-body-gets-no-response",
     [send(request("POST", "/echo", [("Content-Length", "10")], "abc")), {"shut_write": True}, {"closed": True}])

# --- Timeouts ------------------------------------------------------------------

case("idle-connection-closes-without-a-response", [{"silent": 0.3}, {"closed": True}], server="tight")
case("idle-after-a-response-closes", [send(request("GET", "/hello")), response(200, "hello"), {"silent": 0.3},
                                      {"closed": True}], server="tight")
case("keep-alive-within-the-idle-timeout",
     [send(request("GET", "/hello")), response(200, "hello"), {"sleep": 0.3}, send(request("GET", "/hello")),
      response(200, "hello")], server="tight")
case("incomplete-header-section-times-out-with-408",
     [send("GET /hello HTTP/1.1\r\nHo")] + rejected(408), server="tight")
case("header-section-in-pieces-within-the-timeout",
     [send("GET /hello HT"), {"sleep": 0.15}, send("TP/1.1\r\nHost: "), {"sleep": 0.15}, send("test\r\n\r\n"),
      response(200, "hello")], server="tight")
# Every gap is shorter than the timeout, so only a deadline counted from the
# first byte answers by 500 ms; one restarted by each read would wait until
# about 950 ms.
case("header-timeout-counts-the-whole-section",
     [send("GET /hello HTTP/1.1\r\n"), {"sleep": 0.25}, send("X-A: 1\r\n"), {"sleep": 0.2}, send("X-B: 2\r\n"),
      {**response(408, close=True), "within": 0.35}, {"closed": True}], server="tight")

# --- Expect: 100-continue -------------------------------------------------------

case("expect-continue-before-the-body",
     [send(request("POST", "/echo", [("Content-Length", "4"), ("Expect", "100-continue")])),
      {"expect": {"status": 100, "reason": "Continue", "headers": {}, "no_body": True}}, send("data"),
      echo_response(method="POST", headers={"content-length": "4", "expect": "100-continue"}, body="data")])
case("expect-continue-without-a-body-sends-none",
     [send(request("GET", "/hello", [("Expect", "100-continue")])), response(200, "hello")])
case("expect-continue-with-a-chunked-body",
     [send(request("POST", "/echo", [("Transfer-Encoding", "chunked"), ("Expect", "100-continue")])),
      {"expect": {"status": 100, "reason": "Continue", "headers": {}, "no_body": True}},
      send("2\r\nok\r\n0\r\n\r\n"),
      echo_response(method="POST", headers={"transfer-encoding": "chunked", "expect": "100-continue"}, body="ok")])
case("expect-continue-over-the-limit-is-413-without-continue",
     [send(request("POST", "/echo", [("Content-Length", "100"), ("Expect", "100-continue")]))] + rejected(413),
     server="tight")

# --- Concurrency -----------------------------------------------------------------

cases.append({"id": "slow-requests-are-served-concurrently", "kind": "concurrent", "clients": 50, "within": 3.0,
              "steps": []})

suite = {
    "kind": "http",
    "sandbox": "loopback",
    "servers": {"default": "{}", "tight": TIGHT},
    "cases": cases,
}
ids = [entry["id"] for entry in cases]
assert len(ids) == len(set(ids)), "case ids must be unique"
(HERE / "cases.json").write_text(json.dumps(suite, indent=1, ensure_ascii=False) + "\n")
print(f"{len(cases)} cases")
