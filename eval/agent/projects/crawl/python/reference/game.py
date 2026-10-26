"""Crawl's game state, commands, and screen text, written from task.md."""
import data
import level

DIRECTIONS = {
    "h": (-1, 0), "j": (0, 1), "k": (0, -1), "l": (1, 0),
    "y": (-1, -1), "u": (1, -1), "b": (-1, 1), "n": (1, 1),
    "left": (-1, 0), "down": (0, 1), "up": (0, -1), "right": (1, 0),
}
ORDER = [(-1, 0), (0, 1), (0, -1), (1, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)]
GLYPHS = {"gold": "*", "potion": "!", "scroll": "?", "food": ":", "weapon": ")", "armour": "]", "orb": ","}
PASSABLE = ".#+%"
WELCOME = "Welcome to the dungeon. Find the Orb on depth 8."
LETTERS = "abcdefghijklmnopqrstuvwxyz"
PACK_SIZE = 22
FOOD_START, FOOD_MAX, STARVED = 1300, 2000, -850

# command: (prompt verb, kind needed or None, nothing to use, wrong kind)
ITEM_COMMANDS = {
    "e": ("Eat", "food", "You have nothing to eat.", "You can't eat that."),
    "q": ("Quaff", "potion", "You have nothing to quaff.", "You can't quaff that."),
    "r": ("Read", "scroll", "You have nothing to read.", "You can't read that."),
    "w": ("Wield", "weapon", "You have nothing to wield.", "You can't wield that."),
    "a": ("Wear or take off", "armour", "You have nothing to wear.", "You can't wear that."),
    "d": ("Drop", None, "You have nothing to drop.", ""),
}


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


def sign(n):
    return f"+{n}" if n >= 0 else str(n)


def an(word):
    return ("an " if word[0] in "aeiou" else "a ") + word


