"""Grade an HTTP server module: start the project's server.slug with it, and
run each case's exchange over loopback sockets.

A case's steps run on one connection, in order:

- `{"send": text}` sends `text`, encoded as UTF-8; `{"send_repeat": [text,
  n]}` sends `text` `n` times; `{"send_hex": digits}` sends raw bytes.
- `{"sleep": seconds}` waits.
- `{"shut_write": true}` shuts down the client's sending side.
- `{"expect": response}` reads one response and compares its status line,
  its exact set of headers, with names lowercased, and its body. `body` is a
  string, `{"repeat": [text, n]}`, or `{"hex": digits}`; `no_body` reads
  none, as for `HEAD`. With `within`, the response must arrive in that many
  seconds.
- `{"closed": true}` expects the server to close the connection with nothing
  more sent; `{"silent": seconds}` expects nothing for that long.

A `concurrent` case holds one connection stalled partway through a request
while `clients` connections each fetch `/slow`, which takes 300 ms, and
passes when every one gets its response in less than `within` seconds.

Servers run once per grading, one for each options set in cases.json's
`servers`, and are restarted when one has exited.
"""

import os
import shutil
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

START_SECONDS = 15
DEFAULT_CASE_SECONDS = 5


class Failure(Exception):
    pass


class Server:
    def __init__(self, slug, slug_home, directory, name, options):
        self.slug, self.slug_home, self.directory = slug, slug_home, directory
        self.name, self.options = name, options
        self.process = None
        self.port = None

    def program(self):
        return self.directory / f"server-{self.name}.slug"

    def start(self, source):
        """Starts the server and reads its port. Its output goes to files, so
        a server that logs does not stall on a full pipe."""
        self.program().write_text(source.replace("{options}", self.options))
        self.output = self.directory / f"server-{self.name}.out"
        self.errors = self.directory / f"server-{self.name}.err"
        with self.output.open("wb") as output, self.errors.open("wb") as errors:
            self.process = subprocess.Popen(
                [self.slug, self.program().name],
                cwd=self.directory,
                stdout=output,
                stderr=errors,
                env={**os.environ, "SLUG_HOME": str(self.slug_home)},
            )
        deadline = time.monotonic() + START_SECONDS
        while time.monotonic() < deadline:
            first = self.output.read_text(errors="replace").split("\n")
            if len(first) > 1:
                try:
                    self.port = int(first[0].strip())
                    return
                except ValueError:
                    break
            if self.process.poll() is not None:
                break
            time.sleep(0.05)
        error = self.errors.read_text(errors="replace")[:1000]
        self.stop()
        raise Failure(f"server did not start: {error}")

    def alive(self):
        return self.process is not None and self.process.poll() is None

    def stop(self):
        if self.process is not None:
            self.process.kill()
            self.process.wait()
            self.process = None


def expected_body(body):
    if isinstance(body, dict) and "hex" in body:
        return bytes.fromhex(body["hex"])
    if isinstance(body, dict):
        text, times = body["repeat"]
        return (text * times).encode()
    return (body or "").encode()


class Connection:
    def __init__(self, port, seconds):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=seconds)
        self.socket.settimeout(seconds)
        self.buffer = b""

    def receive(self):
        try:
            data = self.socket.recv(65536)
        except socket.timeout:
            raise Failure("timed out waiting for the server") from None
        except ConnectionResetError:
            return b""
        return data

    def read_until(self, marker):
        while marker not in self.buffer:
            data = self.receive()
            if not data:
                raise Failure(f"connection closed before {marker!r}; got {self.buffer[:200]!r}")
            self.buffer += data
        index = self.buffer.index(marker) + len(marker)
        head, self.buffer = self.buffer[:index], self.buffer[index:]
        return head

    def read_exactly(self, count):
        while len(self.buffer) < count:
            data = self.receive()
            if not data:
                raise Failure(f"connection closed with {len(self.buffer)} of {count} body bytes")
            self.buffer += data
        body, self.buffer = self.buffer[:count], self.buffer[count:]
        return body

    def response(self, expect):
        head = self.read_until(b"\r\n\r\n").decode("latin-1")
        lines = head.split("\r\n")[:-2]
        status_line, fields = lines[0], lines[1:]
        headers = {}
        for field in fields:
            if ":" not in field:
                raise Failure(f"malformed header line {field!r}")
            name, value = field.split(":", 1)
            name = name.lower()
            if name in headers:
                raise Failure(f"header {name} sent twice")
            headers[name] = value.strip()
        wanted_line = f"HTTP/1.1 {expect['status']} {expect['reason']}"
        if status_line != wanted_line:
            raise Failure(f"status line {status_line!r}, wanted {wanted_line!r}")
        wanted_headers = expect.get("headers", {})
        if headers != wanted_headers:
            raise Failure(f"headers {headers}, wanted {wanted_headers}")
        if expect.get("no_body"):
            return
        length = int(headers.get("content-length", "0"))
        body = self.read_exactly(length)
        wanted = expected_body(expect.get("body"))
        if body != wanted:
            raise Failure(f"body {body[:200]!r}, wanted {wanted[:200]!r}")

    def closed(self):
        if self.buffer:
            raise Failure(f"unexpected bytes {self.buffer[:200]!r}")
        data = self.receive()
        if data:
            raise Failure(f"expected the connection closed, got {data[:200]!r}")

    def silent(self, seconds):
        self.socket.settimeout(seconds)
        try:
            data = self.socket.recv(65536)
        except socket.timeout:
            return
        finally:
            self.socket.settimeout(DEFAULT_CASE_SECONDS)
        raise Failure(f"expected nothing for {seconds} s, got {data[:200]!r}" if data else "the server closed")

    def close(self):
        self.socket.close()


