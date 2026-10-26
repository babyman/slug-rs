#!/usr/bin/env python3
"""Generates cases.json for the crawl project.

Each case starts a game, from a seed (`--seed N`) or from a snapshot
(`--load start.json`), plays an input script for slug.arcade's headless
mode, and expects the reference's frames (as window digests, see
harness/arcade_grader.py) and its final snapshot (`--dump`). The expected
results come from the Slug reference, reference/crawl.slug; every case is
also played on the Python reference, python/reference, and generation stops
if the two disagree.

Targeted cases build their snapshots from seed 1's first level (and a few
others) with helpers below; bot cases come from tools/bot.py.

Usage: make_cases.py   (needs the release slug build and the staged
slug.arcade clutch; $SLUG overrides the binary)
"""

import json
import os
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
sys.path.insert(0, str(REPO / "eval" / "agent" / "harness"))
sys.path.insert(0, str(HERE / "tools"))
import arcade_grader  # noqa: E402
from bot import play, pygame_  # noqa: E402
import data  # noqa: E402  (python/reference, via bot's path)

SLUG = os.environ.get("SLUG", str(REPO / "target" / "release" / "slug"))
WINDOW = 20
Game = pygame_.Game


def kind(name):
    return next(i for i, m in enumerate(data.MONSTERS) if m["name"] == name)


def potion(name):
    return {"kind": "potion", "type": next(i for i, p in enumerate(data.POTIONS) if p["name"] == name)}


def scroll(name):
    return {"kind": "scroll", "type": next(i for i, p in enumerate(data.SCROLLS) if p["name"] == name)}


def weapon(name, bonus=0):
    return {"kind": "weapon", "type": next(i for i, w in enumerate(data.WEAPONS) if w["name"] == name), "bonus": bonus}


def armour(name, bonus=0):
    return {"kind": "armour", "type": next(i for i, a in enumerate(data.ARMOURS) if a["name"] == name), "bonus": bonus}


FOOD = {"kind": "food"}
ORB = {"kind": "orb"}


# ---------------------------------------------------------------------------
# Snapshots.

def fresh(seed=1, depth=1):
    """Seed's game on its first level (or, generated after it, `depth`),
    with the level's monsters and items cleared and no messages."""
    g = Game(seed)
    for d in range(2, depth + 1):
        g.enter_level(d)
    g.monsters, g.items, g.messages = [], [], []
    return g


def at(g, x, y, terrain="."):
    assert g.map[y][x] in terrain, f"({x}, {y}) is {g.map[y][x]!r}, not {terrain!r}"
    g.player["x"], g.player["y"] = x, y
    g.see()
    return g


def monster(g, name, x, y, awake=True, hp=None):
    assert g.map[y][x] in ".#+%", (x, y)
    k = data.MONSTERS[kind(name)]
    hp = hp if hp is not None else 4 * k["level"]
    g.monsters.append({"x": x, "y": y, "kind": kind(name), "hp": hp, "maxHp": hp, "state": "awake" if awake else "asleep"})


def floor_item(g, x, y, item):
    assert g.map[y][x] in ".#+", (x, y)
    g.items.append({"x": x, "y": y, "item": dict(item)})
    g.see()


def give(g, item, count=1):
    for _ in range(count):
        g.add_to_pack(dict(item))


def tough(g, hp=60):
    g.player["hp"] = g.player["maxHp"] = hp


def keys(text):
    """Keys from a space-separated list: a letter, an upper-case letter for
    shift with it, or a key name such as space, enter, or left."""
    out = []
    for token in text.split():
        if len(token) == 1 and token.isupper():
            out.append((token.lower(), True))
        else:
            out.append((token, False))
    return out


# ---------------------------------------------------------------------------
# Running a case on both references.

def script(pressed):
    lines = ["0 -"]
    for i, (key, shift) in enumerate(pressed):
        f = 2 + 2 * i
        lines += [f"{f} {'shift ' if shift else ''}{key}", f"{f + 1} -"]
    lines.append(f"end {2 + 2 * len(pressed) + 1}")
    return "\n".join(lines) + "\n"