class Game:
    def __init__(self, seed):
        self.rng = level.Rng(seed)
        self.seed = seed
        self.turn = 0
        self.colours = self.rng.shuffle(data.COLOURS)
        self.titles = self.rng.shuffle(data.TITLES)
        self.known = {"potions": [], "scrolls": []}
        self.messages = [WELCOME]
        self.more = []
        self.prompt = None
        self.showing_pack = False
        self.dead = False
        self.won = False
        self.sense = False
        self.pack = [
            {"letter": "a", "item": {"kind": "weapon", "type": 0, "bonus": 1}, "count": 1},
            {"letter": "b", "item": {"kind": "armour", "type": 1, "bonus": 1}, "count": 1},
            {"letter": "c", "item": {"kind": "food"}, "count": 1},
        ]
        self.player = {"x": 0, "y": 0, "hp": 12, "maxHp": 12, "str": 16, "maxStr": 16, "gold": 0,
                       "level": 1, "xp": 0, "weapon": "a", "armour": "b", "food": FOOD_START,
                       "regen": 0, "frozen": 0, "confused": 0, "blind": 0}
        self.enter_level(1)

    @classmethod
    def load(cls, saved):
        """The game a snapshot holds, exactly as it was saved."""
        import copy
        saved = copy.deepcopy(saved)
        g = cls.__new__(cls)
        g.rng = level.Rng(1)
        g.rng.state = saved["rng"]
        g.seed, g.turn, g.depth = saved["seed"], saved["turn"], saved["depth"]
        g.colours, g.titles, g.known = saved["colours"], saved["titles"], saved["known"]
        g.messages, g.more, g.prompt = saved["messages"], saved["more"], saved["prompt"]
        g.showing_pack, g.dead, g.won, g.sense = saved["showingPack"], saved["dead"], saved["won"], saved["sense"]
        g.map, g.rooms, g.items, g.monsters = saved["map"], saved["rooms"], saved["items"], saved["monsters"]
        g.memory = [list(row) for row in saved["memory"]]
        g.pack, g.player = saved["pack"], saved["player"]
        return g

    # The level.

    def enter_level(self, depth):
        lv = level.generate(self.rng, depth)
        self.depth = depth
        self.map = lv["map"]
        self.rooms = lv["rooms"]
        self.items = lv["items"]
        self.monsters = lv["monsters"]
        self.memory = [[" "] * level.WIDTH for _ in range(level.HEIGHT)]
        self.sense = False
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
        if self.player["blind"] > 0:
            return False
        px, py = self.player["x"], self.player["y"]
        if abs(x - px) <= 1 and abs(y - py) <= 1:
            return True
        r = self.lit_room()
        return bool(r) and r["x"] <= x < r["x"] + r["w"] and r["y"] <= y < r["y"] + r["h"]

    def see(self):
        if self.player["blind"] > 0:
            return
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

    def free_spot(self):
        while True:
            room = self.rooms[self.rng.below(9)]
            if room["gone"]:
                continue
            x = room["x"] + 1 + self.rng.below(room["w"] - 2)
            y = room["y"] + 1 + self.rng.below(room["h"] - 2)
            if self.map[y][x] == "." and not self.item_at(x, y) and not self.monster_at(x, y):
                return x, y

    # Items and the pack.

    def identified(self, item):
        if item["kind"] == "potion":
            return item["type"] in self.known["potions"]
        if item["kind"] == "scroll":
            return item["type"] in self.known["scrolls"]
        return True

    def item_name(self, item, count):
        n = f"{count} "
        many = count > 1
        kind = item["kind"]
        if kind == "food":
            return n + "food rations" if many else "a food ration"
        if kind == "weapon":
            return f"a {sign(item['bonus'])} {data.WEAPONS[item['type']]['name']}"
        if kind == "armour":
            return f"{sign(item['bonus'])} {data.ARMOURS[item['type']]['name']}"
        if kind == "potion":
            if self.identified(item):
                name = data.POTIONS[item["type"]]["name"]
                return n + "potions of " + name if many else "a potion of " + name
            colour = self.colours[item["type"]]
            return n + colour + " potions" if many else an(colour + " potion")
        if kind == "scroll":
            if self.identified(item):
                name = data.SCROLLS[item["type"]]["name"]
                return n + "scrolls of " + name if many else "a scroll of " + name
            title = f'"{self.titles[item["type"]]}"'
            return n + "scrolls titled " + title if many else "a scroll titled " + title
        return "the Orb"

    def entry(self, letter):
        return next((e for e in self.pack if e["letter"] == letter), None)

    def pack_line(self, e):
        p = self.player
        suffix = " (weapon in hand)" if e["letter"] == p["weapon"] else " (being worn)" if e["letter"] == p["armour"] else ""
        return f"{e['letter']}) {self.item_name(e['item'], e['count'])}{suffix}"

    @staticmethod
    def stacks(a, b):
        return a["kind"] == b["kind"] and (a["kind"] == "food" or (a["kind"] in ("potion", "scroll") and a["type"] == b["type"]))

    def add_to_pack(self, item):
        for e in self.pack:
            if self.stacks(e["item"], item):
                e["count"] += 1
                return e["letter"]
        if len(self.pack) >= PACK_SIZE:
            return None
        used = {e["letter"] for e in self.pack}
        letter = next(c for c in LETTERS if c not in used)
        self.pack.append({"letter": letter, "item": item, "count": 1})
        self.pack.sort(key=lambda e: e["letter"])
        return letter

    def remove_entry(self, letter):
        self.pack = [e for e in self.pack if e["letter"] != letter]
        for slot in ("weapon", "armour"):
            if self.player[slot] == letter:
                self.player[slot] = None

    def take_one(self, letter):
        e = self.entry(letter)
        if e["count"] > 1:
            e["count"] -= 1
        else:
            self.remove_entry(letter)

    def armour_value(self):
        e = self.entry(self.player["armour"])
        return data.ARMOURS[e["item"]["type"]]["armour"] + e["item"]["bonus"] if e else 0

    def pick_up(self):
        p = self.player
        found = self.item_at(p["x"], p["y"])
        if not found:
            return
        item = found["item"]
        if item["kind"] == "gold":
            self.items.remove(found)
            p["gold"] += item["amount"]
            self.say(f"You found {item['amount']} gold pieces.")
            return
        letter = self.add_to_pack(item)
        if letter is None:
            self.say("Your pack is full.")
        else:
            self.items.remove(found)
            self.say(self.pack_line(self.entry(letter)))

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

    def die(self, cause):
        self.dead = True
        self.say("You die.")
        self.say(f"{cause} on depth {self.depth}.")
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
        if not self.hits(k["level"], 0, self.armour_value()):
            self.say(f"The {k['name']} misses.")
            return True
        p["hp"] -= self.rng.roll(*k["damage"])
        self.say(f"The {k['name']} hits you.")
        loot = [e for e in self.pack if e["letter"] not in (p["weapon"], p["armour"])]
        if p["hp"] <= 0:
            self.die(f"Killed by the {k['name']}")
        elif "stealsGold" in k["flags"] and p["gold"] > 0:
            take = self.rng.below(50 + 10 * self.depth) + 2
            p["gold"] = max(0, p["gold"] - take)
            self.monsters.remove(m)
            self.say("Your purse feels lighter.")
            return False
        elif "stealsItem" in k["flags"] and loot:
            e = loot[self.rng.below(len(loot))]
            self.remove_entry(e["letter"])
            self.monsters.remove(m)
            self.say(f"The {k['name']} stole {self.item_name(e['item'], e['count'])}!")
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
        m["state"] = "awake"
        k = self.kind(m)
        p = self.player
        w = self.entry(p["weapon"])
        dice = data.WEAPONS[w["item"]["type"]]["damage"] if w else [1, 2]
        bonus = w["item"]["bonus"] if w else 0
        if self.hits(p["level"], strength_hit(p["str"]) + bonus, k["armour"]):
            damage = max(0, self.rng.roll(*dice) + bonus + strength_damage(p["str"]))
            if m["hp"] - damage <= 0:
                self.monsters.remove(m)
                self.say(f"You defeat the {k['name']}.")
                self.gain_xp(k["xp"])
            else:
                m["hp"] -= damage
                self.say(f"You hit the {k['name']}.")
        else:
            self.say(f"You miss the {k['name']}.")
        if "paralyses" in k["flags"] and p["blind"] == 0 and m in self.monsters:
            self.say(f"You are frozen by the {k['name']}'s gaze!")
            p["frozen"] += self.rng.below(4) + 2

    # Turns.

    def upkeep(self):
        p = self.player
        p["food"] -= 1
        if p["food"] == 150:
            self.say("You are starting to get hungry.")
        if p["food"] == 50:
            self.say("You are starting to feel weak.")
        if p["food"] <= STARVED:
            self.die("Starved to death")
            return
        if p["food"] <= 0 and p["frozen"] == 0 and self.rng.below(5) == 0:
            p["frozen"] += self.rng.below(8) + 4
            self.say("You faint from lack of food.")
        every = max(3, 21 - 2 * p["level"])
        if p["hp"] < p["maxHp"]:
            p["regen"] += 1
            if p["regen"] >= every:
                p["hp"] += 1
                p["regen"] = 0
        else:
            p["regen"] = 0
        if p["confused"] > 0:
            p["confused"] -= 1
            if p["confused"] == 0:
                self.say("You feel less confused now.")
        if p["blind"] > 0:
            p["blind"] -= 1
            if p["blind"] == 0:
                self.say("The darkness lifts.")

    def end_turn(self):
        self.monsters_act()
        if not self.dead:
            self.upkeep()
        self.turn += 1
        self.see()

    def thaw(self):
        while self.player["frozen"] > 0 and not self.dead:
            self.player["frozen"] -= 1
            self.end_turn()

    def step(self, dx, dy):
        x, y = self.player["x"], self.player["y"]
        if not self.step_ok(x, y, dx, dy):
            return False
        m = self.monster_at(x + dx, y + dy)
        if m:
            self.attack(m)
            self.end_turn()
            return False
        self.player["x"], self.player["y"] = x + dx, y + dy
        self.pick_up()
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
        p = self.player
        if self.terrain(p["x"], p["y"]) != "%":
            self.say("I see no way down.")
        elif self.depth < level.DEEPEST:
            self.enter_level(self.depth + 1)
            self.turn += 1
        elif any(e["item"]["kind"] == "orb" for e in self.pack):
            self.won = True
            self.say(f"You escape with the Orb and {p['gold']} gold.")
            self.say("Press enter to play again.")
        else:
            self.say("You need the Orb to leave.")

    # Item commands.

    def quaff(self, t):
        name = data.POTIONS[t]["name"]
        p = self.player
        if name == "healing":
            p["hp"] += self.rng.roll(p["level"], 4)
            if p["hp"] > p["maxHp"]:
                p["maxHp"] += 1
                p["hp"] = p["maxHp"]
            self.say("You begin to feel better.")
        elif name == "poison":
            p["str"] = max(3, p["str"] - (self.rng.below(3) + 1))
            self.say("You feel very sick now.")
        elif name == "confusion":
            p["confused"] += self.rng.below(8) + 8
            self.say("Wait, what's going on here?")
        elif name == "blindness":
            p["blind"] += self.rng.below(50) + 50
            self.say("A cloak of darkness falls around you.")
        elif name == "gain strength":
            p["str"] += 1
            p["maxStr"] = max(p["maxStr"], p["str"])
            self.say("You feel stronger.")
        elif self.monsters:
            self.sense = True
            self.say("You sense the presence of monsters.")
        else:
            self.say("You have a strange feeling for a moment.")

    def read(self, t):
        name = data.SCROLLS[t]["name"]
        p = self.player
        if name == "identify":
            e = next((e for e in self.pack if not self.identified(e["item"])), None)
            if not e:
                self.say("You have nothing to identify.")
            else:
                self.known["potions" if e["item"]["kind"] == "potion" else "scrolls"].append(e["item"]["type"])
                self.say(self.pack_line(e))
        elif name == "teleportation":
            p["x"], p["y"] = self.free_spot()
            self.say("You are somewhere else.")
        elif name == "magic mapping":
            for y in range(level.HEIGHT):
                for x in range(level.WIDTH):
                    if self.memory[y][x] == " ":
                        self.memory[y][x] = self.map[y][x]
            self.say("A map forms in your mind.")
        elif name == "enchant weapon":
            e = self.entry(p["weapon"])
            if not e:
                self.say("Your hands tingle.")
            else:
                e["item"]["bonus"] += 1
                self.say(f"Your {data.WEAPONS[e['item']['type']]['name']} glows blue for a moment.")
        else:
            e = self.entry(p["armour"])
            if not e:
                self.say("Your skin crawls.")
            else:
                e["item"]["bonus"] += 1
                self.say(f"Your {data.ARMOURS[e['item']['type']]['name']} glows silver for a moment.")

    def use_item(self, c, letter):
        verb, kind, nothing, wrong = ITEM_COMMANDS[c]
        e = self.entry(letter)
        p = self.player
        if not e:
            self.say(f"You have no item {letter}.")
            return
        if kind and e["item"]["kind"] != kind:
            self.say(wrong)
            return
        item = e["item"]
        if c == "e":
            self.take_one(letter)
            p["food"] = min(FOOD_MAX, max(p["food"], 0) + 1100 + self.rng.below(400))
            self.say("Yum, that tasted good.")
            self.end_turn()
        elif c in "qr":
            self.take_one(letter)
            known = self.known["potions" if c == "q" else "scrolls"]
            if item["type"] not in known:
                known.append(item["type"])
            (self.quaff if c == "q" else self.read)(item["type"])
            self.end_turn()
        elif c == "w":
            if p["weapon"] == letter:
                self.say("You are already wielding that.")
            else:
                p["weapon"] = letter
                self.say(f"You are now wielding {self.item_name(item, 1)}.")
                self.end_turn()
        elif c == "a":
            if p["armour"] == letter:
                p["armour"] = None
                self.say(f"You took off {self.item_name(item, 1)}.")
                self.end_turn()
            elif p["armour"] is not None:
                self.say("You are already wearing armour.")
            else:
                p["armour"] = letter
                self.say(f"You are now wearing {self.item_name(item, 1)}.")
                self.end_turn()
        else:
            if p["armour"] == letter:
                self.say("You can't drop armour you are wearing.")
            elif self.item_at(p["x"], p["y"]) or self.terrain(p["x"], p["y"]) == "%":
                self.say("There is no room to drop that here.")
            else:
                self.take_one(letter)
                self.items.append({"x": p["x"], "y": p["y"], "item": dict(item)})
                self.say(f"Dropped {self.item_name(item, 1)}.")
                self.end_turn()

    def ask_item(self, c):
        verb, kind, nothing, wrong = ITEM_COMMANDS[c]
        if c == "r" and self.player["blind"] > 0:
            self.say("You can't see to read.")
        elif not any(kind is None or e["item"]["kind"] == kind for e in self.pack):
            self.say(nothing)
        else:
            self.prompt = c
            self.say(f"{verb} which item? (space to cancel)")

    # Commands.

    def command(self, key, shift=False):
        if self.more:
            if key == "space":
                self.messages, self.more = paginate(self.more)
        elif self.dead or self.won:
            if key == "enter":
                self.__init__(self.seed + 1)
        elif self.showing_pack:
            if key == "space":
                self.showing_pack = False
                self.messages = []
        elif self.prompt:
            if key == "space":
                self.prompt = None
                self.messages = []
            elif len(key) == 1:
                c, self.prompt = self.prompt, None
                self.messages = []
                self.use_item(c, key)
                self.thaw()
                self.messages, self.more = paginate(self.messages)
        elif key in ("space", "enter", "i") or key in DIRECTIONS or key in ITEM_COMMANDS:
            self.messages = []
            if key == "space":
                self.end_turn()
            elif key == "enter":
                self.descend()
            elif key == "i":
                if not self.pack:
                    self.say("You are empty-handed.")
                else:
                    self.showing_pack = True
                    self.say("Press space to continue.")
            elif key in DIRECTIONS:
                if self.player["confused"] > 0:
                    self.step(*ORDER[self.rng.below(8)])
                else:
                    (self.run if shift else self.step)(*DIRECTIONS[key])
            else:
                self.ask_item(key)
            self.thaw()
            self.messages, self.more = paginate(self.messages)

    # Output.

    def text(self):
        p = self.player
        if self.showing_pack:
            rows = [self.pack_line(self.pack[i]) if i < len(self.pack) else "" for i in range(level.HEIGHT)]
        else:
            grid = [list(row) for row in self.memory]
            for m in self.monsters:
                if self.sense or self.visible(m["x"], m["y"]):
                    grid[m["y"]][m["x"]] = data.MONSTERS[m["kind"]]["letter"]
            grid[p["y"]][p["x"]] = "@"
            rows = ["".join(r) for r in grid]
        food = p["food"]
        hunger = "  Faint" if food <= 0 else "  Weak" if food <= 50 else "  Hungry" if food <= 150 else ""
        status = (f"Depth:{self.depth}  Gold:{p['gold']}  Hp:{p['hp']}({p['maxHp']})  "
                  f"Str:{p['str']}({p['maxStr']})  Arm:{self.armour_value()}  Exp:{p['level']}/{p['xp']}{hunger}")
        more = " --More--" if self.more else ""
        return ["  ".join(self.messages) + more] + rows + [status]

    def state(self):
        return {
            "seed": self.seed, "turn": self.turn, "depth": self.depth,
            "colours": self.colours, "titles": self.titles, "known": self.known,
            "messages": self.messages, "more": self.more, "prompt": self.prompt,
            "showingPack": self.showing_pack, "dead": self.dead, "won": self.won, "sense": self.sense,
            "map": self.map, "rooms": self.rooms, "items": self.items, "monsters": self.monsters,
            "memory": ["".join(r) for r in self.memory], "pack": self.pack, "player": self.player,
            "rng": self.rng.state,
        }
