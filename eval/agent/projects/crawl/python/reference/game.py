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


class Game:
    def __init__(self, seed):
        self.rng = level.Rng(seed)
        self.seed = seed
        self.turn = 0
        self.colours = self.rng.shuffle(data.COLOURS)
        self.titles = self.rng.shuffle(data.TITLES)
        self.messages = [WELCOME]
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

    # Commands.

    def end_turn(self):
        self.turn += 1
        self.see()

    def can_step(self, dx, dy):
        x, y = self.player["x"], self.player["y"]
        if not self.passable(x + dx, y + dy):
            return False
        return dx == 0 or dy == 0 or (self.passable(x + dx, y) and self.passable(x, y + dy))

    def step(self, dx, dy):
        x, y = self.player["x"] + dx, self.player["y"] + dy
        if not self.can_step(dx, dy) or self.monster_at(x, y):
            return False
        self.player["x"], self.player["y"] = x, y
        self.end_turn()
        return True

    def run(self, dx, dy):
        while self.step(dx, dy):
            x, y = self.player["x"], self.player["y"]
            if self.terrain(x, y) in "+%" or self.item_at(x, y):
                return
            if any(self.visible(m["x"], m["y"]) for m in self.monsters):
                return

    def descend(self):
        if self.terrain(self.player["x"], self.player["y"]) != "%":
            self.messages.append("I see no way down.")
        elif self.depth == level.DEEPEST:
            self.messages.append("You need the Orb to leave.")
        else:
            self.enter_level(self.depth + 1)
            self.turn += 1

    def command(self, key, shift=False):
        self.messages = []
        if key == "space":
            self.end_turn()
        elif key == "enter":
            self.descend()
        elif key in DIRECTIONS:
            (self.run if shift else self.step)(*DIRECTIONS[key])

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
        return ["  ".join(self.messages)] + ["".join(r) for r in rows] + [status]

    def state(self):
        return {
            "seed": self.seed, "turn": self.turn, "depth": self.depth,
            "colours": self.colours, "titles": self.titles, "messages": self.messages,
            "map": self.map, "rooms": self.rooms, "items": self.items, "monsters": self.monsters,
            "memory": ["".join(r) for r in self.memory], "player": self.player, "rng": self.rng.state,
        }
