#!/usr/bin/env python3
"""Runs the fork fixtures under ThreadSanitizer and fails on any report.

Builds `slug` with `-Zsanitizer=thread` on the nightly toolchain (std
rebuilt with it, so its own synchronization is seen), then runs every
`tests/conformance/fork/*.slug` several times. A fixture's expected outcome
is the conformance suite's to check; this only looks for data races, which
a run may or may not hit, hence the repeats.

    python3 -I scripts/tsan-forks.py [--runs N] [--skip-build]

Needs `rustup toolchain install nightly --component rust-src`; set
SLUG_TSAN_TOOLCHAIN to use another nightly, as CI does to pin one.
"""

import argparse
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET_DIR = ROOT / "target" / "tsan"
FIXTURES = ROOT / "tests" / "conformance" / "fork"
TOOLCHAIN = "+" + os.environ.get("SLUG_TSAN_TOOLCHAIN", "nightly")


def host_triple():
    output = subprocess.run(
        ["rustc", TOOLCHAIN, "-vV"], check=True, capture_output=True, text=True
    ).stdout
    return next(line.split()[1] for line in output.splitlines() if line.startswith("host:"))


def build(triple):
    environment = {
        **os.environ,
        # slug_sanitized keeps the system allocator, which TSan intercepts.
        "RUSTFLAGS": "-Zsanitizer=thread --cfg slug_sanitized",
        "CARGO_INCREMENTAL": "0",
    }
    subprocess.run(
        [
            "cargo", TOOLCHAIN, "build", "-Zbuild-std", "--target", triple,
            "-p", "slug", "--bin", "slug", "--target-dir", str(TARGET_DIR),
        ],
        check=True,
        cwd=ROOT,
        env=environment,
    )
    return TARGET_DIR / triple / "debug" / "slug"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--skip-build", action="store_true")
    options = parser.parse_args()

    triple = host_triple()
    binary = TARGET_DIR / triple / "debug" / "slug" if options.skip_build else build(triple)
    environment = {
        **os.environ,
        "SLUG_HOME": str(ROOT),
        # Reports go to stderr; the exit code alone would also flag a
        # fixture that is meant to fail.
        "TSAN_OPTIONS": "halt_on_error=1",
    }
    fixtures = sorted(FIXTURES.glob("*.slug"))
    races = []
    for fixture in fixtures:
        for run in range(options.runs):
            try:
                result = subprocess.run(
                    [str(binary), str(fixture)],
                    cwd=FIXTURES,
                    env=environment,
                    capture_output=True,
                    text=True,
                    timeout=120,
                )
            except subprocess.TimeoutExpired:
                races.append((fixture.name, "timed out"))
                break
            if "ThreadSanitizer" in result.stderr:
                report = result.stderr[result.stderr.index("WARNING: ThreadSanitizer"):]
                races.append((fixture.name, "\n".join(report.splitlines()[:40])))
                break
    print(f"{len(fixtures)} fork fixtures, {options.runs} runs each")
    for name, report in races:
        print(f"\n== {name}\n{report}")
    sys.exit(1 if races else 0)


if __name__ == "__main__":
    main()
