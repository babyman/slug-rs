#!/usr/bin/env python3
"""Interleaved A/B timing of two Slug builds over the source benchmark corpus.

Sequential benchmark runs drift by up to 30% within one session on a laptop,
so this runner alternates the binaries inside every round: host drift then
affects both sides equally instead of masquerading as a speedup.

    python3 -I scripts/bench-ab.py [BASE] [CANDIDATE] [--rounds N] [--only a,b]

BASE and CANDIDATE are Git revisions. CANDIDATE defaults to the working tree,
including uncommitted changes. Each revision is built once in a detached
worktree and cached under target/bench-ab/; the working tree is rebuilt every
run. Pass --bin LABEL=PATH (twice) to compare prebuilt executables instead.

Timings are host-specific evidence for one decision, never a CI threshold.
"""

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCES = os.path.join(ROOT, "crates/slug-frontend/benches/source")
CACHE = os.path.join(ROOT, "target/bench-ab")


def git(*args):
    return subprocess.run(
        ["git", "-C", ROOT, *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def install(source, binary):
    """Copy a build to a fresh inode: macOS kills a signed executable that was
    overwritten in place."""
    os.makedirs(os.path.dirname(binary), exist_ok=True)
    staging = f"{binary}.tmp"
    shutil.copy2(source, staging)
    os.replace(staging, binary)


def build_revision(revision):
    sha = git("rev-parse", "--verify", f"{revision}^{{commit}}")
    binary = os.path.join(CACHE, "bin", f"slug-{sha[:12]}")
    if os.path.exists(binary):
        return f"{revision} ({sha[:9]})", binary
    worktree = tempfile.mkdtemp(prefix="slug-bench-ab-")
    try:
        git("worktree", "add", "--detach", worktree, sha)
        print(f"building {revision} ({sha[:9]})", file=sys.stderr)
        subprocess.run(
            ["cargo", "build", "--release", "-p", "slug", "--bin", "slug"],
            cwd=worktree,
            env={**os.environ, "CARGO_TARGET_DIR": os.path.join(CACHE, "build")},
            check=True,
        )
        install(os.path.join(CACHE, "build/release/slug"), binary)
    finally:
        git("worktree", "remove", "--force", worktree)
    return f"{revision} ({sha[:9]})", binary


def build_working_tree():
    print("building working tree", file=sys.stderr)
    subprocess.run(
        ["cargo", "build", "--release", "-p", "slug", "--bin", "slug"],
        cwd=ROOT,
        check=True,
    )
    binary = os.path.join(CACHE, "bin", "slug-worktree")
    install(os.path.join(ROOT, "target/release/slug"), binary)
    dirty = git("status", "--porcelain", "--untracked-files=no") != ""
    head = git("rev-parse", "--short=9", "HEAD")
    return f"worktree ({head}{', dirty' if dirty else ''})", binary


def workloads(only):
    """Return (name, held_out) for each workload in source.rs order."""
    with open(os.path.join(ROOT, "crates/slug-frontend/benches/source.rs")) as file:
        text = file.read()
    found = re.findall(r'name: "([^"]+)",\s*held_out: (true|false)', text)
    selected = [(name, held == "true") for name, held in found]
    if only:
        wanted = set(only.split(","))
        unknown = wanted - {name for name, _ in selected}
        if unknown:
            sys.exit(f"unknown workloads: {', '.join(sorted(unknown))}")
        selected = [entry for entry in selected if entry[0] in wanted]
    return selected


def run_once(binary, source):
    """Run one fresh process; return (wall-clock seconds, stdout)."""
    started = time.perf_counter()
    process = subprocess.Popen(
        [binary, source], stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    stdout, stderr = process.communicate()
    elapsed = time.perf_counter() - started
    if process.returncode != 0:
        sys.exit(
            f"{binary} {source} exited with {process.returncode}:\n"
            f"{stderr.decode(errors='replace')}"
        )
    return elapsed, stdout


def peak_rss(binary, source):
    """Run one fresh process and return its peak resident set size in bytes."""
    pid = os.fork()
    if pid == 0:
        devnull = os.open(os.devnull, os.O_WRONLY)
        os.dup2(devnull, 1)
        os.dup2(devnull, 2)
        try:
            os.execv(binary, [binary, source])
        finally:
            os._exit(127)
    _, status, usage = os.wait4(pid, 0)
    if os.waitstatus_to_exitcode(status) != 0:
        sys.exit(f"{binary} {source} failed while measuring memory")
    # ru_maxrss is bytes on macOS and kilobytes on Linux.
    return usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss * 1024


def spread(samples):
    """Interquartile range relative to the median; robust to one outlier."""
    lower, _, upper = statistics.quantiles(samples, n=4)
    return (upper - lower) / statistics.median(samples)


def geomean(values):
    return statistics.geometric_mean(values) if values else None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("base", nargs="?", default="master")
    parser.add_argument("candidate", nargs="?", default=None)
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--only", help="comma-separated workload names")
    parser.add_argument("--bin", action="append", metavar="LABEL=PATH")
    parser.add_argument("--json", action="store_true")
    options = parser.parse_args()
    if options.rounds < 2:
        parser.error("--rounds must be at least 2")

    if options.bin:
        if len(options.bin) != 2:
            sys.exit("--bin must be given exactly twice")
        sides = [tuple(entry.split("=", 1)) for entry in options.bin]
    else:
        candidate = (
            build_revision(options.candidate)
            if options.candidate
            else build_working_tree()
        )
        sides = [build_revision(options.base), candidate]
    selected = workloads(options.only)

    sources = {
        name: os.path.join(SOURCES, "slug", f"{name}.slug") for name, _ in selected
    }
    for name, source in sources.items():
        outputs = {label: run_once(binary, source)[1] for label, binary in sides}
        if len(set(outputs.values())) != 1:
            sys.exit(f"{name}: builds disagree on output: {outputs}")

    times = {(label, name): [] for label, _ in sides for name, _ in selected}
    for round_index in range(options.rounds):
        print(f"round {round_index + 1}/{options.rounds}", file=sys.stderr)
        # Alternate which side goes first so neither always runs on a
        # cooler or warmer machine.
        order = sides if round_index % 2 == 0 else sides[::-1]
        for name, _ in selected:
            for label, binary in order:
                times[(label, name)].append(run_once(binary, sources[name])[0])

    (base_label, base_binary), (candidate_label, candidate_binary) = sides
    rows = []
    for name, held_out in selected:
        base = times[(base_label, name)]
        candidate = times[(candidate_label, name)]
        rows.append(
            {
                "name": name,
                "held_out": held_out,
                "base_median_s": statistics.median(base),
                "candidate_median_s": statistics.median(candidate),
                "ratio": statistics.median(candidate) / statistics.median(base),
                "base_spread": spread(base),
                "candidate_spread": spread(candidate),
                "base_peak_rss": peak_rss(base_binary, sources[name]),
                "candidate_peak_rss": peak_rss(candidate_binary, sources[name]),
            }
        )
    tuned = [row["ratio"] for row in rows if not row["held_out"]]
    held = [row["ratio"] for row in rows if row["held_out"]]
    report = {
        "schema_version": 1,
        "rounds": options.rounds,
        "base": {"label": base_label, "binary": base_binary},
        "candidate": {"label": candidate_label, "binary": candidate_binary},
        "workloads": rows,
        "geomean_ratio": {"tuned": geomean(tuned), "held_out": geomean(held)},
    }

    if options.json:
        print(json.dumps(report, indent=2))
        return
    print(f"base:      {base_label}")
    print(f"candidate: {candidate_label}")
    print(f"{options.rounds} interleaved rounds; ratio = candidate / base, below 1.00x is faster")
    print("spread = interquartile range / median; treat ratios inside it as noise\n")
    print(
        f"{'workload':<22}{'base ms':>10}{'cand ms':>10}{'ratio':>8}"
        f"{'spread':>14}{'base RSS':>11}{'cand RSS':>11}"
    )
    for row in rows:
        spreads = f"{row['base_spread']:.0%}/{row['candidate_spread']:.0%}"
        print(
            f"{row['name']:<22}"
            f"{row['base_median_s'] * 1000:>10.1f}"
            f"{row['candidate_median_s'] * 1000:>10.1f}"
            f"{row['ratio']:>7.2f}x"
            f"{spreads:>14}"
            f"{row['base_peak_rss'] / 1048576:>9.1f}MB"
            f"{row['candidate_peak_rss'] / 1048576:>9.1f}MB"
            f"{'  (hold-out)' if row['held_out'] else ''}"
        )
    summary = ", ".join(
        f"{'hold-out' if group == 'held_out' else group} {ratio:.2f}x"
        for group, ratio in report["geomean_ratio"].items()
        if ratio is not None
    )
    print(f"\ngeomean ratio: {summary}")


if __name__ == "__main__":
    main()
