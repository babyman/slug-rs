#!/usr/bin/env python3
"""Runs each source benchmark with native code off and with every chunk
compiled (`SLUG_NATIVE=all`), and fails if any output, diagnostic, or exit
status differs."""

import argparse
import concurrent.futures
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCHMARKS = ROOT / "crates/slug-frontend/benches/source/slug"


def run(binary: str, source: pathlib.Path, mode: str) -> tuple[int, bytes, bytes]:
    env = dict(os.environ, SLUG_HOME=str(ROOT), SLUG_NATIVE=mode)
    env.pop("SLUG_NATIVE_TRACE", None)
    env.pop("SLUG_NATIVE_DUMP", None)
    result = subprocess.run(
        [binary, str(source)],
        cwd=BENCHMARKS,
        env=env,
        capture_output=True,
        timeout=600,
        check=False,
    )
    return result.returncode, result.stdout, result.stderr


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", help="a slug built with the native feature")
    args = parser.parse_args()
    binary = str(pathlib.Path(args.binary).resolve())
    sources = sorted(BENCHMARKS.glob("*.slug"))

    def same(source: pathlib.Path) -> bool:
        return run(binary, source, "off") == run(binary, source, "all")

    with concurrent.futures.ThreadPoolExecutor(max_workers=os.cpu_count()) as pool:
        results = list(pool.map(same, sources))
    differ = [source.name for source, alike in zip(sources, results) if not alike]
    if differ:
        print("differ with native code:", ", ".join(differ), file=sys.stderr)
        return 1
    print(f"{len(sources)} benchmarks: the same with native code")
    return 0


if __name__ == "__main__":
    sys.exit(main())
