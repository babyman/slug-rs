#!/usr/bin/env python3
"""The `slug` command an agent sees during an evaluation run.

It applies the run's arm to each invocation, forwards it to the real binary,
and appends one JSON line per invocation to the run's log. Configuration comes
from environment variables set by the harness:

- SLUG_EVAL_REAL: the real slug binary.
- SLUG_EVAL_LOG: the JSON-lines invocation log.
- SLUG_EVAL_DIAGNOSTICS: `text` strips `--diagnostic-format=json`; `json`
  forces it.
- SLUG_EVAL_FIXES: `off` empties the `fixes` of JSON diagnostics.
- SLUG_EVAL_REPORT: `off` rejects `slug report` as an unknown command would be.
"""

import json
import os
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
                document["fixes"] = []
                lines.append(json.dumps(document, separators=(",", ":")) + "\n")
                continue
        lines.append(line)
    return "".join(lines)


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
    # Standard input passes straight through: reading it here first would
    # block every invocation whose input never ends, such as a terminal.
    completed = subprocess.run([real] + arguments, capture_output=True)
    stderr = completed.stderr.decode("utf-8", "replace")
    if diagnostics == "json" and fixes == "off":
        stderr = strip_fixes(stderr)
    sys.stdout.buffer.write(completed.stdout)
    sys.stderr.write(stderr)

    codes = codes_from(stderr)
    if completed.returncode != 0 and diagnostics != "json" and command != "report":
        codes = shadow_codes(real, rest)
    record.update(
        {
            "exit": completed.returncode,
            "seconds": round(time.monotonic() - started, 3),
            "codes": codes,
            "stderr_head": stderr[:400],
        }
    )
    append(record)
    return completed.returncode


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