def make(case_id, description, start, text_or_keys):
    """A case from `start` (a seed number or a Game to snapshot) and keys."""
    pressed = keys(text_or_keys) if isinstance(text_or_keys, str) else text_or_keys
    if isinstance(start, int):
        args, files = ["--seed", str(start)], {}
        g = Game(start)
    else:
        snapshot = json.loads(json.dumps(start.state()))
        args, files = ["--load", "start.json"], {"start.json": json.dumps(snapshot) + "\n"}
        g = Game.load(snapshot)
    text = script(pressed)
    frames = 2 + 2 * len(pressed) + 1
    seconds = 20 + frames // 150
    hashes, _, error, dumped = arcade_grader.run(
        SLUG, REPO, HERE, HERE / "reference" / "crawl.slug", text, seconds * 3, args, files, True)
    if error:
        sys.exit(f"{case_id}: the Slug reference failed: {error}")
    for key, shift in pressed:
        g.command(key, shift)
    expected = json.loads(json.dumps(g.state()))
    difference = arcade_grader.first_difference(dumped, expected)
    if difference:
        sys.exit(f"{case_id}: the references disagree: {difference}")
    case = {"id": case_id, "description": description, "frames": frames, "seconds": seconds, "args": args}
    if files:
        case["files"] = files
    case.update({"script": text, "windows": arcade_grader.digests(hashes, WINDOW), "sounds": [], "dump": dumped})
    print(f"{case_id}: {len(pressed)} keys", file=sys.stderr)
    return case


# ---------------------------------------------------------------------------
# The cases. Seed 1's first level (see task.md's example) has lit rooms;
# room 6 (bottom left) has floor x 1-23, y 16-18, a door at (24, 17) with a
# corridor east to room 7's door at (29, 17), and a door at (8, 15) with a
# corridor north to room 3's door at (13, 10). Room 5 holds the stairs at
# (71, 9).

def generation():
    cases = [make(f"gen-seed-{s}", f"a new game with seed {s}: level 1", s, "") for s in range(1, 11)]
    for d in range(2, 9):
        for s in (1, 2):
            g = fresh(s)
            g.depth = d - 1
            stairs = next((x, y) for y, row in enumerate(g.map) for x, c in enumerate(row) if c == "%")
            at(g, *stairs, terrain="%")
            cases.append(make(f"gen-depth-{d}-{s}", f"taking the stairs down to depth {d} (seed {s}'s rng)", g, "enter"))
    return cases


def movement():
    cases = []
    cases.append(make("move-directions", "a move in each direction in a lit room",
                      at(fresh(), 12, 17), "h j k l y u b n"))
    cases.append(make("move-arrows", "the arrow keys move", at(fresh(), 12, 17), "left up right down right"))
    cases.append(make("move-walls", "moves into walls take no turn", at(fresh(), 1, 16), "h k y b u j h"))
    cases.append(make("move-door-diagonal", "no diagonal steps into or out of a doorway",
                      at(fresh(), 24, 17, "+"), "u n y b l h h u"))
    cases.append(make("move-door-into", "no diagonal step into a doorway from the room",
                      at(fresh(), 23, 16), "n j l"))
    cases.append(make("rest", "resting passes turns", at(fresh(), 12, 17), "space space space"))
    cases.append(make("keys-ignored", "keys that are not commands do nothing, not even clear the message line; "
                      "then an arrow key moves", 1, "x m z f g left"))
    cases.append(make("keys-same-frame", "the welcome message clears on the first command", 1, "l"))
    cases.append(make("run-room", "runs end at walls", at(fresh(), 12, 17), "L K H J"))
    cases.append(make("run-door", "a run ends on a door", at(fresh(), 14, 17), "L L"))
    cases.append(make("run-corridor", "a run along a corridor ends where it turns",
                      at(fresh(), 8, 14, "#"), "K L J"))
    g = at(fresh(), 3, 17)
    floor_item(g, 10, 17, FOOD)
    cases.append(make("run-item", "a run ends on picking up an item", g, "L L"))
    g = at(fresh(), 25, 17, "#")
    monster(g, "kobold", 40, 17, awake=False)
    cases.append(make("run-monster", "a run ends when a monster comes into view", g, "L L"))
    g = at(fresh(), 12, 17)
    monster(g, "kobold", 13, 17, awake=False)
    cases.append(make("run-into-monster", "running never attacks", g, "L space"))
    g = at(fresh(), 71, 9, "%")
    cases.append(make("stairs", "taking the stairs down", g, "enter j enter"))
    cases.append(make("stairs-off", "no stairs here", at(fresh(), 12, 17), "enter"))
    return cases


