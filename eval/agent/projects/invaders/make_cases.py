#!/usr/bin/env python3
"""Generates cases.json for the invaders project.

Each case is an input script for slug.arcade's headless mode. Its expected
frames come from running the reference, reference/invaders.slug, and are
stored as window digests (see harness/arcade_grader.py). Bot cases come from
reference/bot.slug, which plays the reference game; the bot's `#` lines name
what happened when, and are kept in the scripts for triage.

Usage: make_cases.py [slug binary]   (run from anywhere; needs the staged
slug.arcade clutch)
"""

import json
import os
import random
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
sys.path.insert(0, str(REPO / "eval" / "agent" / "harness"))
import arcade_grader  # noqa: E402

WINDOW = 30
SLUG = sys.argv[1] if len(sys.argv) > 1 else str(REPO / "target" / "release" / "slug")


def script(events, end):
    """A script from (frame, keys) pairs, keys a string or "-"."""
    lines = [f"{frame} {keys}" for frame, keys in sorted(events, key=lambda e: e[0])]
    return "\n".join(lines + [f"end {end}"]) + "\n"


def start(at=10):
    return [(0, "-"), (at, "enter"), (at + 4, "-")]


def taps(first, every, count, keys="space", moving="-"):
    events = []
    for i in range(count):
        events += [(first + i * every, keys if moving == "-" else f"{moving} {keys}"), (first + i * every + 3, moving)]
    return events


def random_play(seed, frames, fire_rate=0.6):
    rng = random.Random(seed)
    events = start()
    frame = 210
    while frame < frames:
        move = rng.choice(["left", "right", "-", "right", "left"])
        fire = rng.random() < fire_rate
        keys = " ".join([k for k in [move] if k != "-"] + (["space"] if fire else [])) or "-"
        events.append((frame, keys))
        if fire:
            events.append((frame + 3, move))
        frame += rng.randint(4, 40)
    return script(events, frames)


def bot(frames, skill, seed):
    completed = subprocess.run(
        [SLUG, "bot.slug", str(frames), str(skill), str(seed)],
        cwd=HERE / "reference",
        capture_output=True,
        text=True,
        check=True,
        env={**os.environ, "SLUG_HOME": str(REPO)},
    )
    return completed.stdout


def targeted():
    """Hand-made scripts for specific behaviour."""
    cases = []

    def add(name, description, events, end):
        cases.append((name, description, script(events, end)))

    add("boot", "the start screen after power-on", [(0, "-")], 120)
    add("start-prompt", "PLAY PLAYER<1> with the blinking score, then the rack appears", start(30), 520)
    add("start-held", "holding the start key starts one game", [(0, "-"), (20, "enter"), (300, "-")], 600)
    add("idle-game", "no input: the rack marches and the player is shot three times, then game over and the start screen", start(), 3600)
    add("idle-restart", "no input until game over, then a second game", start() + [(3000, "enter"), (3004, "-")], 3900)
    add("move-right", "the player moves right to its limit", start() + [(330, "right"), (600, "-")], 700)
    add("move-left-right", "the player moves left to its limit and back", start() + [(330, "left"), (420, "right"), (540, "left right"), (600, "-")], 700)
    add("fire-once", "one shot, which explodes at the top", start() + [(330, "space"), (335, "-")], 450)
    add("fire-held", "holding fire fires once", start() + [(330, "space"), (700, "-")], 720)
    add("fire-taps", "repeated shots from one place", start() + taps(330, 25, 20), 900)
    add("fire-left-column", "shots up the left column, into the first shield", start() + [(330, "right"), (348, "-")] + taps(360, 30, 25), 1150)
    add("fire-right-column", "shots up the right column", start() + [(330, "right"), (480, "-")] + taps(490, 30, 25), 1300)
    add("fire-moving", "firing while moving across", start() + taps(330, 12, 40, moving="right") + taps(820, 12, 30, moving="left"), 1300)
    add("shield-erosion", "the player's shots eat through a shield", start() + [(330, "right"), (370, "-")] + taps(380, 20, 40), 1300)
    return cases


# Bot runs to search, and the events a selected run must show.
BOT_PLAN = [
    ("bot-first-kills", 2500, 90, 11, None),
    ("bot-saucer", 6000, 90, 2, "saucer hit"),
    ("bot-rack-clear", 9000, 90, 3, "rack 1"),
    ("bot-wait-hit", 9000, 90, 1, "rackEndWaitHit"),
    ("bot-extra-ship", 12000, 90, 1, "score 15"),
    ("bot-one-alien", 9000, 90, 7, "aliens 1"),
    ("bot-game-over", 9000, 70, 4, "gameOverPrint"),
    ("bot-invaded", 9000, 85, 5, "state invaded"),
    ("bot-long-1", 20000, 90, 3, None),
    ("bot-long-2", 20000, 95, 8, None),
    ("bot-long-3", 20000, 80, 12, None),
]


def bot_cases():
    cases = []
    for name, frames, skill, seed, wanted in BOT_PLAN:
        text = bot(frames, skill, seed)
        if wanted is not None and wanted not in text:
            raise SystemExit(f"{name}: bot run (skill {skill}, seed {seed}) never shows `{wanted}`")
        events = [line.split("#", 1)[1].strip() for line in text.splitlines() if line.startswith("#")]
        highlights = [e for e in events if any(w in e for w in ("saucer hit", "rack", "aliens 1", "invaded", "gameOver"))]
        description = f"the test bot (skill {skill}, seed {seed}) plays {frames} frames"
        if highlights:
            description += ": " + "; ".join(highlights[:8])
        cases.append((name, description, text))
    return cases


def main():
    suite = {
        "kind": "arcade",
        "window": WINDOW,
        "cases": [],
    }
    plans = targeted()
    plans += [(f"random-{seed}", f"random play, seed {seed}", random_play(seed, 4000 + 500 * (seed % 5))) for seed in range(1, 13)]
    plans += bot_cases()
    for name, description, text in plans:
        frames = int(text.strip().splitlines()[-1].split()[1])
        seconds = 20 + frames // 150
        hashes, sounds, error, _ = arcade_grader.run(SLUG, REPO, HERE, HERE / "reference" / "invaders.slug", text, seconds)
        if error is not None:
            raise SystemExit(f"{name}: reference failed: {error}")
        if len(hashes) // 8 != frames:
            raise SystemExit(f"{name}: reference showed {len(hashes) // 8} frames, not {frames}")
        suite["cases"].append(
            {
                "id": name,
                "description": description,
                "frames": frames,
                "seconds": seconds,
                "script": text,
                "windows": arcade_grader.digests(hashes, WINDOW),
                "sounds": sounds,
            }
        )
        print(f"{name}: {frames} frames", file=sys.stderr)
    (HERE / "cases.json").write_text(json.dumps(suite, indent=1) + "\n")
    total = sum(case["frames"] for case in suite["cases"])
    print(f"{len(suite['cases'])} cases, {total} frames", file=sys.stderr)


main()
