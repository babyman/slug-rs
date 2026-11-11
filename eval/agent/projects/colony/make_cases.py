"""Writes this project's cases.json:

    python3 -I eval/agent/projects/colony/make_cases.py --slug target/release/slug

Expected results come from running the workspace's one-core colony.slug,
the module agents start from, so a case checks that their version still
does exactly what it did.
"""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]

PRELUDE = r'''
val colony = import("{module}")
val json = import("slug.fmt.json")
val {map, range, reduce, filter} = import("slug.std")
val {await} = import("slug.channel")
var problems = []
val expect = fn(label, actual, wanted) {
    if (actual != wanted) { problems = problems :+ label }
}
val finish = fn() {
    if (len(problems) == 0) { println("PASS") } else { println("FAIL", problems) }
}
// Compares the colony's snapshot with `wanted`, JSON text, field by field.
val same = fn(label, wanted:str) {
    val want = json.parse(wanted)
    val got = json.parse(json.stringify(colony.snapshot()))
    map(["turn", "store", "stats", "rows", "ants"], fn(k) {
        if (got[k] != want[k]) { problems = problems :+ (label + " " + k) }
    })
}
// What `body` throws for a bad `forks`: its data, or what went wrong.
val forksError = fn(body) {
    defer onerror(err) {
        return if (err.type == "colony.forks") { err.data } else { {wrong: "" + err} }
    }
    body()
    {none: true}
}
'''

# Worlds: name, seed, width, height, ants.
WORLDS = {
    "small": (1, 40, 20, 30),
    "crowded": (2, 24, 12, 60),
    "sparse": (3, 60, 30, 8),
    "wide": (4, 90, 16, 120),
    "medium": (5, 80, 40, 300),
    "one": (6, 30, 15, 1),
    "none": (7, 30, 15, 0),
    "packed": (8, 20, 16, 120),
    "repacked": (9, 20, 16, 120),
}
TICKS = 6
OBSERVED = ["a0", "a1", "a5"]
SPEED_WORLD = (7, 160, 80, 1200)
SPEED_TICKS = 10

GENERATE = r'''
val colony = import("colony")
val json = import("slug.fmt.json")
val {map, range} = import("slug.std")
val main = fn() {
    colony.start(cfg("seed", 0), cfg("width", 0), cfg("height", 0), cfg("count", 0))
    println(json.stringify(colony.snapshot()))
    map(range(cfg("ticks", 0)), fn(_) {
        colony.tick()
        println(json.stringify(colony.snapshot()))
    })
    map(["a0", "a1", "a5"], fn(id) {
        defer onerror(_) { println("null") }
        println(json.stringify(colony.observe(id)))
    })
}
'''


def run_starter(slug, world, ticks):
    seed, width, height, count = world
    with tempfile.TemporaryDirectory() as directory:
        shutil.copyfile(HERE / "workspace" / "colony.slug", Path(directory) / "colony.slug")
        Path(directory, "generate.slug").write_text(GENERATE)
        out = subprocess.run(
            [slug, "generate.slug", f"--seed={seed}", f"--width={width}", f"--height={height}",
             f"--count={count}", f"--ticks={ticks}"],
            cwd=directory, check=True, capture_output=True, text=True,
            env={**os.environ, "SLUG_HOME": str(REPO)},
        ).stdout.splitlines()
    snapshots = out[: ticks + 1]
    observes = out[ticks + 1:]
    return snapshots, observes


