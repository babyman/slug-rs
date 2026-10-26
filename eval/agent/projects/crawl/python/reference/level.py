"""Crawl's random numbers and level generation, written from task.md alone.

It is the Python arm's reference and the check that the spec is complete:
it must agree with the Slug reference on every seed.
"""
import data

MASK = 0xFFFFFFFF
RANGE = 1 << 32


class Rng:
    def __init__(self, seed):
        self.state = (seed & MASK) or 0x9E3779B9

    def next(self):
        x = self.state
        x ^= (x << 13) & MASK
        x ^= x >> 17
        x ^= (x << 5) & MASK
        self.state = x
        return x

    def below(self, n):
        limit = RANGE - RANGE % n
        while True:
            x = self.next()
            if x < limit:
                return x % n

    def roll(self, dice, sides):
        return sum(self.below(sides) + 1 for _ in range(dice))

    def pick(self, entries):
        r = self.below(sum(e["weight"] for e in entries))
        total = 0
        for i, e in enumerate(entries):
            total += e["weight"]
            if r < total:
                return i

    def shuffle(self, items):
        items = list(items)
        for i in range(len(items) - 1, 0, -1):
            j = self.below(i + 1)
            items[i], items[j] = items[j], items[i]
        return items


WIDTH, HEIGHT, DEEPEST = 80, 22, 8


def adjacent(a, b):
    if a // 3 == b // 3 and abs(a - b) == 1:
        return True
    return a % 3 == b % 3 and abs(a - b) == 3


def bonus(rng):
    b = rng.below(100)
    return -1 if b < 10 else 1 if b < 25 else 0


def new_item(rng):
    kind = data.ITEM_KINDS[rng.pick(data.ITEM_KINDS)]["kind"]
    if kind == "potion":
        return {"kind": kind, "type": rng.pick(data.POTIONS)}
    if kind == "scroll":
        return {"kind": kind, "type": rng.pick(data.SCROLLS)}
    if kind == "food":
        return {"kind": kind}
    table = data.WEAPONS if kind == "weapon" else data.ARMOURS
    item_type = rng.pick(table)
    return {"kind": kind, "type": item_type, "bonus": bonus(rng)}


