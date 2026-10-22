#!/usr/bin/env python3
"""The `slug` command an agent sees during an evaluation run.

It applies the run's arm to each invocation, forwards it to the real binary,
and appends one JSON line per invocation to the run's log. Configuration comes
from environment variables set by the harness:

- SLUG_EVAL_REAL: the real slug binary.
- SLUG_EVAL_LOG: the JSON-lines invocation log.
- SLUG_EVAL_DIAGNOSTICS: `text` strips `--diagnostic-format=json`; `json`
  forces it.
- SLUG_EVAL_FIXES: `off` removes every machine-applicable edit: it empties the
  `fixes` of diagnostics and the `edits` of their `suggestions`, keeping each
  suggestion's message, in JSON and in text alike.
- SLUG_EVAL_REPORT: `off` rejects `slug report` as an unknown command would be.
"""

import json
import os
import signal
import subprocess
import sys
import time

JSON_FLAG = "--diagnostic-format=json"
SHADOW_TIMEOUT_SECONDS = 20


def runner_arguments(arguments):
    """Splits leading runner flags from the command and program arguments."""
    flags = []
    rest = list(arguments)
    while rest and rest[0] == JSON_FLAG:
        flags.append(rest.pop(0))
    return flags, rest


def codes_from(stderr):
    codes = []
    for line in stderr.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            document = json.loads(line)
        except ValueError:
            continue
        while isinstance(document, dict):
            if "code" in document:
                codes.append(document["code"])
            document = document.get("cause")
    return codes


def strip_fixes(stderr):
    lines = []
    for line in stderr.splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("{"):
            try:
                document = json.loads(stripped)
            except ValueError:
                lines.append(line)
                continue
            if isinstance(document, dict) and "fixes" in document:
                # An import's compile error carries its fixes in `cause`.
                nested = document
                while isinstance(nested, dict):
                    if "fixes" in nested:
                        nested["fixes"] = []
                    for suggestion in nested.get("suggestions") or []:
                        suggestion["edits"] = []
                    nested = nested.get("cause")
                lines.append(json.dumps(document, separators=(",", ":")) + "\n")
                continue
        lines.append(line)
    return "".join(lines)


def strip_text_fixes(stderr):
    """Removes fixes and suggested edits from text diagnostics, which render
    each fix as a `  = fix:` line and every edit as an indented `at` line."""
    return "".join(
        line
        for line in stderr.splitlines(keepends=True)
        if not line.startswith(("  = fix: ", "      at "))
    )


def main():
    real = os.environ["SLUG_EVAL_REAL"]
    diagnostics = os.environ.get("SLUG_EVAL_DIAGNOSTICS", "text")
    fixes = os.environ.get("SLUG_EVAL_FIXES", "on")
    report = os.environ.get("SLUG_EVAL_REPORT", "on")

    _, rest = runner_arguments(sys.argv[1:])
    command = rest[0] if rest else None
    started = time.monotonic()
    record = {"argv": sys.argv[1:], "command": command, "cwd": os.getcwd()}

    if command == "report" and report == "off":
        message = "slug: cannot read report: No such file or directory (os error 2)\n"
        sys.stderr.write(message)
        record.update({"exit": 1, "blocked": "report"})
        append(record)
        return 1

    arguments = ([JSON_FLAG] if diagnostics == "json" else []) + rest
    returncode, stderr = run(real, arguments, fixes, diagnostics)

    codes = codes_from(stderr)
    if returncode != 0 and diagnostics != "json" and command != "report":
        codes = shadow_codes(real, rest)
    record.update(
        {
            "exit": returncode,
            "seconds": round(time.monotonic() - started, 3),
            "codes": codes,
            "stderr_head": stderr[:400],
        }
    )
    append(record)
    return returncode


def run(real, arguments, fixes, diagnostics):
    """Runs the real binary and returns its exit status and filtered standard
    error. Standard input and output pass straight through, so a server's
    output appears as it prints, not when it exits; standard error is
    filtered and passed on a line at a time. A signal that stops the wrapper
    stops the program too."""
    process = subprocess.Popen([real] + arguments, stderr=subprocess.PIPE)

    def forward(number, _frame):
        process.send_signal(number)

    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(number, forward)
    lines = []
    for raw in process.stderr:
        line = raw.decode("utf-8", "replace")
        if fixes == "off":
            line = strip_fixes(line) if diagnostics == "json" else strip_text_fixes(line)
        sys.stderr.write(line)
        sys.stderr.flush()
        lines.append(line)
    return process.wait(), "".join(lines)


def shadow_codes(real, rest):
    """Reruns a failed text-mode invocation in JSON mode only to log its codes."""
    try:
        shadow = subprocess.run(
            [real, JSON_FLAG] + rest,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=SHADOW_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        return []
    return codes_from(shadow.stderr.decode("utf-8", "replace"))


def append(record):
    path = os.environ.get("SLUG_EVAL_LOG")
    if not path:
        return
    with open(path, "a", encoding="utf-8") as log:
        log.write(json.dumps(record) + "\n")


if __name__ == "__main__":
    sys.exit(main())