def run_exchange(port, case):
    seconds = case.get("seconds", DEFAULT_CASE_SECONDS)
    connection = Connection(port, seconds)
    try:
        for step in case["steps"]:
            if "send" in step:
                connection.socket.sendall(step["send"].encode())
            elif "send_hex" in step:
                connection.socket.sendall(bytes.fromhex(step["send_hex"]))
            elif "send_repeat" in step:
                text, times = step["send_repeat"]
                connection.socket.sendall((text * times).encode())
            elif "sleep" in step:
                time.sleep(step["sleep"])
            elif "shut_write" in step:
                connection.socket.shutdown(socket.SHUT_WR)
            elif "expect" in step:
                if "within" in step:
                    connection.socket.settimeout(step["within"])
                connection.response(step["expect"])
                connection.socket.settimeout(seconds)
            elif "closed" in step:
                connection.closed()
            elif "silent" in step:
                connection.silent(step["silent"])
            else:
                raise ValueError(f"unknown step {step}")
    finally:
        connection.close()


def run_concurrent(port, case):
    stalled = Connection(port, DEFAULT_CASE_SECONDS)
    stalled.socket.sendall(b"GET /hello HTTP/1.1\r\nHo")
    results = []

    def fetch():
        try:
            connection = Connection(port, case["within"] + 2)
            connection.socket.sendall(b"GET /slow HTTP/1.1\r\nHost: test\r\n\r\n")
            connection.response({
                "status": 200, "reason": "OK", "body": "slow",
                "headers": {"content-length": "4", "content-type": "text/plain; charset=utf-8"},
            })
            connection.close()
            results.append(None)
        except (Failure, OSError) as error:
            results.append(str(error))

    started = time.monotonic()
    threads = [threading.Thread(target=fetch) for _ in range(case["clients"])]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    elapsed = time.monotonic() - started
    stalled.close()
    errors = [result for result in results if result]
    if errors:
        raise Failure(f"{len(errors)} of {case['clients']} clients failed: {errors[0]}")
    if elapsed >= case["within"]:
        raise Failure(f"{case['clients']} slow requests took {elapsed:.2f} s, wanted under {case['within']} s")


def grade(slug, slug_home, project_dir, module_path, module, suite, cases):
    project_dir = Path(project_dir)
    source = (project_dir / "server.slug").read_text().replace("{module}", module)
    outcomes = []
    with tempfile.TemporaryDirectory(prefix="slug-eval-http-") as directory:
        directory = Path(directory)
        shutil.copyfile(module_path, directory / f"{module}.slug")
        servers = {
            name: Server(slug, slug_home, directory, name, options)
            for name, options in suite["servers"].items()
        }
        try:
            for case in cases:
                server = servers[case.get("server", "default")]
                try:
                    if not server.alive():
                        server.stop()
                        server.start(source)
                    if case.get("kind") == "concurrent":
                        run_concurrent(server.port, case)
                    else:
                        run_exchange(server.port, case)
                    outcomes.append({"id": case["id"], "kind": "http", "passed": True, "detail": ""})
                except (Failure, OSError) as error:
                    outcomes.append({"id": case["id"], "kind": "http", "passed": False, "detail": str(error)[:300]})
        finally:
            for server in servers.values():
                server.stop()
    return outcomes