def generate(rng, depth):
    grid = [[" "] * WIDTH for _ in range(HEIGHT)]
    items, monsters, rooms = [], [], []

    def free(x, y):
        return (grid[y][x] == "." and all((i["x"], i["y"]) != (x, y) for i in items)
                and all((m["x"], m["y"]) != (x, y) for m in monsters))

    def spot_in(room):
        while True:
            x = room["x"] + 1 + rng.below(room["w"] - 2)
            y = room["y"] + 1 + rng.below(room["h"] - 2)
            if free(x, y):
                return x, y

    def spot_anywhere():
        while True:
            room = rooms[rng.below(9)]
            if room["gone"]:
                continue
            x = room["x"] + 1 + rng.below(room["w"] - 2)
            y = room["y"] + 1 + rng.below(room["h"] - 2)
            if free(x, y):
                return x, y

    # 1. Gone cells.
    gone = []
    for _ in range(rng.below(4)):
        c = rng.below(9)
        while c in gone:
            c = rng.below(9)
        gone.append(c)

    # 2. Rooms.
    kinds = [i for i, m in enumerate(data.MONSTERS) if m["depths"][0] <= depth <= m["depths"][1]]
    for c in range(9):
        left, top = 26 * (c % 3), 7 * (c // 3)
        if c in gone:
            x = left + 1 + rng.below(23)
            y = top + 1 + rng.below(4)
            rooms.append({"x": x, "y": y, "w": 1, "h": 1, "gone": True, "dark": False})
            continue
        dark = rng.below(10) < depth - 1
        w = 4 + rng.below(22)
        h = 4 + rng.below(3)
        x = left + rng.below(26 - w)
        y = top + rng.below(7 - h)
        room = {"x": x, "y": y, "w": w, "h": h, "gone": False, "dark": dark}
        rooms.append(room)
        for col in range(x, x + w):
            grid[y][col] = grid[y + h - 1][col] = "-"
        for row in range(y + 1, y + h - 1):
            grid[row][x] = grid[row][x + w - 1] = "|"
            for col in range(x + 1, x + w - 1):
                grid[row][col] = "."
        gold = rng.below(2) == 0
        if gold:
            amount = rng.below(50 + 10 * depth) + 2
            gx, gy = spot_in(room)
            items.append({"x": gx, "y": gy, "item": {"kind": "gold", "amount": amount}})
        if rng.below(100) < (80 if gold else 25):
            mx, my = spot_in(room)
            kind = kinds[rng.below(len(kinds))]
            hp = rng.roll(data.MONSTERS[kind]["level"], 8)
            monsters.append({"x": mx, "y": my, "kind": kind, "hp": hp, "maxHp": hp, "state": "asleep"})

    # 3. Corridors.
    joined = set()

    def carve(x, y):
        if grid[y][x] == " ":
            grid[y][x] = "#"

    def end(room, x, y):
        grid[y][x] = "#" if room["gone"] else "+"

    def join(r, other):
        a, b = min(r, other), max(r, other)
        ra, rb = rooms[a], rooms[b]
        if b == a + 1:
            s = (ra["x"], ra["y"]) if ra["gone"] else (ra["x"] + ra["w"] - 1, ra["y"] + 1 + rng.below(ra["h"] - 2))
            e = (rb["x"], rb["y"]) if rb["gone"] else (rb["x"], rb["y"] + 1 + rng.below(rb["h"] - 2))
            end(ra, *s)
            end(rb, *e)
            t = s[0] + 1 + rng.below(e[0] - s[0] - 1)
            path = [(x, s[1]) for x in range(s[0] + 1, t + 1)]
            step = 1 if e[1] > s[1] else -1
            path += [(t, y) for y in range(s[1] + step, e[1] + step, step)] if e[1] != s[1] else []
            path += [(x, e[1]) for x in range(t + 1, e[0])]
        else:
            s = (ra["x"], ra["y"]) if ra["gone"] else (ra["x"] + 1 + rng.below(ra["w"] - 2), ra["y"] + ra["h"] - 1)
            e = (rb["x"], rb["y"]) if rb["gone"] else (rb["x"] + 1 + rng.below(rb["w"] - 2), rb["y"])
            end(ra, *s)
            end(rb, *e)
            t = s[1] + 1 + rng.below(e[1] - s[1] - 1)
            path = [(s[0], y) for y in range(s[1] + 1, t + 1)]
            step = 1 if e[0] > s[0] else -1
            path += [(x, t) for x in range(s[0] + step, e[0] + step, step)] if e[0] != s[0] else []
            path += [(e[0], y) for y in range(t + 1, e[1])]
        for x, y in path:
            carve(x, y)
        joined.add((a, b))

    def reservoir(r, qualifies):
        j, choice = 0, None
        for i in range(9):
            if adjacent(r, i) and qualifies(i):
                j += 1
                if rng.below(j) == 0:
                    choice = i
        return choice

    r = rng.below(9)
    graph = {r}
    while len(graph) < 9:
        other = reservoir(r, lambda i: i not in graph)
        if other is None:
            r = rng.below(9)
            while r not in graph:
                r = rng.below(9)
        else:
            join(r, other)
            graph.add(other)
    for _ in range(rng.below(5)):
        r = rng.below(9)
        other = reservoir(r, lambda i: (min(r, i), max(r, i)) not in joined)
        if other is not None:
            join(r, other)

    # 4. Items.
    for _ in range(9):
        if rng.below(100) < 36:
            item = new_item(rng)
            x, y = spot_anywhere()
            items.append({"x": x, "y": y, "item": item})
    if depth == DEEPEST:
        x, y = spot_anywhere()
        items.append({"x": x, "y": y, "item": {"kind": "orb"}})

    # 5. Stairs and the player.
    x, y = spot_anywhere()
    grid[y][x] = "%"
    px, py = spot_anywhere()
    return {
        "map": ["".join(row) for row in grid],
        "rooms": rooms,
        "items": items,
        "monsters": monsters,
        "player": {"x": px, "y": py},
    }


def new_game(seed, depth=1):
    """The game for `seed` after taking the stairs down to `depth`."""
    rng = Rng(seed)
    colours = rng.shuffle(data.COLOURS)
    titles = rng.shuffle(data.TITLES)
    for d in range(1, depth + 1):
        level = generate(rng, d)
    return dict(level, colours=colours, titles=titles, rng=rng.state)
