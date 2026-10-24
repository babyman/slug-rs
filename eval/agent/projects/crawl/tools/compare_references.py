#!/usr/bin/env python3
"""Plays seeded games with a bot on the Python reference, replays the keys on
the Slug reference headless, and compares the final states and screens.

Usage: compare_references.py <games> <commands per game>
       compare_references.py find <seed> <commands>   (first differing command)
The slug binary is $SLUG, or the repository's release build.
"""
import json, os, random, subprocess, sys, tempfile
from collections import deque
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(HERE))))
sys.path.insert(0, HERE + "/python/reference")
import game as pygame_, level

def path_step(g, target):
    """First direction key on a shortest path to target, or None."""
    start = (g.player["x"], g.player["y"])
    prev = {start: None}
    todo = deque([start])
    while todo:
        cur = todo.popleft()
        if cur == target:
            while prev[cur] and prev[cur][0] != start:
                cur = prev[cur][0]
            return prev[cur][1] if prev[cur] else None
        for key, (dx, dy) in pygame_.DIRECTIONS.items():
            x, y = cur[0] + dx, cur[1] + dy
            if (x, y) in prev or not g.passable(x, y):
                continue
            if dx and dy and not (g.passable(cur[0] + dx, cur[1]) and g.passable(cur[0], cur[1] + dy)):
                continue
            if g.monster_at(x, y):
                continue
            prev[(x, y)] = (cur, key)
            todo.append((x, y))
    return None

def play(seed, commands, rnd):
    """Even seeds' bots go for items as well as the stairs."""
    g = pygame_.Game(seed)
    keys = []
    for _ in range(commands):
        stairs = next(((x, y) for y, row in enumerate(g.map) for x, c in enumerate(row) if c == "%"))
        orb = next(((i["x"], i["y"]) for i in g.items if i["item"]["kind"] == "orb"), None)
        if orb:
            goal = orb
        elif g.items and seed % 2 == 0:
            goal = min(((i["x"], i["y"]) for i in g.items),
                       key=lambda q: abs(q[0] - g.player["x"]) + abs(q[1] - g.player["y"]))
        else:
            goal = stairs
        r = rnd.random()
        px, py = g.player["x"], g.player["y"]
        foes = [k for k, (dx, dy) in pygame_.DIRECTIONS.items()
                if g.monster_at(px + dx, py + dy) and g.step_ok(px, py, dx, dy)]
        if g.more:
            key, shift = ("space" if r < 0.9 else rnd.choice("hjl")), False
        elif g.dead or g.won:
            key, shift = ("enter" if r < 0.8 else "space"), False
        elif g.showing_pack:
            key, shift = ("space" if r < 0.8 else "q"), False
        elif g.prompt:
            if r < 0.8 and g.pack:
                key = rnd.choice(g.pack)["letter"]
            else:
                key = rnd.choice(["space", "z", "left", "enter"])
            shift = False
        elif g.player["food"] <= 150 and r < 0.5:
            key, shift = "e", False
        elif r < 0.08:
            key, shift = rnd.choice("eqrwadiqqrr"), False
        elif foes and r < 0.85:
            key, shift = rnd.choice(foes), rnd.random() < 0.1
        elif (g.player["x"], g.player["y"]) == stairs and r < 0.7:
            key, shift = "enter", False
        elif r < 0.5:
            key, shift = path_step(g, goal) or "space", False
        elif r < 0.75:
            key, shift = rnd.choice("hjklyubn"), True
        elif r < 0.95:
            key, shift = rnd.choice("hjklyubn"), False
        else:
            key, shift = rnd.choice(["space", "enter"]), False
        g.command(key, shift)
        keys.append((key, shift))
    return g, keys

def script(keys):
    lines = ["0 -"]
    for i, (key, shift) in enumerate(keys):
        f = 2 + 2 * i
        lines += [f"{f} {'shift ' if shift else ''}{key}", f"{f + 1} -"]
    lines.append(f"end {2 + 2 * len(keys) + 1}")
    return "\n".join(lines) + "\n"

slug = os.environ.get("SLUG", REPO + "/target/release/slug")
env = dict(os.environ, SLUG_HOME=REPO)
def slug_state(seed, keys, tmp):
    open(f"{tmp}/s.txt", "w").write(script(keys))
    subprocess.run([slug, "crawl.slug", "--seed", str(seed), "--dump", f"{tmp}/d.json"], cwd=HERE + "/reference",
                   env=dict(env, SLUG_ARCADE_HEADLESS=f"{tmp}/s.txt", SLUG_ARCADE_OUT=tmp), check=True)
    return json.load(open(f"{tmp}/d.json"))

def python_state(seed, keys):
    g = pygame_.Game(seed)
    for key, shift in keys:
        g.command(key, shift)
    return json.loads(json.dumps(dict(g.state(), text=g.text())))

if sys.argv[1] == "find":
    # find <seed> <commands>: the first command after which the two differ.
    seed, commands = int(sys.argv[2]), int(sys.argv[3])
    _, keys = play(seed, commands, random.Random(seed))
    with tempfile.TemporaryDirectory() as tmp:
        lo, hi = 0, len(keys)  # states agree after lo commands, differ after hi
        while hi - lo > 1:
            mid = (lo + hi) // 2
            if slug_state(seed, keys[:mid], tmp) == python_state(seed, keys[:mid]):
                lo = mid
            else:
                hi = mid
        before = python_state(seed, keys[:lo])
        got, want = slug_state(seed, keys[:hi], tmp), python_state(seed, keys[:hi])
        print(f"command {hi}: {keys[hi - 1]}")
        print("before:", "\n".join(before["text"][:1] + before["text"][23:]))
        print("slug:  ", got["text"][0], "|", got["text"][23])
        print("python:", want["text"][0], "|", want["text"][23])
        for k in want:
            if got.get(k) != want[k] and k not in ("text", "memory", "map"):
                print(k, "slug:", json.dumps(got.get(k))[:300])
                print(k, "py:  ", json.dumps(want[k])[:300])
    sys.exit(0)

seeds, commands = int(sys.argv[1]), int(sys.argv[2])
ok = 0
with tempfile.TemporaryDirectory() as tmp:
    for seed in range(int(os.environ.get("FIRST", 1)), int(os.environ.get("FIRST", 1)) + seeds):
        g, keys = play(seed, commands, random.Random(seed))
        open(f"{tmp}/s.txt", "w").write(script(keys))
        subprocess.run([slug, "crawl.slug", "--seed", str(seed), "--dump", f"{tmp}/d.json"], cwd=HERE + "/reference",
                       env=dict(env, SLUG_ARCADE_HEADLESS=f"{tmp}/s.txt", SLUG_ARCADE_OUT=tmp), check=True)
        want = json.loads(json.dumps(dict(g.state(), text=g.text())))
        got = json.load(open(f"{tmp}/d.json"))
        diff = [k for k in want if got.get(k) != want[k]] + [k for k in got if k not in want]
        if diff:
            print("seed", seed, "differs in", diff)
        else:
            ok += 1
        print(f"seed {seed}: seed now {g.seed} depth {g.depth} turn {g.turn} xp {g.player['xp']}", file=sys.stderr)
print(ok, "of", seeds, "identical")
