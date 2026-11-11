#!/usr/bin/env python3
"""Runs the fork fixtures under ThreadSanitizer and fails on any report.

Builds `slug` with `-Zsanitizer=thread` on the nightly toolchain (std
rebuilt with it, so its own synchronization is seen), then runs every
`tests/conformance/fork/*.slug` several times through the conformance
runner, `slug-fixtures`, with each fixture's time limit scaled for the slow
instrumented build. A run fails on a data race, which TSan reports on stderr
and in its exit status, and on any fixture that misses its expected outcome
or output, so a fixture that ends early cannot pass without reaching what it
tests. TSan writes its reports to files rather than stderr, and any report
fails the run on its own: a fixture expecting an error cannot absorb TSan's
exit status or a report that follows its expected diagnostic. Races may or
may not occur in a run, hence the repeats.

    python3 -I scripts/tsan-forks.py [--runs N] [--skip-build]

Needs `rustup toolchain install nightly --component rust-src`; set
SLUG_TSAN_TOOLCHAIN to use another nightly, as CI does to pin one.
"""

import argparse
import os
import pathlib
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET_DIR = ROOT / "target" / "tsan"
# Where TSan writes each process's reports, as `report.<pid>`.
REPORTS = TARGET_DIR / "reports"
FIXTURES = ROOT / "tests" / "conformance" / "fork"
TOOLCHAIN = "+" + os.environ.get("SLUG_TSAN_TOOLCHAIN", "nightly")
# How much longer than its own limit a fixture may take under TSan.
TIMEOUT_SCALE = 10


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


def build_runner():
    """The conformance runner, an ordinary build: only `slug` is instrumented."""
    subprocess.run(
        ["cargo", "build", "-p", "slug", "--bin", "slug-fixtures"],
        check=True,
        cwd=ROOT,
        env={**os.environ, "CARGO_INCREMENTAL": "0"},
    )
    return ROOT / "target" / "debug" / "slug-fixtures"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--skip-build", action="store_true")
    options = parser.parse_args()

    triple = host_triple()
    binary = TARGET_DIR / triple / "debug" / "slug" if options.skip_build else build(triple)
    # The runner starts each fixture with a clean environment, so a wrapper
    # gives TSan its options and a PATH to find its symbolizer; without one
    # it warns on stderr, which a fixture's expected stderr would not match.
    wrapper = TARGET_DIR / "slug-tsan"
    REPORTS.mkdir(parents=True, exist_ok=True)
    options_text = f"halt_on_error=1 log_path={REPORTS / 'report'}"
    wrapper.write_text(
        "#!/bin/sh\n"
        f"PATH={shlex.quote(os.environ.get('PATH', '/usr/bin:/bin'))}\n"
        f"SLUG_HOME={shlex.quote(str(ROOT))}\n"
        f"TSAN_OPTIONS={shlex.quote(options_text)}\n"
        "export PATH SLUG_HOME TSAN_OPTIONS\n"
        f'exec {shlex.quote(str(binary))} "$@"\n'
    )
    wrapper.chmod(0o755)
    runner = build_runner()
    failures = []
    for run in range(options.runs):
        for stale in REPORTS.glob("report.*"):
            stale.unlink()
        result = subprocess.run(
            [str(runner), str(FIXTURES), "--slug", str(wrapper), "--timeout-scale", str(TIMEOUT_SCALE)],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        reports = sorted(REPORTS.glob("report.*"))
        if reports:
            failures.append(
                (run + 1, "\n".join(report.read_text(errors="replace") for report in reports))
            )
        if result.returncode != 0:
            failures.append((run + 1, result.stderr))
        if failures:
            break
    print(f"{len(sorted(FIXTURES.glob('*.slug')))} fork fixtures, {options.runs} runs each")
    for run, report in failures:
        # A race report's two stacks run to about 80 lines each.
        print(f"\n== run {run}\n" + "\n".join(report.splitlines()[:400]))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