def seeing():
    cases = []
    cases.append(make("see-corridor", "in a corridor the player sees only the neighbours",
                      at(fresh(), 25, 17, "#"), "l l l l l"))
    cases.append(make("see-door", "standing in a doorway shows the whole lit room",
                      at(fresh(), 28, 17, "#"), "l l"))
    # A dark room: the first seed and depth with one.
    for seed in range(1, 40):
        g = fresh(seed, 6)
        dark = [r for r in g.rooms if not r["gone"] and r["dark"]]
        if dark:
            r = dark[0]
            g = at(g, r["x"] + 1, r["y"] + 1)
            monster(g, "bat", r["x"] + r["w"] - 2, r["y"] + 1, awake=False)
            cases.append(make("see-dark-room", "in a dark room the player sees only the neighbours",
                              g, "L J H " + "l " * 3))
            break
    return cases


def monsters():
    cases = []
    for m in data.MONSTERS:
        g = tough(at(fresh(), 12, 17)) or at(fresh(), 12, 17)
        tough(g)
        g.player["gold"] = 100
        give(g, potion("healing"))
        monster(g, m["name"], 13, 17)
        name = m["name"].replace(" ", "-")
        cases.append(make(f"attacked-by-{name}", f"an awake {m['name']} attacks", g, "space " * 8))
        g = at(fresh(), 12, 17)
        tough(g)
        monster(g, m["name"], 13, 17, awake=False)
        cases.append(make(f"attack-{name}", f"attacking a sleeping {m['name']}", g, "l " * 8))
    g = at(fresh(), 3, 17)
    monster(g, "jackal", 20, 17, awake=False)
    monster(g, "kobold", 21, 16, awake=False)
    monster(g, "kobold", 3, 9, awake=False)
    monster(g, "floating eye", 4, 17, awake=False)
    cases.append(make("wake", "who wakes: mean monsters at once, others by chance, none from other rooms", g,
                      "space " * 8))
    g = at(fresh(), 12, 17)
    tough(g)
    monster(g, "orc", 5, 8)
    monster(g, "hobgoblin", 60, 16)
    cases.append(make("chase", "awake monsters come by the shortest path", g, "space " * 20))
    g = at(fresh(), 27, 17, "#")
    tough(g)
    monster(g, "zombie", 30, 17)
    monster(g, "orc", 31, 17)
    monster(g, "kobold", 31, 18)
    cases.append(make("chase-queue", "monsters wait behind each other in a corridor", g, "space " * 8))
    g = at(fresh(), 12, 17)
    tough(g)
    monster(g, "bat", 16, 17)
    monster(g, "bat", 18, 18)
    cases.append(make("bats", "bats flutter", g, "space " * 12))
    g = at(fresh(), 12, 17)
    m = monster(g, "troll", 40, 17, awake=False)
    g.monsters[-1]["hp"] = 5
    cases.append(make("troll-regenerates", "trolls regain hit points, even asleep", g, "space " * 6))
    g = at(fresh(), 12, 17)
    g.player["xp"] = 9
    monster(g, "kobold", 13, 17, hp=1)
    cases.append(make("level-up", "experience gives a level", g, "l " * 6))
    g = at(fresh(), 12, 17)
    g.player["xp"] = 75
    g.player["level"] = 4
    monster(g, "troll", 13, 17, hp=1)
    cases.append(make("level-up-twice", "one kill can give two levels", g, "l " * 8))
    g = at(fresh(), 12, 17)
    g.player["hp"] = 2
    monster(g, "troll", 13, 17)
    cases.append(make("death", "dying, then starting the next seed's game", g,
                      "space space space l space space enter l"))
    g = at(fresh(), 12, 17)
    tough(g)
    for x, y in ((11, 16), (12, 16), (13, 16), (11, 17), (13, 17), (11, 18), (12, 18), (13, 18)):
        monster(g, "kobold", x, y)
    cases.append(make("more", "--More-- pages a long turn's messages", g, "space " * 12 + "l l"))
    g = at(fresh(), 12, 17)
    monster(g, "floating eye", 13, 17, hp=50)
    monster(g, "kobold", 11, 17)
    tough(g)
    cases.append(make("frozen", "the floating eye freezes the player", g, "l " * 6 + "space " * 6))
    return cases


