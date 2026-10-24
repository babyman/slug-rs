#!/usr/bin/env python3
"""Generates data.slug, the data Crawl needs: the font and the game's tables.

The font is raylib's default font, decoded from the glyph bitmap embedded in
the vendored raylib's rtext.c (LoadFontDefault; zlib licence). Its glyphs
are 10 pixels high with widths of 1 to 7 pixels; each printable ASCII glyph
(32-126) is placed in an 8x10 cell, starting at column (7 - width) // 2 so
that column 7 stays blank between neighbours.

Usage: make_data.py

It writes workspace/data.slug, which agents get, and the identical
reference/data.slug, which the reference imports; and the same data as
data.py in python/workspace and python/reference for the Python arm.
"""
import os
import re

PROJECT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(PROJECT))))
RTEXT = os.path.join(REPO, "clutch/slug.arcade.clutch/native/vendor/raylib/src/rtext.c")

HEIGHT = 10
DIVISOR = 1
ATLAS = 128
FIRST, LAST = 32, 126


def array(source, name):
    """The integers in the C array initialiser `name[...] = { ... };`."""
    match = re.search(name + r"\[\d+\] = \{(.*?)\};", source, re.S)
    return [int(token, 0) for token in re.findall(r"0x[0-9a-fA-F]+|\d+", match.group(1))]


