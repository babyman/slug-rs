#!/usr/bin/env python3
"""Build a profile-guided release `slug` executable.

    python3 -I scripts/pgo-build.py [--source DIR] [--target-dir DIR] [--out PATH]

Builds an instrumented `slug`, runs it once over the tuned source benchmark
workloads, merges the profiles with the toolchain's `llvm-profdata`, and
rebuilds with them. Hold-out workloads are never used for training, so they
still show whether a profile generalizes. The workloads always come from this
checkout, so builds of different revisions train on the same programs.

The build passes `--target` explicitly so that Cargo applies the
instrumentation flags to `slug` and its dependencies but not to build scripts.
It needs the `llvm-tools` rustup component; `--digest` prints a hash of the
training set for caching builds.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCES = os.path.join(ROOT, "crates/slug-frontend/benches/source")
# `http-server` imports the staged slug.net.tcp clutch from this checkout.
ENVIRONMENT = {**os.environ, "SLUG_HOME": os.environ.get("SLUG_HOME", ROOT)}


def training_workloads():
    """The tuned workloads in source.rs order, as paths to their programs."""
    with open(os.path.join(ROOT, "crates/slug-frontend/benches/source.rs")) as file:
        text = file.read()
    found = re.findall(r'name: "([^"]+)",\s*held_out: (true|false)', text)
    return [
        os.path.join(SOURCES, "slug", f"{name}.slug")
        for name, held_out in found
        if held_out == "false"
    ]


def training_digest():
    """A hash of every training program and the modules they import."""
    digest = hashlib.sha256()
    for directory, _, files in sorted(os.walk(os.path.join(SOURCES, "slug"))):
        for name in sorted(files):
            path = os.path.join(directory, name)
            digest.update(os.path.relpath(path, SOURCES).encode())
            with open(path, "rb") as file:
                digest.update(file.read())
    for path in training_workloads():
        digest.update(os.path.basename(path).encode())
    return digest.hexdigest()[:12]


def rustc(source, *args):
    # The source checkout's rust-toolchain.toml selects the compiler.
    return subprocess.run(
        ["rustc", *args], cwd=source, check=True, capture_output=True, text=True
    ).stdout


def build(source, target_dir, host, rustflags):
    subprocess.run(
        [
            "cargo", "build", "--release", "-p", "slug", "--bin", "slug",
            "--target", host,
        ],
        cwd=source,
        env={**os.environ, "CARGO_TARGET_DIR": target_dir, "RUSTFLAGS": rustflags},
        check=True,
    )
    return os.path.join(target_dir, host, "release", "slug")


def install(source, binary):
    """Copy a build to a fresh inode: macOS kills a signed executable that was
    overwritten in place."""
    os.makedirs(os.path.dirname(binary), exist_ok=True)
    staging = f"{binary}.tmp"
    shutil.copy2(source, staging)
    os.replace(staging, binary)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--source", default=ROOT, help="checkout to build")
    parser.add_argument("--target-dir", default=os.path.join(ROOT, "target/pgo"))
    parser.add_argument("--out", default=os.path.join(ROOT, "target/pgo/slug"))
    parser.add_argument("--digest", action="store_true", help="print the training-set hash")
    options = parser.parse_args()
    if options.digest:
        print(training_digest())
        return

    source = os.path.abspath(options.source)
    target_dir = os.path.abspath(options.target_dir)
    host = re.search(r"^host: (\S+)$", rustc(source, "-vV"), re.MULTILINE).group(1)
    sysroot = rustc(source, "--print", "sysroot").strip()
    profdata = os.path.join(sysroot, "lib/rustlib", host, "bin/llvm-profdata")
    if not os.path.exists(profdata):
        sys.exit(f"{profdata} is missing; run `rustup component add llvm-tools`")

    raw = os.path.join(target_dir, "profiles/raw")
    merged = os.path.join(target_dir, "profiles/slug.profdata")
    shutil.rmtree(raw, ignore_errors=True)
    os.makedirs(raw)

    print("building instrumented slug", file=sys.stderr)
    instrumented = build(
        source, os.path.join(target_dir, "instrumented"), host, f"-Cprofile-generate={raw}"
    )
    for program in training_workloads():
        name = os.path.splitext(os.path.basename(program))[0]
        print(f"training on {name}", file=sys.stderr)
        subprocess.run(
            [instrumented, program],
            stdout=subprocess.DEVNULL,
            env={**ENVIRONMENT, "LLVM_PROFILE_FILE": os.path.join(raw, f"{name}-%p.profraw")},
            check=True,
        )
    profiles = [os.path.join(raw, name) for name in sorted(os.listdir(raw))]
    subprocess.run([profdata, "merge", "-o", merged, *profiles], check=True)

    print("building profile-guided slug", file=sys.stderr)
    optimized = build(
        source, os.path.join(target_dir, "optimized"), host, f"-Cprofile-use={merged}"
    )
    install(optimized, os.path.abspath(options.out))
    print(os.path.abspath(options.out))


if __name__ == "__main__":
    main()
