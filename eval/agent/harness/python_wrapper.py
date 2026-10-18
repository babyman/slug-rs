#!/usr/bin/env python3
"""The `python3` command an agent sees during a Python-arm run.

It forwards each invocation to the real interpreter with the terminal's
standard streams untouched, and appends one JSON line per invocation to the
run's log, the same log `slug_wrapper.py` writes for Slug runs. Configuration
comes from environment variables set by the harness:

- SLUG_EVAL_PYTHON: the real python3.
- SLUG_EVAL_LOG: the JSON-lines invocation log.

Agents also use Python to edit files, so each line records how the program was
given: a script, `-c`, `-m`, or standard input.
"""

import json
import os
import subprocess
import sys
import time


def program_kind(arguments):
    for index, argument in enumerate(arguments):
        if argument in ("-c", "-m"):
            return {"-c": "inline", "-m": "module"}[argument], arguments[index + 1 : index + 2]
        if argument == "-":
            return "stdin", []
        if not argument.startswith("-"):
            return "script", [argument]
    return "stdin", []


def main():
    arguments = sys.argv[1:]
    started = time.monotonic()
    completed = subprocess.run([os.environ["SLUG_EVAL_PYTHON"], *arguments])
    kind, target = program_kind(arguments)
    entry = {
        "argv": arguments,
        "command": "python3",
        "program": kind,
        "target": target,
        "cwd": os.getcwd(),
        "exit": completed.returncode,
        "seconds": round(time.monotonic() - started, 3),
        "codes": [],
    }
    with open(os.environ["SLUG_EVAL_LOG"], "a") as log:
        log.write(json.dumps(entry) + "\n")
    sys.exit(completed.returncode)


if __name__ == "__main__":
    main()
