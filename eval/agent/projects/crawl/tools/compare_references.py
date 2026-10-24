#!/usr/bin/env python3
"""Plays seeded games with a bot on the Python reference, replays the keys on
the Slug reference headless, and compares the final states and screens.

Usage: compare_references.py <games> <commands per game> [slug binary]
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
    g = pygame_.Game(seed)
    keys = []
    for _ in range(commands):
        stairs = next(((x, y) for y, row in enumerate(g.map) for x, c in enumerate(row) if c == "%"))
        r = rnd.random()
        px, py = g.player["x"], g.player["y"]
        foes = [k for k, (dx, dy) in pygame_.DIRECTIONS.items()
                if g.monster_at(px + dx, py + dy) and g.step_ok(px, py, dx, dy)]
        if g.more:
            key, shift = ("space" if r < 0.9 else rnd.choice("hjl")), False
        elif g.dead:
            key, shift = ("enter" if r < 0.8 else "space"), False
        elif foes and r < 0.85:
            key, shift = rnd.choice(foes), rnd.random() < 0.1
        elif (g.player["x"], g.player["y"]) == stairs and r < 0.7:
            key, shift = "enter", False
        elif r < 0.5:
            key, shift = path_step(g, stairs) or "space", False
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

slug = sys.argv[3] if len(sys.argv) > 3 else REPO + "/target/release/slug"
env = dict(os.environ, SLUG_HOME=REPO)
seeds, commands = int(sys.argv[1]), int(sys.argv[2])
ok = 0
with tempfile.TemporaryDirectory() as tmp:
    for seed in range(1, seeds + 1):
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