def items():
    cases = []
    g = at(fresh(), 12, 17)
    floor_item(g, 13, 17, {"kind": "gold", "amount": 37})
    floor_item(g, 14, 17, FOOD)
    floor_item(g, 15, 17, potion("healing"))
    floor_item(g, 16, 17, potion("healing"))
    floor_item(g, 17, 17, weapon("dagger", -1))
    floor_item(g, 18, 17, scroll("identify"))
    floor_item(g, 19, 17, ORB)
    cases.append(make("pickup", "picking up gold, stacks, and new entries", g, "l " * 8))
    g = at(fresh(), 12, 17)
    for i in range(19):
        give(g, weapon(data.WEAPONS[i % 4]["name"], i // 4))
    give(g, FOOD)
    floor_item(g, 13, 17, potion("poison"))
    floor_item(g, 11, 17, FOOD)
    cases.append(make("pack-full", "a full pack, which still takes stacking items", g,
                      "l h h d e l l i space"))
    for p in data.POTIONS:
        g = at(fresh(), 12, 17)
        give(g, {"kind": "potion", "type": data.POTIONS.index(p)}, 2)
        g.player["hp"] = 7
        g.player["str"] = 4
        monster(g, "kobold", 40, 17, awake=False)
        name = p["name"].replace(" ", "-")
        cases.append(make(f"quaff-{name}", f"quaffing a potion of {p['name']}", g,
                          "i space q d l l k j r h space q d i space"))
    g = at(fresh(), 12, 17)
    give(g, potion("healing"))
    cases.append(make("quaff-healing-full", "healing at full hit points raises the maximum", g, "q d"))
    g = at(fresh(), 12, 17)
    give(g, potion("see monsters"))
    cases.append(make("quaff-see-nothing", "sensing with no monsters", g, "q d"))
    for s in data.SCROLLS:
        g = at(fresh(), 12, 17)
        give(g, {"kind": "scroll", "type": data.SCROLLS.index(s)}, 2)
        give(g, potion("blindness"), 2)
        give(g, scroll("magic mapping"))
        name = s["name"].replace(" ", "-")
        cases.append(make(f"read-{name}", f"reading a scroll of {s['name']}", g, "r d r d i space"))
    g = at(fresh(), 12, 17)
    give(g, scroll("identify"))
    cases.append(make("read-identify-nothing", "identify with nothing to identify", g, "r d"))
    g = at(fresh(), 12, 17)
    give(g, scroll("enchant weapon"))
    give(g, scroll("enchant armour"))
    cases.append(make("read-enchant-bare", "enchanting with no weapon or armour", g, "d a l a b r d r e i space"))
    g = at(fresh(), 12, 17)
    give(g, weapon("dagger", 2))
    give(g, armour("plate mail", -1))
    cases.append(make("equipment", "wielding, wearing, and taking off", g,
                      "w d w d w a w c a d a b a d a d i space"))
    g = at(fresh(), 12, 17)
    give(g, FOOD, 2)
    cases.append(make("drop", "dropping: worn armour, stacks, occupied squares", g,
                      "d b d c d c l d c h d a l d a"))
    g = at(fresh(), 71, 9, "%")
    cases.append(make("drop-stairs", "nothing can be dropped on the stairs", g, "d c"))
    g = at(fresh(), 12, 17)
    cases.append(make("prompts", "prompts: cancel, other keys, missing letters, wrong kinds", g,
                      "q r w a w left enter z w c e b e space e c"))
    g = at(fresh(), 12, 17)
    g.pack = []
    g.player["weapon"] = g.player["armour"] = None
    cases.append(make("empty-pack", "nothing to use with an empty pack", g, "i e q r w a d l"))
    g = at(fresh(), 12, 17)
    g.player["confused"] = 6
    cases.append(make("confused", "confused moves go any way, and never run", g, "l L l k j h space space l"))
    g = at(fresh(), 12, 17)
    g.player["blind"] = 3
    give(g, scroll("identify"))
    monster(g, "floating eye", 13, 17, hp=40)
    cases.append(make("blind", "blind: nothing seen, no reading, no freezing gaze", g, "r l l j j space j"))
    g = at(fresh(), 12, 17)
    tough(g)
    give(g, potion("poison"), 3)
    give(g, scroll("teleportation"))
    monster(g, "nymph", 13, 17)
    cases.append(make("nymph", "a nymph steals an entry that is not wielded or worn", g, "space " * 10 + "i space"))
    g = at(fresh(), 12, 17)
    tough(g)
    g.player["gold"] = 25
    monster(g, "leprechaun", 13, 17)
    cases.append(make("leprechaun", "a leprechaun steals gold", g, "space " * 10))
    g = at(fresh(), 71, 9, "%")
    g.depth = 8
    cases.append(make("orb-needed", "the way out needs the Orb", g, "enter"))
    give(g, ORB)
    g.player["gold"] = 321
    cases.append(make("win", "escaping with the Orb, then the next seed's game", g, "enter space enter l"))
    return cases


def hunger():
    cases = []
    for food, turns, name in ((152, 4, "hungry"), (52, 4, "weak"), (3, 20, "faint"), (-847, 4, "starve")):
        g = at(fresh(), 12, 17)
        g.player["food"] = food
        cases.append(make(f"hunger-{name}", f"hunger: {name}", g, "space " * turns + "enter enter"))
    g = at(fresh(), 12, 17)
    g.player["food"] = -100
    cases.append(make("eat-fainting", "eating from below zero", g, "e c"))
    g = at(fresh(), 12, 17)
    g.player["food"] = 1950
    cases.append(make("eat-full", "the food counter stops at 2000", g, "e c"))
    g = at(fresh(), 12, 17)
    g.player["hp"] = 5
    g.player["maxHp"] = 30
    cases.append(make("heal-level-1", "healing at level 1", g, "space " * 45))
    g = at(fresh(), 12, 17)
    g.player.update(hp=5, maxHp=30, level=10)
    cases.append(make("heal-level-10", "healing at level 10", g, "space " * 12))
    return cases


def bots():
    cases = []
    for seed, commands in ((101, 600), (102, 600), (103, 1000), (104, 1000), (105, 1500), (106, 1500)):
        _, pressed = play(seed, commands, random.Random(seed))
        cases.append(make(f"bot-{seed}", f"a bot plays seed {seed} for {commands} commands", seed, pressed))
    return cases


def main():
    cases = generation() + movement() + seeing() + monsters() + items() + hunger() + bots()
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    (HERE / "cases.json").write_text(json.dumps({"kind": "arcade", "window": WINDOW, "cases": cases}, indent=1) + "\n")
    print(f"{len(cases)} cases", file=sys.stderr)


if __name__ == "__main__":
    main()
