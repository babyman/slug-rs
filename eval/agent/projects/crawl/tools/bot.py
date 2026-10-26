"""A random-ish bot that plays Crawl on the Python reference, and the
slug.arcade input scripts for its keys. Used by make_cases.py and
compare_references.py."""
import os
import sys
from collections import deque

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE + "/python/reference")
import game as pygame_  # noqa: E402


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
    """An input script pressing each (key, shift) for one frame, two frames
    apart, starting at frame 2."""
    lines = ["0 -"]
    for i, (key, shift) in enumerate(keys):
        f = 2 + 2 * i
        lines += [f"{f} {'shift ' if shift else ''}{key}", f"{f + 1} -"]
    lines.append(f"end {2 + 2 * len(keys) + 1}")
    return "\n".join(lines) + "\n"