def glyphs():
    """Each ASCII glyph as HEIGHT bytes, bit 0 leftmost, in an 8-wide cell."""
    source = open(RTEXT).read()
    words = array(source, "defaultFontData")
    widths = array(source, "charsWidth")
    # Bit j of word n is atlas pixel n * 32 + j, rows of ATLAS pixels.
    pixel = lambda x, y: (words[(y * ATLAS + x) // 32] >> ((y * ATLAS + x) % 32)) & 1
    # Glyph positions, as LoadFontDefault lays them out in the atlas.
    cells, line, x = [], 0, DIVISOR
    for width in widths:
        if x + width + DIVISOR >= ATLAS:
            line, x = line + 1, DIVISOR
        cells.append((x, DIVISOR + line * (HEIGHT + DIVISOR), width))
        x += width + DIVISOR
    out = []
    for code in range(FIRST, LAST + 1):
        left, top, width = cells[code - FIRST]
        offset = (7 - width) // 2
        rows = []
        for y in range(HEIGHT):
            byte = 0
            for dx in range(width):
                byte |= pixel(left + dx, top + y) << (offset + dx)
            rows.append(byte)
        out.append(rows)
    return out


# Monsters, in the order the spec's depth selection lists them. `armour` is
# higher-is-better (Rogue's 10 - AC); `damage` is [dice, sides]; `depths` is
# the inclusive range of depths where the monster is generated.
MONSTERS = [
    {"letter": "K", "name": "kobold", "level": 1, "armour": 3, "damage": [1, 4], "xp": 1, "depths": [1, 5], "flags": []},
    {"letter": "J", "name": "jackal", "level": 1, "armour": 3, "damage": [1, 2], "xp": 2, "depths": [1, 4], "flags": ["mean"]},
    {"letter": "B", "name": "bat", "level": 1, "armour": 7, "damage": [1, 2], "xp": 1, "depths": [1, 6], "flags": ["flies"]},
    {"letter": "S", "name": "snake", "level": 1, "armour": 5, "damage": [1, 3], "xp": 2, "depths": [1, 6], "flags": ["mean"]},
    {"letter": "H", "name": "hobgoblin", "level": 1, "armour": 5, "damage": [1, 8], "xp": 3, "depths": [2, 7], "flags": ["mean"]},
    {"letter": "E", "name": "floating eye", "level": 1, "armour": 1, "damage": [0, 0], "xp": 5, "depths": [2, 8], "flags": ["still", "paralyses"]},
    {"letter": "O", "name": "orc", "level": 1, "armour": 4, "damage": [1, 8], "xp": 5, "depths": [3, 8], "flags": []},
    {"letter": "L", "name": "leprechaun", "level": 3, "armour": 2, "damage": [1, 1], "xp": 10, "depths": [3, 8], "flags": ["stealsGold"]},
    {"letter": "N", "name": "nymph", "level": 3, "armour": 1, "damage": [0, 0], "xp": 37, "depths": [4, 8], "flags": ["stealsItem"]},
    {"letter": "Z", "name": "zombie", "level": 2, "armour": 2, "damage": [1, 8], "xp": 6, "depths": [4, 8], "flags": ["mean"]},
    {"letter": "T", "name": "troll", "level": 6, "armour": 6, "damage": [2, 6], "xp": 120, "depths": [6, 8], "flags": ["mean", "regenerates"]},
]

# Each item table lists kinds with their generation weights out of 100.
ITEM_KINDS = [
    {"kind": "potion", "weight": 27},
    {"kind": "scroll", "weight": 30},
    {"kind": "food", "weight": 17},
    {"kind": "weapon", "weight": 13},
    {"kind": "armour", "weight": 13},
]

WEAPONS = [
    {"name": "mace", "weight": 25, "damage": [2, 4]},
    {"name": "long sword", "weight": 25, "damage": [3, 4]},
    {"name": "dagger", "weight": 25, "damage": [1, 6]},
    {"name": "two-handed sword", "weight": 25, "damage": [4, 4]},
]

ARMOURS = [
    {"name": "leather armour", "weight": 30, "armour": 2},
    {"name": "ring mail", "weight": 30, "armour": 3},
    {"name": "chain mail", "weight": 25, "armour": 5},
    {"name": "plate mail", "weight": 15, "armour": 7},
]

POTIONS = [
    {"name": "healing", "weight": 25},
    {"name": "poison", "weight": 15},
    {"name": "confusion", "weight": 15},
    {"name": "blindness", "weight": 10},
    {"name": "gain strength", "weight": 15},
    {"name": "see monsters", "weight": 20},
]

SCROLLS = [
    {"name": "identify", "weight": 40},
    {"name": "teleportation", "weight": 15},
    {"name": "magic mapping", "weight": 15},
    {"name": "enchant weapon", "weight": 15},
    {"name": "enchant armour", "weight": 15},
]

# Unidentified names, shuffled at the start of each game; potion kind n gets
# the shuffled list's entry n.
COLOURS = [
    "amber", "black", "blue", "brown", "crimson", "cyan", "golden", "green",
    "grey", "orange", "pink", "purple", "silver", "white", "yellow",
]

TITLES = [
    "zorn plib", "akka tem", "vel urnab", "hoth pek", "snee wobl", "mirk yan",
    "otta brun", "fex lodi", "quam dree", "ulb nott", "grinn sa", "pell vorx",
]


def slug_literal(value, indent=0, schema=None):
    """A Slug literal for JSON-like data, one entry per line for lists of maps,
    which become `schema` structs when one is given."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$") + '"'
    if isinstance(value, dict):
        return (f"{schema} " if schema else "") + "{" + ", ".join(f"{k}: {slug_literal(v)}" for k, v in value.items()) + "}"
    if value and all(isinstance(v, dict) for v in value):
        pad = " " * (indent + 4)
        return "[\n" + "".join(f"{pad}{slug_literal(v, indent + 4, schema)},\n" for v in value) + " " * indent + "]"
    return "[" + ", ".join(slug_literal(v) for v in value) + "]"


def slug_type(value):
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, (int, float)):
        return "num"
    if isinstance(value, str):
        return "str"
    return "list<" + slug_type(value[0]) + ">" if value else "list<str>"


DECLARED = set()


def table(name, value, comment, schema=None):
    """An exported table; a list of maps becomes a list of `schema` structs,
    with the schema declared before its first table."""
    lines = []
    if schema and schema not in DECLARED:
        DECLARED.add(schema)
        fields = ", ".join(f"{k}:{slug_type(v)}" for k, v in value[0].items())
        lines += [f"export val {schema} = struct {{ {fields} }}", ""]
    lines += ["/**", *[f" * {line}" if line else " *" for line in comment], " */"]
    return lines + [f"export val {name} = {slug_literal(value, 0, schema)}", ""]


def hex_value(name, data, comment):
    lines = ["/**", *[f" * {line}" if line else " *" for line in comment], " */"]
    hexes = "".join(f"{b:02x}" for b in data)
    chunks = [hexes[i:i + 80] for i in range(0, len(hexes), 80)]
    lines.append(f"export val {name} =")
    lines.append("    " + " +\n    ".join(f'0x"{c}"' for c in chunks))
    return lines + [""]


def main():
    out = [
        "/**",
        " * Crawl's data: the font and the game's tables. Generated by",
        " * tools/make_data.py; do not edit.",
        " */",
        "",
    ]
    font = [b for rows in glyphs() for b in rows]
    out += hex_value("font", font, [
        f"The font: characters {FIRST} (space) to {LAST} (`~`), {HEIGHT} bytes each, one",
        "per pixel row from the top. Bit 0 of a byte is the leftmost pixel of",
        "the 8-pixel cell, the same order as a slug.arcade frame, so character",
        f"c's row r is `font[(c - {FIRST}) * {HEIGHT} + r]`.",
    ])
    out += table("monsters", MONSTERS, [
        "Monster kinds, in table order. `armour` is higher-is-better, `damage` is",
        "[dice, sides], and `depths` is the inclusive range of depths where the",
        "kind is generated.",
    ], "Monster")
    out += table("itemKinds", ITEM_KINDS, ["Item kinds and their generation weights out of 100."], "ItemKind")
    out += table("weapons", WEAPONS, ["Weapon types: weight out of 100 and damage [dice, sides]."], "Weapon")
    out += table("armours", ARMOURS, ["Armour types: weight out of 100 and armour value."], "Armour")
    out += table("potions", POTIONS, ["Potion types and their weights out of 100."], "Kind")
    out += table("scrolls", SCROLLS, ["Scroll types and their weights out of 100."], "Kind")
    out += table("colours", COLOURS, ["Unidentified potion colours, before the game's shuffle."])
    out += table("titles", TITLES, ["Unidentified scroll titles, before the game's shuffle."])
    for target in ("workspace/data.slug", "reference/data.slug"):
        path = os.path.join(PROJECT, target)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        open(path, "w").write("\n".join(out))
    python = [
        '"""Crawl\'s data for the Python arm: the same font and tables as data.slug.',
        "Generated by tools/make_data.py; do not edit.",
        '"""',
        "",
        f"FONT = bytes.fromhex({bytes(font).hex()!r})",
    ]
    for name, value in [("MONSTERS", MONSTERS), ("ITEM_KINDS", ITEM_KINDS), ("WEAPONS", WEAPONS),
                        ("ARMOURS", ARMOURS), ("POTIONS", POTIONS), ("SCROLLS", SCROLLS),
                        ("COLOURS", COLOURS), ("TITLES", TITLES)]:
        python.append(f"{name} = {value!r}")
    for target in ("python/workspace/data.py", "python/reference/data.py"):
        path = os.path.join(PROJECT, target)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        open(path, "w").write("\n".join(python) + "\n")


if __name__ == "__main__":
    main()
