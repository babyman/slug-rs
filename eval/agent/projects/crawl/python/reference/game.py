"""Crawl's game state, commands, and screen text, written from task.md."""
import data
import level

DIRECTIONS = {
    "h": (-1, 0), "j": (0, 1), "k": (0, -1), "l": (1, 0),
    "y": (-1, -1), "u": (1, -1), "b": (-1, 1), "n": (1, 1),
}
GLYPHS = {"gold": "*", "potion": "!", "scroll": "?", "food": ":", "weapon": ")", "armour": "]", "orb": ","}
PASSABLE = ".#+%"
WELCOME = "Welcome to the dungeon. Find the Orb on depth 8."
ORDER = [(-1, 0), (0, 1), (0, -1), (1, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)]


def strength_hit(s):
    return 1 if s >= 18 else -1 if s <= 6 else 0


def strength_damage(s):
    return 2 if s >= 18 else 1 if s == 17 else -1 if s <= 6 else 0


def paginate(messages):
    """The page to show now and the rest."""
    if len("  ".join(messages)) <= 80:
        return messages, []
    n = 1
    while n < len(messages) and len("  ".join(messages[:n + 1])) + len(" --More--") <= 80:
        n += 1
    return messages[:n], messages[n:]


class Game:
    def __init__(self, seed):
        self.rng = level.Rng(seed)
        self.seed = seed
        self.turn = 0
        self.colours = self.rng.shuffle(data.COLOURS)
        self.titles = self.rng.shuffle(data.TITLES)
        self.messages = [WELCOME]
        self.more = []
        self.dead = False
        self.player = {"x": 0, "y": 0, "hp": 12, "maxHp": 12, "str": 16, "maxStr": 16,
                       "gold": 0, "level": 1, "xp": 0, "armour": 0}
        self.enter_level(1)

    # The level.

    def enter_level(self, depth):
        lv = level.generate(self.rng, depth)
        self.depth = depth
        self.map = lv["map"]
        self.rooms = lv["rooms"]
        self.items = lv["items"]
        self.monsters = lv["monsters"]
        self.memory = [[" "] * level.WIDTH for _ in range(level.HEIGHT)]
        self.player["x"], self.player["y"] = lv["player"]["x"], lv["player"]["y"]
        self.see()

    def terrain(self, x, y):
        if 0 <= x < level.WIDTH and 0 <= y < level.HEIGHT:
            return self.map[y][x]
        return " "

    def passable(self, x, y):
        return self.terrain(x, y) in PASSABLE

    def item_at(self, x, y):
        return next((i for i in self.items if (i["x"], i["y"]) == (x, y)), None)

    def monster_at(self, x, y):
        return next((m for m in self.monsters if (m["x"], m["y"]) == (x, y)), None)

    def room_at(self, x, y):
        for r in self.rooms:
            if not r["gone"] and r["x"] <= x < r["x"] + r["w"] and r["y"] <= y < r["y"] + r["h"]:
                return r
        return None

    def lit_room(self):
        room = self.room_at(self.player["x"], self.player["y"])
        return room if room and not room["dark"] else None

    def visible(self, x, y):
        px, py = self.player["x"], self.player["y"]
        if abs(x - px) <= 1 and abs(y - py) <= 1:
            return True
        r = self.lit_room()
        return bool(r) and r["x"] <= x < r["x"] + r["w"] and r["y"] <= y < r["y"] + r["h"]

    def see(self):
        squares = []
        r = self.lit_room()
        if r:
            squares += [(x, y) for y in range(r["y"], r["y"] + r["h"]) for x in range(r["x"], r["x"] + r["w"])]
        px, py = self.player["x"], self.player["y"]
        squares += [(px + dx, py + dy) for dy in (-1, 0, 1) for dx in (-1, 0, 1)]
        for x, y in squares:
            if 0 <= x < level.WIDTH and 0 <= y < level.HEIGHT:
                item = self.item_at(x, y)
                self.memory[y][x] = GLYPHS[item["item"]["kind"]] if item else self.map[y][x]

    # Monsters and combat.

    def step_ok(self, x, y, dx, dy):
        if not self.passable(x + dx, y + dy):
            return False
        return dx == 0 or dy == 0 or (self.passable(x + dx, y) and self.passable(x, y + dy))

    def kind(self, m):
        return data.MONSTERS[m["kind"]]

    def hits(self, lvl, bonus, armour):
        return self.rng.below(20) + 1 + lvl + bonus >= 10 + armour

    def say(self, text):
        self.messages.append(text)

    def die(self, killer):
        self.dead = True
        self.say("You die.")
        self.say(f"Killed by the {killer} on depth {self.depth}.")
        self.say("Press enter to play again.")

    def gain_xp(self, xp):
        p = self.player
        p["xp"] += xp
        need = 10 * 2 ** (p["level"] - 1)
        while p["xp"] >= need:
            gain = self.rng.roll(1, 10)
            p["level"] += 1
            p["hp"] += gain
            p["maxHp"] += gain
            self.say(f"Welcome to level {p['level']}.")
            need *= 2

    def distances(self):
        start = (self.player["x"], self.player["y"])
        dist = {start: 0}
        queue = [start]
        for x, y in queue:
            for dx, dy in ORDER:
                q = (x + dx, y + dy)
                if q not in dist and self.step_ok(x, y, dx, dy):
                    dist[q] = dist[(x, y)] + 1
                    queue.append(q)
        return dist

    def near(self, x, y):
        px, py = self.player["x"], self.player["y"]
        if abs(x - px) <= 1 and abs(y - py) <= 1:
            return True
        r = self.room_at(px, py)
        return bool(r) and r["x"] <= x < r["x"] + r["w"] and r["y"] <= y < r["y"] + r["h"]

    def open_for(self, x, y):
        return not self.monster_at(x, y) and (x, y) != (self.player["x"], self.player["y"])

    def monster_attack(self, m):
        """Returns whether the monster is still on the level."""
        k = self.kind(m)
        p = self.player
        if not self.hits(k["level"], 0, p["armour"]):
            self.say(f"The {k['name']} misses.")
            return True
        p["hp"] -= self.rng.roll(*k["damage"])
        self.say(f"The {k['name']} hits you.")
        if p["hp"] <= 0:
            self.die(k["name"])
        elif "stealsGold" in k["flags"] and p["gold"] > 0:
            take = self.rng.below(50 + 10 * self.depth) + 2
            p["gold"] = max(0, p["gold"] - take)
            self.monsters.remove(m)
            self.say("Your purse feels lighter.")
            return False
        return True

    def monster_act(self, m, dist):
        """Returns (still on the level, distances)."""
        k = self.kind(m)
        flags = k["flags"]
        if "regenerates" in flags and m["hp"] < m["maxHp"]:
            m["hp"] += 1
        if m["state"] == "asleep":
            if "still" not in flags and self.near(m["x"], m["y"]) and ("mean" in flags or self.rng.below(3) == 0):
                m["state"] = "awake"
            return True, dist
        if "still" in flags:
            return True, dist
        if "flies" in flags and self.rng.below(2) == 0:
            dx, dy = ORDER[self.rng.below(8)]
            if self.step_ok(m["x"], m["y"], dx, dy) and self.open_for(m["x"] + dx, m["y"] + dy):
                m["x"] += dx
                m["y"] += dy
            return True, dist
        dx, dy = self.player["x"] - m["x"], self.player["y"] - m["y"]
        if abs(dx) <= 1 and abs(dy) <= 1 and self.step_ok(m["x"], m["y"], dx, dy):
            return self.monster_attack(m), dist
        dist = dist or self.distances()
        here = dist.get((m["x"], m["y"]))
        if here is None:
            return True, dist
        best, best_d = None, here
        for dx, dy in ORDER:
            d = dist.get((m["x"] + dx, m["y"] + dy))
            if d is not None and d < best_d and self.step_ok(m["x"], m["y"], dx, dy) \
                    and self.open_for(m["x"] + dx, m["y"] + dy):
                best, best_d = (dx, dy), d
        if best:
            m["x"] += best[0]
            m["y"] += best[1]
        return True, dist

    def monsters_act(self):
        i, dist = 0, None
        while i < len(self.monsters) and not self.dead:
            stays, dist = self.monster_act(self.monsters[i], dist)
            if stays:
                i += 1

    def attack(self, m):
        """The player attacks; returns the turns frozen afterwards."""
        m["state"] = "awake"
        k = self.kind(m)
        p = self.player
        if self.hits(p["level"], strength_hit(p["str"]), k["armour"]):
            damage = max(0, self.rng.roll(1, 2) + strength_damage(p["str"]))
            if m["hp"] - damage <= 0:
                self.monsters.remove(m)
                self.say(f"You defeat the {k['name']}.")
                self.gain_xp(k["xp"])
            else:
                m["hp"] -= damage
                self.say(f"You hit the {k['name']}.")
        else:
            self.say(f"You miss the {k['name']}.")
        if "paralyses" in k["flags"] and m in self.monsters:
            self.say(f"You are frozen by the {k['name']}'s gaze!")
            return self.rng.below(4) + 2
        return 0

    # Commands.

    def end_turn(self):
        self.monsters_act()
        self.turn += 1
        self.see()

    def step(self, dx, dy):
        x, y = self.player["x"], self.player["y"]
        if not self.step_ok(x, y, dx, dy):
            return False
        m = self.monster_at(x + dx, y + dy)
        if m:
            frozen = self.attack(m)
            self.end_turn()
            for _ in range(frozen):
                if self.dead:
                    break
                self.end_turn()
            return False
        self.player["x"], self.player["y"] = x + dx, y + dy
        self.end_turn()
        return True

    def run(self, dx, dy):
        while not self.monster_at(self.player["x"] + dx, self.player["y"] + dy) and self.step(dx, dy):
            x, y = self.player["x"], self.player["y"]
            if self.terrain(x, y) in "+%" or self.item_at(x, y) or self.messages or self.dead:
                return
            if any(self.visible(m["x"], m["y"]) for m in self.monsters):
                return

    def descend(self):
        if self.terrain(self.player["x"], self.player["y"]) != "%":
            self.say("I see no way down.")
        elif self.depth == level.DEEPEST:
            self.say("You need the Orb to leave.")
        else:
            self.enter_level(self.depth + 1)
            self.turn += 1

    def command(self, key, shift=False):
        if self.more:
            if key == "space":
                self.messages, self.more = paginate(self.more)
            return
        if self.dead:
            if key == "enter":
                self.__init__(self.seed + 1)
            return
        self.messages = []
        if key == "space":
            self.end_turn()
        elif key == "enter":
            self.descend()
        elif key in DIRECTIONS:
            (self.run if shift else self.step)(*DIRECTIONS[key])
        self.messages, self.more = paginate(self.messages)

    # Output.

    def text(self):
        rows = [list(row) for row in self.memory]
        for m in self.monsters:
            if self.visible(m["x"], m["y"]):
                rows[m["y"]][m["x"]] = data.MONSTERS[m["kind"]]["letter"]
        rows[self.player["y"]][self.player["x"]] = "@"
        p = self.player
        status = (f"Depth:{self.depth}  Gold:{p['gold']}  Hp:{p['hp']}({p['maxHp']})  "
                  f"Str:{p['str']}({p['maxStr']})  Arm:{p['armour']}  Exp:{p['level']}/{p['xp']}")
        more = " --More--" if self.more else ""
        return ["  ".join(self.messages) + more] + ["".join(r) for r in rows] + [status]

    def state(self):
        return {
            "seed": self.seed, "turn": self.turn, "depth": self.depth,
            "colours": self.colours, "titles": self.titles, "messages": self.messages,
            "more": self.more, "dead": self.dead,
            "map": self.map, "rooms": self.rooms, "items": self.items, "monsters": self.monsters,
            "memory": ["".join(r) for r in self.memory], "player": self.player, "rng": self.rng.state,
        }