def raw(text):
    assert "'" not in text and "\\" not in text
    return "'" + text + "'"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--slug", required=True)
    slug = os.path.abspath(parser.parse_args().slug)
    cases = []

    def case(id, body, seconds=20, kind="program", **extra):
        cases.append({"id": id, "kind": kind, "source": "val main = fn() {\n" + body + "\nfinish()\n}\n",
                      "seconds": seconds, **extra})

    expected = {name: run_starter(slug, world, TICKS) for name, world in WORLDS.items()}

    def started(name):
        seed, width, height, count = WORLDS[name]
        return f"colony.start({seed}, {width}, {height}, {count})\n"

    # Every world, every tick, on one fork and on several.
    for name, (snapshots, _) in expected.items():
        for forks in (1, 2, 3, 4, 7):
            lines = started(name) + f"same(\"start\", {raw(snapshots[0])})\n"
            for t in range(1, TICKS + 1):
                lines += f"colony.tick({forks})\nsame(\"tick {t}\", {raw(snapshots[t])})\n"
            case(f"ticks-{name}-forks-{forks}", lines)

    # `run`, and forks changing from tick to tick.
    for name in ("small", "medium", "wide"):
        snapshots, _ = expected[name]
        case(f"run-{name}-forks-4", started(name) + f"colony.run({TICKS}, 4)\nsame(\"run\", {raw(snapshots[TICKS])})\n")
        case(f"run-{name}-default", started(name) + f"colony.run({TICKS})\nsame(\"run\", {raw(snapshots[TICKS])})\n")
        lines = started(name)
        for t, forks in zip(range(1, TICKS + 1), (4, 1, 3, 8, 2, 1)):
            lines += f"colony.tick({forks})\nsame(\"tick {t} on {forks}\", {raw(snapshots[t])})\n"
        case(f"mixed-forks-{name}", lines)

    # More forks than ants.
    for name in ("one", "none", "sparse"):
        snapshots, _ = expected[name]
        case(f"many-forks-{name}", started(name) + f"colony.run({TICKS}, 16)\nsame(\"run\", {raw(snapshots[TICKS])})\n")

    # What ants see, after ticks on forks.
    for name in ("small", "crowded", "medium"):
        snapshots, observes = expected[name]
        lines = started(name) + f"colony.run({TICKS}, 3)\n"
        for id, seen in zip(OBSERVED, observes):
            if seen != "null":
                lines += f"expect(\"observe {id}\", json.stringify(colony.observe(\"{id}\")), {raw(seen)})\n"
        lines += f"same(\"after observing\", {raw(snapshots[TICKS])})\n"
        case(f"observe-{name}", lines)

    # Ticks in a task of their own, as a server's would be: the colony is
    # shared by every task.
    for name in ("small", "medium"):
        snapshots, _ = expected[name]
        case(f"spawned-ticker-{name}", started(name) + f'''val ticker = spawn {{ colony.run(3, 4) }}
await(ticker)
same("after the ticker", {raw(snapshots[3])})
val more = spawn {{ colony.tick(2); colony.tick(1); colony.tick(4) }}
await(more)
same("after the second ticker", {raw(snapshots[6])})
''')
        case(f"spawned-start-{name}", f'''val starter = spawn {{ {started(name).strip()} }}
await(starter)
colony.run({TICKS}, 4)
same("run", {raw(snapshots[TICKS])})
''')

    # A new world started in another task, after this one ran a world:
    # nothing this task remembers of the old one carries over.
    snapshots, observes = expected["repacked"]
    for forks in (1, 4):
        lines = started("packed") + f"colony.run({TICKS}, {forks})\ncolony.observe(\"a0\")\n"
        lines += f"val restart = spawn {{ {started('repacked').strip()} }}\nawait(restart)\n"
        lines += f"same(\"restarted\", {raw(snapshots[0])})\n"
        lines += f"colony.run({TICKS}, {forks})\nsame(\"run\", {raw(snapshots[TICKS])})\n"
        lines += f"expect(\"observe a0\", json.stringify(colony.observe(\"a0\")), {raw(observes[0])})\n"
        case(f"restart-in-spawned-task-forks-{forks}", lines)

    # Bad `forks`, which change nothing.
    snapshots, _ = expected["small"]
    for bad in ("0", "-2", "2.5", "1.5"):
        case(f"bad-forks-tick-{bad}", started("small") + f'''expect("tick error", forksError(fn() {{ colony.tick({bad}) }}), {{forks: {bad}}})
expect("run error", forksError(fn() {{ colony.run(3, {bad}) }}), {{forks: {bad}}})
same("unchanged", {raw(snapshots[0])})
colony.tick(2)
same("then a tick", {raw(snapshots[1])})
''')

    # Speed: the same program on one fork and on four.
    speed, _ = run_starter(slug, SPEED_WORLD, SPEED_TICKS)
    final = json.loads(speed[SPEED_TICKS])
    seed, width, height, count = SPEED_WORLD
    cases.append({
        "id": "speedup-4-forks",
        "kind": "speedup",
        "source": f'''val main = fn() {{
colony.start({seed}, {width}, {height}, {count})
colony.run({SPEED_TICKS}, cfg("forks", 1))
val s = colony.snapshot()
expect("stats", json.stringify(s.stats), {raw(json.dumps(final["stats"], separators=(",", ":"), sort_keys=True))})
expect("store", s.store, {final["store"]})
expect("turn", s.turn, {final["turn"]})
finish()
}}
''',
        "fast": 4,
        "slow": 1,
        "ratio": 0.6,
        "seconds": 60,
    })

    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate case id"
    (HERE / "cases.json").write_text(json.dumps({"prelude": PRELUDE, "cases": cases}, indent=1) + "\n")
    print(len(cases), "cases")


if __name__ == "__main__":
    main()
