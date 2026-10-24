#!/usr/bin/env python3
"""Writes python/cases.json, the Python arm's cases, from ../cases.json.

Each Slug case becomes a Python case with the same id: the same start (a
seed or a snapshot) and the same keys. Its screens are the 24 rows after the
start and after every key, from the Python reference (python/reference),
stored as the first 16 hex digits of the SHA-256 of the rows joined with
newlines. Generation stops unless the reference's final snapshot is the Slug
case's, and its final screen the Slug reference's (`--screen`).

Usage: python/make_cases.py   (the slug binary is $SLUG, or the release build)
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
REPO = PROJECT.parents[3]
SLUG = os.environ.get("SLUG", str(REPO / "target" / "release" / "slug"))
sys.path.insert(0, str(HERE / "reference"))
from game import Game  # noqa: E402


def rows(game):
    return [row.ljust(80) for row in game.text()]


def digest(screen):
    return hashlib.sha256("\n".join(screen).encode()).hexdigest()[:16]


def keys(script):
    """The keys an arcade script presses, as [key, shift] pairs."""
    pressed = []
    for line in script.splitlines():
        words = line.split()
        if words[0].isdigit() and words[1:] != ["-"]:
            pressed.append([words[-1], words[1] == "shift"])
    return pressed


def slug_screen(case, directory):
    """The Slug reference's final screen text for a case."""
    for name, text in case.get("files", {}).items():
        Path(directory, name).write_text(text)
    Path(directory, "script.txt").write_text(case["script"])
    environment = dict(os.environ, SLUG_HOME=str(REPO), SLUG_ARCADE_HEADLESS="script.txt", SLUG_ARCADE_OUT=directory)
    subprocess.run(
        [SLUG, str(PROJECT / "reference" / "crawl.slug"), *case["args"], "--screen", "screen.json"],
        cwd=directory, env=environment, check=True, capture_output=True,
    )
    return [row.ljust(80) for row in json.loads(Path(directory, "screen.json").read_text())]


def convert(case, directory):
    if case["args"][0] == "--seed":
        start = {"seed": int(case["args"][1])}
        game = Game(start["seed"])
    else:
        start = {"load": json.loads(case["files"]["start.json"])}
        game = Game.load(json.loads(case["files"]["start.json"]))
    pressed = keys(case["script"])
    screens = [digest(rows(game))]
    for key, shift in pressed:
        game.command(key, shift)
        screens.append(digest(rows(game)))
    state = json.loads(json.dumps(game.state()))
    if state != case["dump"]:
        sys.exit(f"{case['id']}: the Python reference's final snapshot differs")
    if slug_screen(case, directory) != rows(game):
        sys.exit(f"{case['id']}: the Python reference's final screen differs from the Slug reference's")
    # Twice as long as Slug's limit for the same keys, which counts 2 frames a key.
    seconds = 20 + len(pressed) // 75
    return {"id": case["id"], "description": case["description"], "seconds": seconds,
            "start": start, "keys": pressed, "screens": screens, "snapshot": case["dump"]}


def main():
    suite = json.loads((PROJECT / "cases.json").read_text())
    with tempfile.TemporaryDirectory(prefix="crawl-python-cases-") as directory:
        cases = [convert(case, directory) for case in suite["cases"]]
    out = {"kind": "python-game", "module": "crawl", "cases": cases}
    (HERE / "cases.json").write_text(json.dumps(out, separators=(",", ":")) + "\n")
    print(f"{len(cases)} cases, {sum(len(case['keys']) for case in cases)} keys", file=sys.stderr)


if __name__ == "__main__":
    main()
